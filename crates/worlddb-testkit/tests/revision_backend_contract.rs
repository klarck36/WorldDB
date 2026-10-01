//! Contract tests for engine/backend responsibility and revision visibility.

use worlddb_core::{
    CancellablePublishError, CommitCancellation, InMemoryRevisionBackend, Revision,
    RevisionBackend, RevisionLogError,
};
use worlddb_testkit::backend_contract::{
    assert_revision_backend_contract, assert_storage_backend_contract,
};

#[test]
fn in_memory_backend_satisfies_revision_contract() -> Result<(), RevisionLogError> {
    assert_revision_backend_contract(InMemoryRevisionBackend::new())
}

#[test]
fn in_memory_backend_satisfies_logical_storage_contract() -> Result<(), RevisionLogError> {
    assert_storage_backend_contract(InMemoryRevisionBackend::new())
}

#[test]
#[should_panic(expected = "one complete batch must advance exactly one revision")]
fn revision_contract_rejects_a_backend_that_splits_a_batch_across_revisions() {
    let _ = assert_revision_backend_contract(SplittingBackend::default());
}

#[derive(Default)]
struct SplittingBackend {
    inner: InMemoryRevisionBackend<u8>,
}

impl RevisionBackend<u8> for SplittingBackend {
    type Read<'a>
        = std::vec::IntoIter<(Revision, &'a u8)>
    where
        Self: 'a,
        u8: 'a;

    fn latest_published(&self) -> Revision {
        self.inner.latest_published()
    }

    fn publish(&mut self, entries: Vec<u8>) -> Result<Revision, RevisionLogError> {
        let mut revision = self.inner.latest_published();
        for entry in entries {
            revision = self.inner.publish(vec![entry])?;
        }
        Ok(revision)
    }

    fn publish_cancellable(
        &mut self,
        entries: Vec<u8>,
        cancellation: &CommitCancellation,
    ) -> Result<Revision, CancellablePublishError> {
        let permit = cancellation
            .begin_commitpoint()
            .map_err(CancellablePublishError::Commitpoint)?;
        match self.publish(entries) {
            Ok(revision) => {
                permit.committed();
                Ok(revision)
            }
            Err(error) => {
                permit.not_committed();
                Err(CancellablePublishError::Publish(error))
            }
        }
    }

    fn read_at(&self, revision: Revision) -> Result<Self::Read<'_>, RevisionLogError> {
        Ok(self
            .inner
            .read_at(revision)?
            .collect::<Vec<_>>()
            .into_iter())
    }
}

#[derive(Debug, Eq, PartialEq)]
enum ModelEngineError {
    InvalidValue,
    Backend(RevisionLogError),
}

struct ModelEngine<B> {
    backend: B,
}

impl<B> ModelEngine<B>
where
    B: RevisionBackend<u8>,
{
    fn commit(&mut self, batch: Vec<u8>) -> Result<Revision, ModelEngineError> {
        // This fixture rule stands for engine validation; it is intentionally
        // absent from the backend contract.
        if batch.contains(&0) {
            return Err(ModelEngineError::InvalidValue);
        }
        self.backend
            .publish(batch)
            .map_err(ModelEngineError::Backend)
    }
}

#[test]
fn engine_validation_rejects_invalid_value_before_backend_call() {
    let mut engine = ModelEngine {
        backend: CountingBackend::default(),
    };
    assert_eq!(
        engine.commit(vec![0, 1]),
        Err(ModelEngineError::InvalidValue),
        "engine validation must reject before calling publish"
    );
    assert_eq!(engine.backend.publish_calls, 0);
    assert_eq!(engine.backend.latest_published(), Revision::GENESIS);
}

#[test]
fn valid_engine_batch_calls_backend_once_for_revision_assignment() -> Result<(), String> {
    let mut engine = ModelEngine {
        backend: CountingBackend::default(),
    };
    let revision = engine
        .commit(vec![1, 2])
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(revision, Revision::FIRST_COMMIT);
    assert_eq!(engine.backend.publish_calls, 1);
    assert_eq!(engine.backend.latest_published(), revision);
    Ok(())
}

#[derive(Default)]
struct CountingBackend {
    inner: InMemoryRevisionBackend<u8>,
    publish_calls: usize,
}

impl RevisionBackend<u8> for CountingBackend {
    type Read<'a>
        = std::vec::IntoIter<(Revision, &'a u8)>
    where
        Self: 'a,
        u8: 'a;

    fn latest_published(&self) -> Revision {
        self.inner.latest_published()
    }

    fn publish(&mut self, entries: Vec<u8>) -> Result<Revision, RevisionLogError> {
        self.publish_calls += 1;
        self.inner.publish(entries)
    }

    fn publish_cancellable(
        &mut self,
        entries: Vec<u8>,
        cancellation: &CommitCancellation,
    ) -> Result<Revision, CancellablePublishError> {
        let permit = cancellation
            .begin_commitpoint()
            .map_err(CancellablePublishError::Commitpoint)?;
        match self.publish(entries) {
            Ok(revision) => {
                permit.committed();
                Ok(revision)
            }
            Err(error) => {
                permit.not_committed();
                Err(CancellablePublishError::Publish(error))
            }
        }
    }

    fn read_at(&self, revision: Revision) -> Result<Self::Read<'_>, RevisionLogError> {
        Ok(self
            .inner
            .read_at(revision)?
            .collect::<Vec<_>>()
            .into_iter())
    }
}
