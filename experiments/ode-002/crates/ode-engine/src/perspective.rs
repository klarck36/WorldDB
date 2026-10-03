//! Authenticated Perspective definitions and epistemic context binding.

use std::collections::BTreeMap;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use worlddb_core::{
    PerspectiveCatalogSnapshot, PerspectiveDefinitionRevision, PerspectiveId,
    PerspectiveRetirementId, Revision, SchemaMode,
};
use worlddb_storage_file::FileProjectMetadataManager;

use crate::{EngineError, EngineHost, SchemaModeInput};

/// Epistemic mode selected for one input or query context.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EpistemicModeInput {
    /// Perspective-free assertions about the world state.
    WorldState,
    /// Assertions represented as known by one explicit Perspective.
    Knows,
    /// Assertions represented as believed by one explicit Perspective.
    Believes,
    /// Assertions represented as claimed by one explicit Perspective.
    Claims,
}

impl EpistemicModeInput {
    const fn is_perspective_scoped(self) -> bool {
        !matches!(self, Self::WorldState)
    }
}

/// One closed Perspective catalog or context action from an authenticated window.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum PerspectiveCommand {
    /// Reads the project Perspective catalog at a selected shared revision.
    Snapshot { mode: SchemaModeInput },
    /// Creates one host-identified Perspective definition.
    Create {
        expected_base_revision: u64,
        display_name: Option<String>,
        description: Option<String>,
    },
    /// Appends metadata under one existing Perspective identity.
    Update {
        expected_base_revision: u64,
        perspective_id: String,
        display_name: Option<String>,
        description: Option<String>,
    },
    /// Retires one active Perspective without removing its history.
    Retire {
        expected_base_revision: u64,
        perspective_id: String,
    },
    /// Validates one independent input or query context binding.
    ValidateContext {
        epistemic_mode: EpistemicModeInput,
        perspective_id: Option<String>,
    },
}

/// Result of one authenticated Perspective operation.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PerspectiveResponse {
    Snapshot(PerspectiveSnapshotView),
    Published(PerspectivePublicationView),
    ContextBound(PerspectiveContextView),
}

/// Perspective metadata and lifecycle state pinned to one shared revision.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PerspectiveSnapshotView {
    pub revision: u64,
    pub perspectives: Vec<PerspectiveView>,
}

/// One stable project Perspective identity and its visible metadata history.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PerspectiveView {
    /// Private identity used by typed host commands and HTML select options.
    pub perspective_id: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub created_revision: u64,
    pub metadata_history: Vec<PerspectiveDefinitionView>,
    pub retired_revision: Option<u64>,
}

/// One immutable metadata revision for a Perspective.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PerspectiveDefinitionView {
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub recorded_revision: u64,
}

/// Safe outcome of one committed Perspective catalog mutation.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PerspectivePublicationView {
    pub operation_id: String,
    pub revision: u64,
    pub perspective_id: Option<String>,
    pub retirement_id: Option<String>,
}

/// Bound epistemic context; identity is kept internal and no user role is implied.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PerspectiveContextView {
    pub epistemic_mode: EpistemicModeInput,
    pub perspective_label: Option<String>,
}

impl EngineHost {
    /// Executes an authenticated Perspective catalog or context operation.
    pub fn perspectives(
        &self,
        command: PerspectiveCommand,
    ) -> Result<PerspectiveResponse, EngineError> {
        let _guard = self.schema_management.lock().map_err(|_| {
            EngineError::Perspective("Perspective catalog manager state is unavailable".to_owned())
        })?;
        let principal = self.principal_id.ok_or_else(|| {
            EngineError::Perspective("an authenticated project session is required".to_owned())
        })?;
        let mut manager =
            FileProjectMetadataManager::open(self._layout.clone(), &self._writer_lock, principal)
                .map_err(|error| EngineError::Perspective(error.to_string()))?;

        match command {
            PerspectiveCommand::Snapshot { mode } => {
                let (mode, recorded_as_of) = selected_mode(mode)?;
                let catalog = manager
                    .perspectives_at(mode, recorded_as_of)
                    .map_err(|error| EngineError::Perspective(error.to_string()))?;
                Ok(PerspectiveResponse::Snapshot(perspective_snapshot_view(
                    &catalog,
                )))
            }
            PerspectiveCommand::Create {
                expected_base_revision,
                display_name,
                description,
            } => {
                let receipt = manager
                    .create_perspective(
                        revision_from_u64(expected_base_revision)?,
                        worlddb_core::storage_internal::generate_schema_management_operation_id()
                            .map_err(|_| {
                            EngineError::Perspective("operation identity is unavailable".to_owned())
                        })?,
                        worlddb_core::storage_internal::generate_project_bootstrap_id::<
                            PerspectiveId,
                        >()
                        .map_err(|_| {
                            EngineError::Perspective(
                                "Perspective identity is unavailable".to_owned(),
                            )
                        })?,
                        display_name,
                        description,
                    )
                    .map_err(|error| EngineError::Perspective(error.to_string()))?;
                Ok(PerspectiveResponse::Published(PerspectivePublicationView {
                    operation_id: receipt.operation_id().to_string(),
                    revision: receipt.revision().value(),
                    perspective_id: receipt.perspective_id().map(|id| id.to_string()),
                    retirement_id: None,
                }))
            }
            PerspectiveCommand::Update {
                expected_base_revision,
                perspective_id,
                display_name,
                description,
            } => {
                let perspective_id = parse_perspective_id(&perspective_id)?;
                let receipt = manager
                    .update_perspective(
                        revision_from_u64(expected_base_revision)?,
                        worlddb_core::storage_internal::generate_schema_management_operation_id()
                            .map_err(|_| {
                            EngineError::Perspective("operation identity is unavailable".to_owned())
                        })?,
                        perspective_id,
                        display_name,
                        description,
                    )
                    .map_err(|error| EngineError::Perspective(error.to_string()))?;
                Ok(PerspectiveResponse::Published(PerspectivePublicationView {
                    operation_id: receipt.operation_id().to_string(),
                    revision: receipt.revision().value(),
                    perspective_id: receipt.perspective_id().map(|id| id.to_string()),
                    retirement_id: None,
                }))
            }
            PerspectiveCommand::Retire {
                expected_base_revision,
                perspective_id,
            } => {
                let perspective_id = parse_perspective_id(&perspective_id)?;
                let receipt = manager
                    .retire_perspective(
                        revision_from_u64(expected_base_revision)?,
                        worlddb_core::storage_internal::generate_schema_management_operation_id()
                            .map_err(|_| {
                            EngineError::Perspective("operation identity is unavailable".to_owned())
                        })?,
                        worlddb_core::storage_internal::generate_project_bootstrap_id::<
                            PerspectiveRetirementId,
                        >()
                        .map_err(|_| {
                            EngineError::Perspective(
                                "retirement identity is unavailable".to_owned(),
                            )
                        })?,
                        perspective_id,
                    )
                    .map_err(|error| EngineError::Perspective(error.to_string()))?;
                Ok(PerspectiveResponse::Published(PerspectivePublicationView {
                    operation_id: receipt.operation_id().to_string(),
                    revision: receipt.revision().value(),
                    perspective_id: receipt.perspective_id().map(|id| id.to_string()),
                    retirement_id: receipt.perspective_retirement_id().map(|id| id.to_string()),
                }))
            }
            PerspectiveCommand::ValidateContext {
                epistemic_mode,
                perspective_id,
            } => {
                let perspective_id = selected_perspective(epistemic_mode, perspective_id)?;
                let perspective_label = perspective_id
                    .map(|id| {
                        manager
                            .validate_perspective_use(id)
                            .map(|definition| perspective_display_label(&definition))
                            .map_err(|error| EngineError::Perspective(error.to_string()))
                    })
                    .transpose()?;
                Ok(PerspectiveResponse::ContextBound(PerspectiveContextView {
                    epistemic_mode,
                    perspective_label,
                }))
            }
        }
    }
}

fn selected_mode(input: SchemaModeInput) -> Result<(SchemaMode, Revision), EngineError> {
    match input {
        SchemaModeInput::Current => Ok((SchemaMode::Current, Revision::GENESIS)),
        SchemaModeInput::Historical { recorded_as_of } => {
            Ok((SchemaMode::Historical, revision_from_u64(recorded_as_of)?))
        }
        SchemaModeInput::Explicit { revision } => {
            let revision = revision_from_u64(revision)?;
            Ok((
                SchemaMode::Explicit(worlddb_core::SchemaRevision::from_published_revision(
                    revision,
                )),
                revision,
            ))
        }
    }
}

fn revision_from_u64(value: u64) -> Result<Revision, EngineError> {
    Revision::try_from(value)
        .map_err(|_| EngineError::Perspective("revision is outside the supported range".to_owned()))
}

fn parse_perspective_id(value: &str) -> Result<PerspectiveId, EngineError> {
    PerspectiveId::from_str(value)
        .map_err(|_| EngineError::Perspective("invalid Perspective identity".to_owned()))
}

fn selected_perspective(
    mode: EpistemicModeInput,
    perspective_id: Option<String>,
) -> Result<Option<PerspectiveId>, EngineError> {
    match (mode.is_perspective_scoped(), perspective_id) {
        (false, None) => Ok(None),
        (false, Some(_)) => Err(EngineError::Perspective(
            "WorldState cannot be bound to a Perspective".to_owned(),
        )),
        (true, None) => Err(EngineError::Perspective(
            "Knows, Believes, and Claims require an explicit Perspective".to_owned(),
        )),
        (true, Some(value)) => parse_perspective_id(&value).map(Some),
    }
}

fn perspective_display_label(definition: &PerspectiveDefinitionRevision) -> String {
    definition
        .display_name()
        .map(str::to_owned)
        .unwrap_or_else(|| {
            format!(
                "Unbenannte Perspektive · Revision {}",
                definition.recorded_revision().value()
            )
        })
}

fn perspective_snapshot_view(catalog: &PerspectiveCatalogSnapshot) -> PerspectiveSnapshotView {
    let mut grouped = BTreeMap::<PerspectiveId, Vec<&PerspectiveDefinitionRevision>>::new();
    for definition in catalog.definitions() {
        grouped
            .entry(definition.perspective_id())
            .or_default()
            .push(definition);
    }
    let perspectives = grouped
        .into_iter()
        .filter_map(|(perspective_id, history)| {
            let first = history.first()?;
            let latest = history.last()?;
            Some(PerspectiveView {
                perspective_id: perspective_id.to_string(),
                display_name: latest.display_name().map(str::to_owned),
                description: latest.description().map(str::to_owned),
                created_revision: first.recorded_revision().value(),
                metadata_history: history
                    .into_iter()
                    .map(|definition| PerspectiveDefinitionView {
                        display_name: definition.display_name().map(str::to_owned),
                        description: definition.description().map(str::to_owned),
                        recorded_revision: definition.recorded_revision().value(),
                    })
                    .collect(),
                retired_revision: catalog
                    .retirements()
                    .iter()
                    .find(|retirement| retirement.perspective_id() == perspective_id)
                    .map(|retirement| retirement.created_revision().value()),
            })
        })
        .collect();
    PerspectiveSnapshotView {
        revision: catalog.revision().value(),
        perspectives,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::str::FromStr;
    use std::sync::atomic::{AtomicU64, Ordering};

    use worlddb_core::PrincipalId;

    use super::{
        EpistemicModeInput, PerspectiveCommand, PerspectiveContextView, PerspectiveResponse,
    };
    use crate::{EngineHost, create_project};

    static NEXT_PROJECT: AtomicU64 = AtomicU64::new(0);

    fn project_root() -> PathBuf {
        std::env::temp_dir().join(format!(
            "worlddb-ode-perspectives-{}-{}",
            std::process::id(),
            NEXT_PROJECT.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn principal() -> Result<PrincipalId, String> {
        PrincipalId::from_str("00000000-0000-7000-8000-000000000101")
            .map_err(|error| error.to_string())
    }

    #[test]
    fn perspective_crud_keeps_historical_definitions_and_contexts_separate() -> Result<(), String> {
        let root = project_root();
        let principal = principal()?;
        create_project(&root, principal).map_err(|error| error.to_string())?;
        let (engine, _) =
            EngineHost::open_authorized(&root, principal).map_err(|error| error.to_string())?;
        let initial = match engine
            .perspectives(PerspectiveCommand::Snapshot {
                mode: crate::SchemaModeInput::Current,
            })
            .map_err(|error| error.to_string())?
        {
            PerspectiveResponse::Snapshot(snapshot) => snapshot,
            _ => return Err("Perspective snapshot returned an unexpected response".to_owned()),
        };
        if !initial.perspectives.is_empty() {
            return Err("a new project unexpectedly contains a Perspective".to_owned());
        }
        let base = initial.revision;
        let created = match engine
            .perspectives(PerspectiveCommand::Create {
                expected_base_revision: base,
                display_name: Some("Stadtwache".to_owned()),
                description: Some("Sicht der Stadtwache".to_owned()),
            })
            .map_err(|error| error.to_string())?
        {
            PerspectiveResponse::Published(receipt) => receipt,
            _ => return Err("Perspective creation returned an unexpected response".to_owned()),
        };
        let id = created
            .perspective_id
            .clone()
            .ok_or_else(|| "Perspective creation did not return its private identity".to_owned())?;
        let historical_create = match engine
            .perspectives(PerspectiveCommand::Snapshot {
                mode: crate::SchemaModeInput::Historical {
                    recorded_as_of: created.revision,
                },
            })
            .map_err(|error| error.to_string())?
        {
            PerspectiveResponse::Snapshot(snapshot) => snapshot,
            _ => {
                return Err(
                    "historical Perspective snapshot returned an unexpected response".to_owned(),
                );
            }
        };
        if historical_create
            .perspectives
            .first()
            .and_then(|view| view.display_name.as_deref())
            != Some("Stadtwache")
        {
            return Err("created Perspective was missing from its historical snapshot".to_owned());
        }

        let bound = engine
            .perspectives(PerspectiveCommand::ValidateContext {
                epistemic_mode: EpistemicModeInput::Knows,
                perspective_id: Some(id.clone()),
            })
            .map_err(|error| error.to_string())?;
        if !matches!(
            bound,
            PerspectiveResponse::ContextBound(PerspectiveContextView {
                epistemic_mode: EpistemicModeInput::Knows,
                perspective_label: Some(_),
            })
        ) {
            return Err("active Perspective could not bind a Knows context".to_owned());
        }
        if engine
            .perspectives(PerspectiveCommand::ValidateContext {
                epistemic_mode: EpistemicModeInput::WorldState,
                perspective_id: Some(id.clone()),
            })
            .is_ok()
            || engine
                .perspectives(PerspectiveCommand::ValidateContext {
                    epistemic_mode: EpistemicModeInput::Believes,
                    perspective_id: None,
                })
                .is_ok()
        {
            return Err("invalid WorldState/Perspective context pair was accepted".to_owned());
        }

        let updated = match engine
            .perspectives(PerspectiveCommand::Update {
                expected_base_revision: created.revision,
                perspective_id: id.clone(),
                display_name: Some("Wache".to_owned()),
                description: None,
            })
            .map_err(|error| error.to_string())?
        {
            PerspectiveResponse::Published(receipt) => receipt,
            _ => return Err("Perspective update returned an unexpected response".to_owned()),
        };
        let before_update = match engine
            .perspectives(PerspectiveCommand::Snapshot {
                mode: crate::SchemaModeInput::Explicit {
                    revision: created.revision,
                },
            })
            .map_err(|error| error.to_string())?
        {
            PerspectiveResponse::Snapshot(snapshot) => snapshot,
            _ => {
                return Err(
                    "explicit Perspective snapshot returned an unexpected response".to_owned(),
                );
            }
        };
        if before_update
            .perspectives
            .first()
            .and_then(|view| view.display_name.as_deref())
            != Some("Stadtwache")
        {
            return Err("Perspective update changed an earlier catalog revision".to_owned());
        }

        let _ = engine
            .perspectives(PerspectiveCommand::ValidateContext {
                epistemic_mode: EpistemicModeInput::Claims,
                perspective_id: Some(id.clone()),
            })
            .map_err(|error| error.to_string())?;
        let retired = match engine
            .perspectives(PerspectiveCommand::Retire {
                expected_base_revision: updated.revision,
                perspective_id: id.clone(),
            })
            .map_err(|error| error.to_string())?
        {
            PerspectiveResponse::Published(receipt) => receipt,
            _ => return Err("Perspective retirement returned an unexpected response".to_owned()),
        };
        if engine
            .perspectives(PerspectiveCommand::ValidateContext {
                epistemic_mode: EpistemicModeInput::Believes,
                perspective_id: Some(id),
            })
            .is_ok()
        {
            return Err("retired Perspective remained selectable".to_owned());
        }
        let at_retirement = match engine
            .perspectives(PerspectiveCommand::Snapshot {
                mode: crate::SchemaModeInput::Explicit {
                    revision: retired.revision,
                },
            })
            .map_err(|error| error.to_string())?
        {
            PerspectiveResponse::Snapshot(snapshot) => snapshot,
            _ => {
                return Err(
                    "retirement Perspective snapshot returned an unexpected response".to_owned(),
                );
            }
        };
        if at_retirement
            .perspectives
            .first()
            .and_then(|view| view.retired_revision)
            != Some(retired.revision)
        {
            return Err("Perspective retirement was not recorded in the catalog".to_owned());
        }

        drop(engine);
        let _ = std::fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn perspective_commands_are_closed_and_pair_validation_has_no_default() -> Result<(), String> {
        let malformed = serde_json::from_str::<PerspectiveCommand>(
            r#"{"command":"snapshot","mode":{"mode":"current"},"principal":"renderer"}"#,
        );
        assert!(malformed.is_err());

        assert!(
            super::selected_perspective(EpistemicModeInput::WorldState, None)
                .map_err(|error| error.to_string())?
                .is_none()
        );
        assert!(super::selected_perspective(EpistemicModeInput::Knows, None).is_err());
        assert!(
            super::selected_perspective(
                EpistemicModeInput::WorldState,
                Some("00000000-0000-7000-8000-000000000001".to_owned())
            )
            .is_err()
        );
        assert!(
            super::selected_perspective(
                EpistemicModeInput::Claims,
                Some("00000000-0000-7000-8000-000000000001".to_owned())
            )
            .is_ok()
        );
        Ok(())
    }
}
