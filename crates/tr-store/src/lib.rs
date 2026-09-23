//! Single background writer. Index data never overwrites durable annotations.
use anyhow::{Context, Result, bail, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use std::{
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tr_core::Annotation;
use tr_core::decoder::RawEngine;
use tr_core::editing::EditRecipe;
use tr_core::location::{Favorite, Location};
use uuid::Uuid;

pub struct Catalog {
    pub library: Connection,
    index: Connection,
    root: PathBuf,
}
pub struct StoredAsset {
    pub id: String,
    pub annotation: Annotation,
    pub revision: u64,
}
#[derive(Debug, Clone)]
pub struct LoadedEdit {
    pub asset_id: String,
    pub source_digest: String,
    pub generation: u64,
    pub revision: u64,
    pub recipe: EditRecipe,
    pub can_undo: bool,
    pub can_redo: bool,
}
pub enum FavoriteEdit {
    Add { location: Location, label: String },
    Rename { id: String, label: String },
    Move { id: String, index: usize },
    Remove(String),
}
pub fn sqlite_version() -> &'static str {
    rusqlite::version()
}

fn configure(connection: &Connection, durable: bool) -> Result<()> {
    connection.busy_timeout(Duration::from_secs(2))?;
    connection.execute_batch("PRAGMA foreign_keys=ON; PRAGMA trusted_schema=OFF; PRAGMA journal_mode=WAL; PRAGMA mmap_size=0; PRAGMA cache_size=-8192; PRAGMA wal_autocheckpoint=256;")?;
    connection.execute_batch(if durable {
        "PRAGMA synchronous=FULL;"
    } else {
        "PRAGMA synchronous=NORMAL;"
    })?;
    #[cfg(target_os = "macos")]
    if durable {
        connection.execute_batch("PRAGMA fullfsync=ON;")?;
    }
    Ok(())
}
fn path_key(path: &Path) -> Vec<u8> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        path.as_os_str().as_bytes().to_vec()
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        path.as_os_str()
            .encode_wide()
            .flat_map(u16::to_le_bytes)
            .collect()
    }
}
impl Catalog {
    pub fn open(root: &Path) -> Result<Self> {
        ensure!(
            rusqlite::version_number() >= 3_051_003,
            "SQLite privo della baseline WAL-reset richiesta"
        );
        std::fs::create_dir_all(root.join("backups"))?;
        let library = Connection::open(root.join("library.sqlite"))?;
        let version: u32 = library.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        ensure!(
            version <= 3,
            "Libreria di una versione più recente: apertura interrotta"
        );
        configure(&library, true)?;
        if version == 0 {
            let tables: u32 = library.query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table'",
                [],
                |r| r.get(0),
            )?;
            ensure!(
                tables == 0,
                "Schema legacy non riconosciuto: nessuna migrazione automatica"
            );
            library.execute_batch(&format!(
                "BEGIN IMMEDIATE; {} COMMIT;",
                include_str!("library.sql")
            ))?;
        }
        // A consistent verified v1 snapshot must exist before the migration.
        if version <= 1 {
            backup_connection(&library, root)?;
            library.execute_batch(
                "BEGIN IMMEDIATE;
                CREATE TABLE browser_favorite (
                    id TEXT PRIMARY KEY,
                    label TEXT NOT NULL CHECK(length(label)<=256),
                    location TEXT NOT NULL,
                    position INTEGER NOT NULL,
                    revision INTEGER NOT NULL CHECK(revision>=0)
                );
                PRAGMA user_version=2;
                COMMIT;",
            )?;
        }
        if version <= 2 {
            backup_connection(&library, root)?;
            library.execute_batch(
                "BEGIN IMMEDIATE;
                CREATE TABLE photo_edit_head (
                    asset_id TEXT PRIMARY KEY REFERENCES asset(id),
                    source_digest TEXT NOT NULL,
                    generation INTEGER NOT NULL CHECK(generation>=0),
                    cursor INTEGER NOT NULL CHECK(cursor>=0),
                    next_revision INTEGER NOT NULL CHECK(next_revision>=1),
                    redo TEXT NOT NULL CHECK(length(redo)<=16384)
                );
                CREATE TABLE photo_edit_revision (
                    asset_id TEXT NOT NULL REFERENCES asset(id),
                    revision INTEGER NOT NULL CHECK(revision>=0),
                    parent_revision INTEGER,
                    source_digest TEXT NOT NULL,
                    recipe TEXT NOT NULL CHECK(length(recipe)<=1048576),
                    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                    PRIMARY KEY(asset_id,revision)
                );
                PRAGMA user_version=3;
                COMMIT;",
            )?;
        }
        let index = Connection::open(root.join("index.sqlite"))?;
        configure(&index, false)?;
        let index_version: u32 = index.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        ensure!(index_version <= 1, "Indice di una versione più recente");
        if index_version == 0 {
            index.execute_batch(include_str!("index.sql"))?;
        }
        let catalog = Self {
            library,
            index,
            root: root.into(),
        };
        catalog.integrity()?;
        catalog.backup()?;
        Ok(catalog)
    }
    pub fn observe(&mut self, path: &Path, digest: &str, bytes: u64) -> Result<StoredAsset> {
        let key = path_key(path);
        let existing: Option<String> = self
            .library
            .query_row(
                "SELECT asset_id FROM asset_location WHERE path_key=?1 AND content_hash=?2",
                params![key, digest],
                |r| r.get(0),
            )
            .optional()?;
        let id = if let Some(id) = existing {
            id
        } else {
            // Equal contents in different locations are not proof of a shared asset identity.
            let id = Uuid::new_v4().to_string();
            let annotation = serde_json::to_string(&Annotation::default())?;
            let tx = self.library.transaction()?;
            tx.execute(
                "INSERT INTO asset VALUES (?1,?2,?3,0,?4,0)",
                params![id, digest, path.to_string_lossy(), annotation],
            )?;
            tx.execute("INSERT INTO asset_revision(asset_id,revision,annotation,origin,change_id) VALUES (?1,0,?2,'initial',?3)", params![id, annotation, Uuid::new_v4().to_string()])?;
            tx.execute("INSERT INTO asset_location VALUES (?1,?2,?3) ON CONFLICT(path_key) DO UPDATE SET asset_id=excluded.asset_id,content_hash=excluded.content_hash", params![key, id, digest])?;
            tx.commit()?;
            id
        };
        self.index.execute("INSERT INTO item(path_key,asset_id,display_name,size,content_hash) VALUES (?1,?2,?3,?4,?5) ON CONFLICT(path_key) DO UPDATE SET asset_id=excluded.asset_id,size=excluded.size,content_hash=excluded.content_hash,seen_at=CURRENT_TIMESTAMP", params![key, id, path.file_name().unwrap_or_default().to_string_lossy(), i64::try_from(bytes)?, digest])?;
        self.get(&id)
    }
    pub fn get(&self, id: &str) -> Result<StoredAsset> {
        let (text, revision): (String, i64) = self.library.query_row(
            "SELECT annotation, revision FROM asset WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let annotation: Annotation = serde_json::from_str(&text)?;
        annotation.validate()?;
        Ok(StoredAsset {
            id: id.into(),
            annotation,
            revision: revision.try_into()?,
        })
    }
    pub fn save(
        &mut self,
        id: &str,
        expected: u64,
        annotation: &Annotation,
        undo: bool,
    ) -> Result<u64> {
        annotation.validate()?;
        let next = expected.checked_add(1).context("Overflow revisione")?;
        let expected_sql = i64::try_from(expected)?;
        let next_sql = i64::try_from(next)?;
        let json = serde_json::to_string(annotation)?;
        let change_id = Uuid::new_v4().to_string();
        let tx = self.library.transaction()?;
        let changed = tx.execute(
            "UPDATE asset SET annotation=?1,rating=?2,revision=?3 WHERE id=?4 AND revision=?5",
            params![json, annotation.rating, next_sql, id, expected_sql],
        )?;
        if changed != 1 {
            bail!("Conflitto: la revisione della foto è cambiata. Ricaricare prima di riprovare.");
        }
        tx.execute("INSERT INTO asset_revision(asset_id,revision,annotation,origin,change_id) VALUES (?1,?2,?3,?4,?5)", params![id, next_sql, json, if undo { "undo" } else { "user" }, change_id])?;
        tx.execute(
            "INSERT INTO effect_journal VALUES (?1,?2,?3,'library_annotation','committed')",
            params![change_id, id, next_sql],
        )?;
        tx.commit()?;
        Ok(next)
    }
    pub fn backup(&self) -> Result<PathBuf> {
        backup_connection(&self.library, &self.root)
    }
    /// The asset identity is checked independently of any derived index entry.
    pub fn load_edit(&self, id: &str, initial_engine: RawEngine) -> Result<LoadedEdit> {
        let digest: String =
            self.library
                .query_row("SELECT content_hash FROM asset WHERE id=?1", [id], |r| {
                    r.get(0)
                })?;
        let head: Option<(i64,i64,String,String)> = self.library.query_row(
            "SELECT generation,cursor,redo,source_digest FROM photo_edit_head WHERE asset_id=?1",
            [id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
        if let Some((generation, cursor, redo, source_digest)) = head {
            ensure!(
                digest == source_digest,
                "Ricetta associata a una sorgente diversa"
            );
            let (json,parent): (String,Option<i64>) = self.library.query_row(
                "SELECT recipe,parent_revision FROM photo_edit_revision WHERE asset_id=?1 AND revision=?2",
                params![id,cursor], |r| Ok((r.get(0)?,r.get(1)?)))?;
            let recipe: EditRecipe = serde_json::from_str(&json)?;
            recipe.validate()?;
            let redo: Vec<u64> = serde_json::from_str(&redo)?;
            ensure!(redo.len() <= 200, "Cronologia fotografica fuori quota");
            Ok(LoadedEdit {
                asset_id: id.into(),
                source_digest,
                generation: generation.try_into()?,
                revision: cursor.try_into()?,
                recipe,
                can_undo: parent.is_some(),
                can_redo: !redo.is_empty(),
            })
        } else {
            Ok(LoadedEdit {
                asset_id: id.into(),
                source_digest: digest,
                generation: 0,
                revision: 0,
                recipe: EditRecipe::neutral(initial_engine),
                can_undo: false,
                can_redo: false,
            })
        }
    }
    pub fn save_edit(
        &mut self,
        id: &str,
        expected_generation: u64,
        recipe: &EditRecipe,
    ) -> Result<LoadedEdit> {
        recipe.validate()?;
        let before = self.load_edit(id, recipe.raw_engine)?;
        ensure!(
            before.generation == expected_generation,
            "Conflitto: ricetta fotografica aggiornata altrove"
        );
        let next_generation = before
            .generation
            .checked_add(1)
            .context("Generazione esaurita")?;
        let tx = self.library.transaction()?;
        let next_revision: i64 = tx
            .query_row(
                "SELECT next_revision FROM photo_edit_head WHERE asset_id=?1",
                [id],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(1);
        let digest = &before.source_digest;
        if before.generation == 0 {
            tx.execute("INSERT INTO photo_edit_revision(asset_id,revision,parent_revision,source_digest,recipe) VALUES (?1,0,NULL,?2,?3)",
                params![id,digest,serde_json::to_string(&EditRecipe::neutral(recipe.raw_engine))?])?;
            tx.execute(
                "INSERT INTO photo_edit_head VALUES (?1,?2,0,0,1,'[]')",
                params![id, digest],
            )?;
        }
        tx.execute("INSERT INTO photo_edit_revision(asset_id,revision,parent_revision,source_digest,recipe) VALUES (?1,?2,?3,?4,?5)",
            params![id,next_revision,i64::try_from(before.revision)?,digest,serde_json::to_string(recipe)?])?;
        let changed = tx.execute("UPDATE photo_edit_head SET generation=?1,cursor=?2,next_revision=?3,redo='[]' WHERE asset_id=?4 AND generation=?5 AND source_digest=?6",
            params![i64::try_from(next_generation)?,next_revision,next_revision+1,id,i64::try_from(expected_generation)?,digest])?;
        ensure!(
            changed == 1,
            "Conflitto: ricetta fotografica aggiornata altrove"
        );
        tx.commit()?;
        self.load_edit(id, recipe.raw_engine)
    }
    pub fn step_edit(
        &mut self,
        id: &str,
        expected_generation: u64,
        undo: bool,
    ) -> Result<LoadedEdit> {
        let before = self.load_edit(id, RawEngine::default())?;
        ensure!(
            before.generation == expected_generation,
            "Conflitto: cronologia fotografica aggiornata altrove"
        );
        let (mut redo, cursor): (String, i64) = self.library.query_row(
            "SELECT redo,cursor FROM photo_edit_head WHERE asset_id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let mut stack: Vec<u64> = serde_json::from_str(&redo)?;
        ensure!(stack.len() <= 200, "Cronologia fotografica fuori quota");
        let target =
            if undo {
                let parent: Option<i64> = self.library.query_row(
                "SELECT parent_revision FROM photo_edit_revision WHERE asset_id=?1 AND revision=?2",
                params![id,cursor], |r| r.get(0))?;
                let parent = parent.context("Nessuna modifica da annullare")?;
                stack.push(cursor.try_into()?);
                parent
            } else {
                i64::try_from(stack.pop().context("Nessuna modifica da ripetere")?)?
            };
        redo = serde_json::to_string(&stack)?;
        let changed = self.library.execute("UPDATE photo_edit_head SET generation=generation+1,cursor=?1,redo=?2 WHERE asset_id=?3 AND generation=?4",
            params![target,redo,id,i64::try_from(expected_generation)?])?;
        ensure!(
            changed == 1,
            "Conflitto: cronologia fotografica aggiornata altrove"
        );
        self.load_edit(id, before.recipe.raw_engine)
    }
    pub fn favorites(&self) -> Result<Vec<Favorite>> {
        let mut stmt = self.library.prepare(
            "SELECT id,label,location,revision FROM browser_favorite ORDER BY position,id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })?;
        let mut favorites = Vec::new();
        for row in rows {
            let (id, label, json, revision) = row?;
            let location: Location = serde_json::from_str(&json)?;
            location.validate()?;
            favorites.push(Favorite {
                id,
                label,
                location,
                revision: revision.try_into()?,
            });
        }
        ensure!(favorites.len() <= 200, "Favorite count limit");
        Ok(favorites)
    }
    pub fn edit_favorite(&mut self, edit: FavoriteEdit) -> Result<Vec<Favorite>> {
        let mut favorites = self.favorites()?;
        match edit {
            FavoriteEdit::Add { location, label } => {
                location.validate()?;
                ensure!(label.chars().count() <= 256, "Favorite label limit");
                ensure!(favorites.len() < 200, "Favorite count limit (200)");
                if favorites.iter().any(|f| f.location == location) {
                    return Ok(favorites);
                }
                favorites.push(Favorite {
                    id: Uuid::new_v4().to_string(),
                    label,
                    location,
                    revision: 0,
                });
            }
            FavoriteEdit::Rename { id, label } => {
                ensure!(label.chars().count() <= 256, "Favorite label limit");
                let f = favorites
                    .iter_mut()
                    .find(|f| f.id == id)
                    .context("Favorite missing")?;
                f.label = label;
                f.revision += 1;
            }
            FavoriteEdit::Move { id, index } => {
                let from = favorites
                    .iter()
                    .position(|f| f.id == id)
                    .context("Favorite missing")?;
                let mut f = favorites.remove(from);
                f.revision += 1;
                favorites.insert(index.min(favorites.len()), f);
            }
            FavoriteEdit::Remove(id) => favorites.retain(|f| f.id != id),
        }
        let tx = self.library.transaction()?;
        tx.execute("DELETE FROM browser_favorite", [])?;
        for (index, favorite) in favorites.iter().enumerate() {
            tx.execute(
                "INSERT INTO browser_favorite VALUES (?1,?2,?3,?4,?5)",
                params![
                    favorite.id,
                    favorite.label,
                    serde_json::to_string(&favorite.location)?,
                    index as i64,
                    i64::try_from(favorite.revision)?
                ],
            )?;
        }
        tx.commit()?;
        Ok(favorites)
    }
    pub fn export_json(&self, path: &Path) -> Result<()> {
        use std::io::Write;
        let mut stmt = self.library.prepare(
            "SELECT id, content_hash, last_path_hint, annotation, revision FROM asset ORDER BY id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
            ))
        })?;
        let mut assets = vec![];
        for row in rows {
            let (id, hash, path, state, revision) = row?;
            assets.push(serde_json::json!({"id":id,"hash":hash,"path_hint":path,"annotation":serde_json::from_str::<Annotation>(&state)?,"revision":revision}));
        }
        let output = serde_json::to_vec_pretty(
            &serde_json::json!({"application":"TrueRenderer","schema":2,"kind":"R0 library snapshot (not the v1 portable archive)","assets":assets,"favorites":self.favorites()?}),
        )?;
        // A new export is never silently allowed to replace an existing file.
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        file.write_all(&output)?;
        file.sync_all()?;
        Ok(())
    }
    pub fn integrity(&self) -> Result<()> {
        for conn in [&self.library, &self.index] {
            let result: String = conn.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
            ensure!(result == "ok", "Integrità SQLite: {result}");
            let violations: u32 =
                conn.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
                    r.get(0)
                })?;
            ensure!(violations == 0, "Relazioni SQLite incoerenti");
        }
        Ok(())
    }
}

fn backup_connection(library: &Connection, root: &Path) -> Result<PathBuf> {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let path = root
        .join("backups")
        .join(format!("library-{stamp}-{}.sqlite", Uuid::new_v4()));
    library.backup("main", &path, None)?;
    let check = Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let status: String = check.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    ensure!(status == "ok", "Backup non integro: {status}");
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn photo_edits_survive_reopen_backup_and_history_branch() {
        let dir = tempfile::tempdir().unwrap();
        let mut catalog = Catalog::open(dir.path()).unwrap();
        let item = catalog
            .observe(Path::new("/synthetic/photo.png"), "source-a", 4)
            .unwrap();
        let engine = RawEngine::default();
        let mut recipe = EditRecipe::neutral(engine);
        recipe.exposure_ev = 1.;
        let first = catalog.save_edit(&item.id, 0, &recipe).unwrap();
        assert_eq!(first.revision, 1);
        recipe.exposure_ev = 2.;
        let second = catalog
            .save_edit(&item.id, first.generation, &recipe)
            .unwrap();
        assert_eq!(second.revision, 2);
        let undone = catalog
            .step_edit(&item.id, second.generation, true)
            .unwrap();
        assert_eq!(undone.recipe.exposure_ev, 1.);
        assert!(undone.can_redo);
        let backup = catalog.backup().unwrap();
        drop(catalog);
        let restored = tempfile::tempdir().unwrap();
        std::fs::copy(backup, restored.path().join("library.sqlite")).unwrap();
        let mut catalog = Catalog::open(restored.path()).unwrap();
        let reopened = catalog.load_edit(&item.id, engine).unwrap();
        assert_eq!(reopened.recipe, undone.recipe);
        let redone = catalog
            .step_edit(&item.id, reopened.generation, false)
            .unwrap();
        assert_eq!(redone.recipe.exposure_ev, 2.);
        let undo_again = catalog
            .step_edit(&item.id, redone.generation, true)
            .unwrap();
        recipe.exposure_ev = -1.;
        let branch = catalog
            .save_edit(&item.id, undo_again.generation, &recipe)
            .unwrap();
        assert!(!branch.can_redo);
        assert!(
            catalog
                .save_edit(&item.id, undo_again.generation, &recipe)
                .is_err()
        );
        assert_eq!(
            catalog
                .load_edit(&item.id, engine)
                .unwrap()
                .recipe
                .exposure_ev,
            -1.
        );
    }
    #[test]
    fn migration_preserves_v1_backup_and_favorites_survive_export_restore() {
        let dir = tempfile::tempdir().unwrap();
        let v1 = Connection::open(dir.path().join("library.sqlite")).unwrap();
        v1.execute_batch(include_str!("library.sql")).unwrap();
        drop(v1);
        let mut catalog = Catalog::open(dir.path()).unwrap();
        let backups: Vec<_> = std::fs::read_dir(dir.path().join("backups"))
            .unwrap()
            .map(|p| p.unwrap().path())
            .filter(|path| path.extension().is_some_and(|e| e == "sqlite"))
            .collect();
        assert!(backups.iter().any(|p| {
            Connection::open(p)
                .unwrap()
                .query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
                .unwrap()
                == 1
        }));
        let first = catalog
            .edit_favorite(FavoriteEdit::Add {
                location: Location::from_path(Path::new("/synthetic/first")),
                label: "First".into(),
            })
            .unwrap()[0]
            .clone();
        catalog
            .edit_favorite(FavoriteEdit::Add {
                location: Location::from_path(Path::new("/synthetic/second")),
                label: "Second".into(),
            })
            .unwrap();
        catalog
            .edit_favorite(FavoriteEdit::Rename {
                id: first.id.clone(),
                label: "Renamed".into(),
            })
            .unwrap();
        let expected = catalog
            .edit_favorite(FavoriteEdit::Move {
                id: first.id,
                index: 1,
            })
            .unwrap();
        assert_eq!(expected[1].label, "Renamed");
        assert!(
            catalog
                .edit_favorite(FavoriteEdit::Rename {
                    id: expected[0].id.clone(),
                    label: "x".repeat(257)
                })
                .is_err()
        );
        assert_eq!(catalog.favorites().unwrap(), expected);
        let export = dir.path().join("export.json");
        catalog.export_json(&export).unwrap();
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(export).unwrap()).unwrap();
        assert_eq!(value["favorites"].as_array().unwrap().len(), 2);
        let backup = catalog.backup().unwrap();
        drop(catalog);
        let restore = tempfile::tempdir().unwrap();
        std::fs::copy(backup, restore.path().join("library.sqlite")).unwrap();
        assert_eq!(
            Catalog::open(restore.path()).unwrap().favorites().unwrap(),
            expected
        );
    }
    #[test]
    fn future_library_is_rejected_before_pragmas_or_migration() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.sqlite");
        let connection = Connection::open(&path).unwrap();
        connection.execute_batch("PRAGMA user_version=99; CREATE TABLE future_data(value TEXT); INSERT INTO future_data VALUES('keep');").unwrap();
        drop(connection);
        let before = std::fs::read(&path).unwrap();
        assert!(Catalog::open(dir.path()).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
    #[test]
    fn durable_state_survives_index_deletion_and_restore() {
        let dir = tempfile::tempdir().unwrap();
        let mut catalog = Catalog::open(dir.path()).unwrap();
        let item = catalog
            .observe(Path::new("/corpus/a.png"), "hash-a", 100)
            .unwrap();
        let annotation = Annotation {
            rating: 5,
            ..Default::default()
        };
        catalog.save(&item.id, 0, &annotation, false).unwrap();
        let backup = catalog.backup().unwrap();
        drop(catalog);
        std::fs::remove_file(dir.path().join("index.sqlite")).unwrap();
        let mut catalog = Catalog::open(dir.path()).unwrap();
        let restored = catalog
            .observe(Path::new("/corpus/a.png"), "hash-a", 100)
            .unwrap();
        assert_eq!(restored.id, item.id);
        assert_eq!(restored.annotation.rating, 5);
        let mut destination = Connection::open_in_memory().unwrap();
        destination
            .restore("main", backup, None::<fn(rusqlite::backup::Progress)>)
            .unwrap();
        assert_eq!(
            destination
                .query_row("SELECT rating FROM asset", [], |r| r.get::<_, i32>(0))
                .unwrap(),
            5
        );
    }
    #[test]
    fn stale_revision_rolls_back_every_table_and_undo_increments() {
        let dir = tempfile::tempdir().unwrap();
        let mut catalog = Catalog::open(dir.path()).unwrap();
        let item = catalog.observe(Path::new("/a.png"), "a", 10).unwrap();
        let a = Annotation {
            rating: 3,
            ..Default::default()
        };
        catalog.save(&item.id, 0, &a, false).unwrap();
        assert!(
            catalog
                .save(&item.id, 0, &Annotation::default(), false)
                .is_err()
        );
        assert_eq!(catalog.get(&item.id).unwrap().revision, 1);
        assert_eq!(
            catalog
                .library
                .query_row("SELECT count(*) FROM effect_journal", [], |r| r
                    .get::<_, u32>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            catalog
                .save(&item.id, 1, &Annotation::default(), true)
                .unwrap(),
            2
        );
        catalog.integrity().unwrap();
    }
    #[test]
    fn replacement_and_equal_content_copy_have_distinct_identity() {
        let dir = tempfile::tempdir().unwrap();
        let mut catalog = Catalog::open(dir.path()).unwrap();
        let a = catalog.observe(Path::new("/a.png"), "a", 10).unwrap();
        let b = catalog.observe(Path::new("/b.png"), "a", 10).unwrap();
        let replacement = catalog.observe(Path::new("/a.png"), "b", 10).unwrap();
        assert_ne!(a.id, b.id);
        assert_ne!(a.id, replacement.id);
    }
}
