use super::*;
use tr_core::editing::looks::{Look, MAX_LOOK_BYTES};

const MAX_LOOKS: usize = 128;

#[derive(Clone, Debug, PartialEq)]
pub struct SavedLook {
    pub id: String,
    pub name: String,
    pub revision: u64,
    pub archived: bool,
    pub look: Look,
}

pub enum LookEdit {
    Create {
        name: String,
        look: Look,
    },
    Rename {
        id: String,
        expected: u64,
        name: String,
    },
    Archive {
        id: String,
        expected: u64,
        archived: bool,
    },
}
fn validate_name(name: &str) -> Result<()> {
    ensure!(
        !name.is_empty()
            && name.trim() == name
            && name.len() <= 128
            && !name.chars().any(char::is_control),
        "Nome look non valido"
    );
    Ok(())
}
impl Catalog {
    pub fn looks(&self) -> Result<Vec<SavedLook>> {
        let mut query = self.library.prepare("SELECT id,name,revision,archived,payload FROM photo_look ORDER BY name COLLATE NOCASE,id LIMIT 129")?;
        let mut rows = query.query([])?;
        let mut looks = Vec::new();
        while let Some(row) = rows.next()? {
            let json: String = row.get(4)?;
            ensure!(json.len() <= MAX_LOOK_BYTES, "Look oltre 48 KiB");
            let look: Look = serde_json::from_str(&json)?;
            look.validate()?;
            let name: String = row.get(1)?;
            validate_name(&name)?;
            let id: String = row.get(0)?;
            ensure!(!Uuid::parse_str(&id)?.is_nil(), "Identità look non valida");
            looks.push(SavedLook {
                id,
                name,
                revision: u64::try_from(row.get::<_, i64>(2)?)?,
                archived: row.get(3)?,
                look,
            });
            ensure!(
                looks.len() <= MAX_LOOKS,
                "Massimo 128 look salvati, inclusi gli archiviati"
            );
        }
        Ok(looks)
    }

    pub fn edit_look(&mut self, edit: LookEdit) -> Result<Vec<SavedLook>> {
        // Validate the library before writing, including a bounded list size.
        let existing = self.looks()?;
        let tx = self.library.transaction()?;
        match edit {
            LookEdit::Create { name, look } => {
                validate_name(&name)?;
                look.validate()?;
                let count: i64 =
                    tx.query_row("SELECT count(*) FROM photo_look", [], |r| r.get(0))?;
                ensure!(
                    count < MAX_LOOKS as i64,
                    "Massimo 128 look salvati, inclusi gli archiviati"
                );
                tx.execute(
                    "INSERT INTO photo_look VALUES (?1,?2,0,0,?3)",
                    params![
                        Uuid::new_v4().to_string(),
                        name,
                        serde_json::to_string(&look)?
                    ],
                )?;
            }
            LookEdit::Rename { id, expected, name } => {
                validate_name(&name)?;
                let current = existing
                    .iter()
                    .find(|l| l.id == id)
                    .context("Look non disponibile")?;
                ensure!(
                    current.revision == expected,
                    "Look cambiato: aggiorna l’elenco"
                );
                if current.name != name {
                    ensure!(tx.execute("UPDATE photo_look SET name=?1,revision=revision+1 WHERE id=?2 AND revision=?3", params![name,id,i64::try_from(expected)?])? == 1, "Look cambiato: aggiorna l’elenco");
                }
            }
            LookEdit::Archive {
                id,
                expected,
                archived,
            } => {
                let current = existing
                    .iter()
                    .find(|l| l.id == id)
                    .context("Look non disponibile")?;
                ensure!(
                    current.revision == expected,
                    "Look cambiato: aggiorna l’elenco"
                );
                if current.archived != archived {
                    ensure!(tx.execute("UPDATE photo_look SET archived=?1,revision=revision+1 WHERE id=?2 AND revision=?3", params![archived,id,i64::try_from(expected)?])? == 1, "Look cambiato: aggiorna l’elenco");
                }
            }
        }
        tx.commit()?;
        self.looks()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tr_core::editing::layers::{Colorize, Layer, Operator};

    fn look() -> Look {
        let mut r = EditRecipe::neutral(Default::default());
        r.layer_stack().layers.push(Layer::new(
            "Tone",
            Operator::Colorize(Colorize {
                amount: 20.,
                ..Default::default()
            }),
        ));
        Look::capture(&r, &[r.layers.as_ref().unwrap().layers[0].id], false).unwrap()
    }
    #[test]
    fn look_migration_has_v4_backup_and_restore_retains_looks_without_sources_or_index() {
        let dir = tempfile::tempdir().unwrap();
        let catalog = Catalog::open(dir.path()).unwrap();
        catalog
            .library
            .execute_batch("DROP TABLE photo_look; PRAGMA user_version=4;")
            .unwrap();
        drop(catalog);
        let mut catalog = Catalog::open(dir.path()).unwrap();
        assert!(
            std::fs::read_dir(dir.path().join("backups"))
                .unwrap()
                .flatten()
                .filter(|p| p.path().extension().is_some_and(|e| e == "sqlite"))
                .any(|p| {
                    Connection::open(p.path())
                        .unwrap()
                        .query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
                        .unwrap()
                        == 4
                })
        );
        let created = catalog
            .edit_look(LookEdit::Create {
                name: "Warm".into(),
                look: look(),
            })
            .unwrap();
        let backup = catalog.backup().unwrap();
        drop(catalog);
        let restored = tempfile::tempdir().unwrap();
        std::fs::copy(backup, restored.path().join("library.sqlite")).unwrap();
        let mut catalog = Catalog::open(restored.path()).unwrap();
        assert_eq!(catalog.looks().unwrap(), created);
        let item = catalog
            .observe(Path::new("/synthetic/offline.png"), "synthetic", 4)
            .unwrap();
        let initial = catalog.load_edit(&item.id, Default::default()).unwrap();
        let look = &created[0].look;
        let applied = look
            .append_to(&initial.recipe, &[look.layers[0].id], 1., false)
            .unwrap();
        let saved = catalog
            .save_edit(&item.id, initial.generation, &applied)
            .unwrap();
        let undone = catalog.step_edit(&item.id, saved.generation, true).unwrap();
        assert_eq!(undone.recipe, initial.recipe);
        let redone = catalog
            .step_edit(&item.id, undone.generation, false)
            .unwrap();
        assert_eq!(redone.recipe, applied);
        assert_eq!(catalog.looks().unwrap(), created);
    }
    #[test]
    fn look_rename_archive_restore_conflicts_and_duplicate_names_preserve_independent_copies() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = Catalog::open(dir.path()).unwrap();
        let first = c
            .edit_look(LookEdit::Create {
                name: "Same".into(),
                look: look(),
            })
            .unwrap()[0]
            .clone();
        let both = c
            .edit_look(LookEdit::Create {
                name: "Same".into(),
                look: first.look.clone(),
            })
            .unwrap();
        assert_eq!(both.len(), 2);
        assert_ne!(both[0].id, both[1].id);
        assert_eq!(
            c.edit_look(LookEdit::Rename {
                id: first.id.clone(),
                expected: 0,
                name: first.name.clone()
            })
            .unwrap(),
            both
        );
        c.edit_look(LookEdit::Rename {
            id: first.id.clone(),
            expected: 0,
            name: "Renamed".into(),
        })
        .unwrap();
        assert!(
            c.edit_look(LookEdit::Archive {
                id: first.id.clone(),
                expected: 0,
                archived: true
            })
            .is_err()
        );
        c.edit_look(LookEdit::Archive {
            id: first.id.clone(),
            expected: 1,
            archived: true,
        })
        .unwrap();
        drop(c);
        let mut c = Catalog::open(dir.path()).unwrap();
        let archived = c
            .looks()
            .unwrap()
            .into_iter()
            .find(|l| l.id == first.id)
            .unwrap();
        assert!(archived.archived);
        assert_eq!(archived.look, first.look);
        let result = c
            .edit_look(LookEdit::Archive {
                id: first.id.clone(),
                expected: 2,
                archived: false,
            })
            .unwrap();
        let restored = result.iter().find(|l| l.id == first.id).unwrap();
        assert!(!restored.archived);
        assert_eq!(restored.revision, 3);
        assert_eq!(restored.look, first.look);
    }
    #[test]
    fn failed_look_writes_and_invalid_content_leave_library_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = Catalog::open(dir.path()).unwrap();
        for name in ["", " leading", "line\nbreak"] {
            assert!(
                c.edit_look(LookEdit::Create {
                    name: name.into(),
                    look: look()
                })
                .is_err()
            );
        }
        let mut invalid = look();
        invalid.version = 99;
        assert!(
            c.edit_look(LookEdit::Create {
                name: "Future".into(),
                look: invalid
            })
            .is_err()
        );
        c.library.execute_batch("CREATE TRIGGER fail_look BEFORE INSERT ON photo_look BEGIN SELECT RAISE(ABORT,'injected disk failure'); END;").unwrap();
        assert!(
            c.edit_look(LookEdit::Create {
                name: "Valid".into(),
                look: look()
            })
            .is_err()
        );
        assert!(c.looks().unwrap().is_empty());
        c.library.execute_batch("DROP TRIGGER fail_look;").unwrap();
        let before = c
            .edit_look(LookEdit::Create {
                name: "Valid".into(),
                look: look(),
            })
            .unwrap();
        c.library.execute_batch("CREATE TRIGGER fail_rename BEFORE UPDATE ON photo_look BEGIN SELECT RAISE(ABORT,'injected disk failure'); END;").unwrap();
        assert!(
            c.edit_look(LookEdit::Rename {
                id: before[0].id.clone(),
                expected: 0,
                name: "Change".into()
            })
            .is_err()
        );
        assert_eq!(c.looks().unwrap(), before);
    }
}
