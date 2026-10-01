use worlddb_core::{InMemoryRevisionBackend, Revision, RevisionBackend};
use worlddb_testkit::backend_contract::assert_revision_backend_contract;
use worlddb_testkit::sqlite_reference::SqliteReferenceBackend;

#[test]
fn sqlite_reference_satisfies_the_revision_backend_contract() -> Result<(), String> {
    let backend = SqliteReferenceBackend::open_in_memory().map_err(|error| error.to_string())?;
    assert_revision_backend_contract(backend).map_err(|error| error.to_string())
}

#[test]
fn sqlite_and_in_memory_backends_return_identical_logical_history() -> Result<(), String> {
    let mut sqlite = SqliteReferenceBackend::open_in_memory().map_err(|error| error.to_string())?;
    let mut model = InMemoryRevisionBackend::new();
    let batches = [vec![4_u8, 1, 9], Vec::new(), vec![0, 255, 4]];
    for batch in batches {
        let sqlite_revision = sqlite
            .publish(batch.clone())
            .map_err(|error| error.to_string())?;
        let model_revision = model.publish(batch).map_err(|error| error.to_string())?;
        assert_eq!(sqlite_revision, model_revision);
        assert_eq!(sqlite.latest_published(), model.latest_published());

        let mut as_of = Revision::GENESIS;
        while as_of <= sqlite_revision {
            let sqlite_history = sqlite
                .read_at(as_of)
                .map_err(|error| error.to_string())?
                .map(|(revision, value)| (revision, *value))
                .collect::<Vec<_>>();
            let model_history = model
                .read_at(as_of)
                .map_err(|error| error.to_string())?
                .map(|(revision, value)| (revision, *value))
                .collect::<Vec<_>>();
            assert_eq!(sqlite_history, model_history);
            if as_of == sqlite_revision {
                break;
            }
            as_of = as_of.next_commit().map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}
