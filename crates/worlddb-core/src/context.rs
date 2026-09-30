//! Typed assertion context and epistemic partition keys.

use std::fmt;

use crate::ids::{HistorySpaceId, LayerId, PerspectiveId};

/// The world itself or one explicitly identified in-world perspective.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PerspectiveScope {
    /// Perspective-free world-state partition.
    World,
    /// One stable project Perspective identity.
    Perspective(PerspectiveId),
}

/// A closed epistemic partition; modes never imply one another.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum EpistemicMode {
    /// A proposition in the world-state partition.
    WorldState,
    /// A proposition explicitly represented as known by a Perspective.
    Knows,
    /// A proposition explicitly represented as believed by a Perspective.
    Believes,
    /// A proposition explicitly represented as claimed by a Perspective.
    Claims,
}

/// One concrete HistorySpace/Layer/Perspective/Epistemic context.
///
/// This is the assertion/masking context after a `LayerSelection` has been
/// resolved to an explicit `LayerId`. Event contexts have their own shape and
/// do not acquire a Perspective or EpistemicMode through this type.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ContextKey {
    history_space_id: HistorySpaceId,
    layer_id: LayerId,
    perspective_scope: PerspectiveScope,
    epistemic_mode: EpistemicMode,
}

impl ContextKey {
    /// Creates a context only for a valid PerspectiveScope/EpistemicMode pair.
    pub fn new(
        history_space_id: HistorySpaceId,
        layer_id: LayerId,
        perspective_scope: PerspectiveScope,
        epistemic_mode: EpistemicMode,
    ) -> Result<Self, ContextError> {
        let valid = matches!(
            (perspective_scope, epistemic_mode),
            (PerspectiveScope::World, EpistemicMode::WorldState)
                | (
                    PerspectiveScope::Perspective(_),
                    EpistemicMode::Knows | EpistemicMode::Believes | EpistemicMode::Claims
                )
        );
        if !valid {
            return Err(ContextError::InvalidPerspectiveModePair {
                perspective_scope,
                epistemic_mode,
            });
        }
        Ok(Self {
            history_space_id,
            layer_id,
            perspective_scope,
            epistemic_mode,
        })
    }

    /// Returns the HistorySpace component.
    #[must_use]
    pub const fn history_space_id(self) -> HistorySpaceId {
        self.history_space_id
    }

    /// Returns the concrete Layer component.
    #[must_use]
    pub const fn layer_id(self) -> LayerId {
        self.layer_id
    }

    /// Returns the explicit epistemic scope.
    #[must_use]
    pub const fn perspective_scope(self) -> PerspectiveScope {
        self.perspective_scope
    }

    /// Returns the explicit epistemic mode.
    #[must_use]
    pub const fn epistemic_mode(self) -> EpistemicMode {
        self.epistemic_mode
    }
}

/// Invalid combination of PerspectiveScope and EpistemicMode.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ContextError {
    /// `WorldState` requires `World`; `Knows`, `Believes`, and `Claims` require a Perspective.
    InvalidPerspectiveModePair {
        /// Requested epistemic scope.
        perspective_scope: PerspectiveScope,
        /// Requested epistemic mode.
        epistemic_mode: EpistemicMode,
    },
}

impl fmt::Display for ContextError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPerspectiveModePair {
                perspective_scope,
                epistemic_mode,
            } => write!(
                formatter,
                "epistemic mode {epistemic_mode:?} is incompatible with scope {perspective_scope:?}"
            ),
        }
    }
}

impl std::error::Error for ContextError {}

#[cfg(test)]
mod tests {
    use super::{ContextError, ContextKey, EpistemicMode, PerspectiveScope};
    use crate::ids::{DomainId, HistorySpaceId, IdValidationError, LayerId, PerspectiveId};
    use std::error::Error;
    use std::fmt;

    type TestResult = Result<(), TestError>;

    #[derive(Debug)]
    enum TestError {
        Id(IdValidationError),
        Context(ContextError),
    }

    impl fmt::Display for TestError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Self::Id(error) => write!(formatter, "{error}"),
                Self::Context(error) => write!(formatter, "{error}"),
            }
        }
    }

    impl Error for TestError {}

    impl From<IdValidationError> for TestError {
        fn from(error: IdValidationError) -> Self {
            Self::Id(error)
        }
    }

    impl From<ContextError> for TestError {
        fn from(error: ContextError) -> Self {
            Self::Context(error)
        }
    }

    macro_rules! value {
        ($result:expr) => {
            match $result {
                Ok(value) => value,
                Err(error) => return Err(error.into()),
            }
        };
    }

    fn uuid<T: DomainId>(byte: u8) -> Result<T, IdValidationError> {
        let mut bytes = [0_u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[15] = byte;
        T::try_from_bytes(bytes)
    }

    #[test]
    fn epistemic_modes_accept_only_their_matching_scope() -> TestResult {
        let history_space = value!(uuid::<HistorySpaceId>(1));
        let layer = value!(uuid::<LayerId>(2));
        let perspective_id = value!(uuid::<PerspectiveId>(3));
        let perspective = PerspectiveScope::Perspective(perspective_id);

        let world_state = ContextKey::new(
            history_space,
            layer,
            PerspectiveScope::World,
            EpistemicMode::WorldState,
        )?;
        assert_eq!(world_state.perspective_scope(), PerspectiveScope::World);
        assert_eq!(world_state.epistemic_mode(), EpistemicMode::WorldState);
        assert_eq!(world_state.history_space_id(), history_space);
        assert_eq!(world_state.layer_id(), layer);

        for mode in [
            EpistemicMode::Knows,
            EpistemicMode::Believes,
            EpistemicMode::Claims,
        ] {
            let context = ContextKey::new(history_space, layer, perspective, mode)?;
            assert_eq!(context.perspective_scope(), perspective);
            assert_eq!(context.epistemic_mode(), mode);
        }

        for mode in [
            EpistemicMode::Knows,
            EpistemicMode::Believes,
            EpistemicMode::Claims,
        ] {
            assert_eq!(
                ContextKey::new(history_space, layer, PerspectiveScope::World, mode).err(),
                Some(ContextError::InvalidPerspectiveModePair {
                    perspective_scope: PerspectiveScope::World,
                    epistemic_mode: mode,
                })
            );
        }
        assert_eq!(
            ContextKey::new(history_space, layer, perspective, EpistemicMode::WorldState).err(),
            Some(ContextError::InvalidPerspectiveModePair {
                perspective_scope: perspective,
                epistemic_mode: EpistemicMode::WorldState,
            })
        );
        Ok(())
    }
}
