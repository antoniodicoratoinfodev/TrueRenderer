//! Single background writer. Index data never overwrites durable annotations.
use anyhow::{Context, Result, bail, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use std::{
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tr_core::Annotation;
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
        configure(&library, true)?;
        let version: u32 = library.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        ensure!(
            version <= 1,
            "Libreria di una versione più recente: apertura interrotta"
        );
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
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = self
            .root
            .join("backups")
            .join(format!("library-{stamp}-{}.sqlite", Uuid::new_v4()));
        self.library.backup("main", &path, None)?;
        let check = Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let status: String = check.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        ensure!(status == "ok", "Backup non integro: {status}");
        Ok(path)
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
            &serde_json::json!({"application":"TrueRenderer","schema":1,"kind":"R0 library snapshot (not the v1 portable archive)","assets":assets}),
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

#[cfg(test)]
mod tests {
    use super::*;
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
