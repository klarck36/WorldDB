//! Reusable conformance checks for revision backends.

use worlddb_core::{
    Revision, RevisionBackend, RevisionLogError, StorageBackend, StorageFeatureSet,
};

/// Asserts the logical storage capabilities and revision behavior expected by
/// the in-memory reference contract.
///
/// This does not assert persistent durability. A backend may use this helper
/// for logical conformance while still reporting `Memory` durability.
#[track_caller]
pub fn assert_storage_backend_contract<B>(backend: B) -> Result<(), RevisionLogError>
where
    B: StorageBackend<u8>,
{
    assert!(
        backend
            .storage_capabilities()
            .features()
            .contains_all(StorageFeatureSet::logical_reference()),
        "backend must advertise the logical reference capabilities"
    );
    assert_revision_backend_contract(backend)
}

/// Asserts the common revision publication and historical-read contract.
///
/// Backend-specific tests should call this with their own initial backend
/// value. It checks that a complete batch receives one revision, that the
/// visible head advances sequentially, and that later writes do not alter an
/// earlier historical read.
#[track_caller]
pub fn assert_revision_backend_contract<B>(mut backend: B) -> Result<(), RevisionLogError>
where
    B: RevisionBackend<u8>,
{
    assert_eq!(backend.latest_published(), Revision::GENESIS);
    assert!(backend.read_at(Revision::GENESIS)?.next().is_none());
    assert!(backend.read_at(Revision::FIRST_COMMIT).is_err());

    let first = backend.publish(vec![1, 2, 3])?;
    assert_eq!(
        first,
        Revision::FIRST_COMMIT,
        "one complete batch must advance exactly one revision"
    );
    assert_eq!(backend.latest_published(), first);
    assert_eq!(
        backend.read_at(first)?.collect::<Vec<_>>(),
        vec![(first, &1), (first, &2), (first, &3)],
        "one complete batch must appear at exactly one revision"
    );

    let second = backend.publish(vec![4, 5])?;
    assert_eq!(second, first.next_commit()?);
    assert_eq!(backend.latest_published(), second);
    assert_eq!(
        backend.read_at(first)?.collect::<Vec<_>>(),
        vec![(first, &1), (first, &2), (first, &3)],
        "publishing later revisions must not change historical reads"
    );
    assert_eq!(
        backend.read_at(second)?.collect::<Vec<_>>(),
        vec![
            (first, &1),
            (first, &2),
            (first, &3),
            (second, &4),
            (second, &5)
        ]
    );
    Ok(())
}
