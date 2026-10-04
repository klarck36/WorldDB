//! Authenticated HistorySpace and Layer commands for the desktop host.

use std::str::FromStr;

use serde::{Deserialize, Serialize};
use worlddb_core::{
    HistorySpaceId, LayerId, Lifecycle, Revision, SchemaMode, SchemaRevision, Symbol,
};
use worlddb_storage_file::FileProjectMetadataManager;

use crate::{EngineError, EngineHost, SchemaLifecycle, SchemaModeInput};

/// One closed HistorySpace or Layer action from a renderer window.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum BranchLayerCommand {
    /// Reads both project catalogs from one shared revision.
    Snapshot { mode: SchemaModeInput },
    /// Adds an immutable child with a caller-selected parent cutoff.
    CreateChild {
        expected_base_revision: u64,
        parent_history_space_id: String,
        base_revision: u64,
    },
    /// Adds an active overlay Layer at an explicit precedence rank.
    CreateLayer {
        expected_base_revision: u64,
        symbol: String,
        #[serde(default)]
        description: Option<String>,
        precedence_rank: i32,
    },
    /// Revises one Layer and may switch the unique base in the same commit.
    ReviseLayer {
        expected_base_revision: u64,
        layer_id: String,
        description: Option<String>,
        precedence_rank: i32,
        lifecycle: SchemaLifecycle,
        base_layer_id: String,
    },
}

/// Result of one branch and Layer catalog action.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BranchLayerResponse {
    Snapshot(BranchLayerSnapshotView),
    Published(BranchLayerPublicationView),
}

/// Catalogs pinned to one shared data/schema revision.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BranchLayerSnapshotView {
    pub revision: u64,
    pub branches: Vec<BranchView>,
    pub layers: Vec<LayerView>,
    pub base_layer_id: String,
}

/// One immutable HistorySpace ancestry definition.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BranchView {
    /// Private identity used by typed host commands and HTML select options.
    pub history_space_id: String,
    pub parent_history_space_id: Option<String>,
    pub base_revision: u64,
}

/// One Layer state in the selected historical or current catalog.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct LayerView {
    /// Private identity used by typed host commands and HTML select options.
    pub layer_id: String,
    pub symbol: String,
    pub description: Option<String>,
    pub precedence_rank: i32,
    pub lifecycle: String,
    pub is_base: bool,
}

/// Safe result of a single committed catalog update.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BranchLayerPublicationView {
    pub operation_id: String,
    pub revision: u64,
    pub branch_created: bool,
    pub layer_changed: bool,
}

impl EngineHost {
    /// Executes an authenticated HistorySpace and Layer catalog read or mutation.
    pub fn branch_layers(
        &self,
        command: BranchLayerCommand,
    ) -> Result<BranchLayerResponse, EngineError> {
        execute(self, command)
    }
}

pub(super) fn execute(
    engine: &EngineHost,
    command: BranchLayerCommand,
) -> Result<BranchLayerResponse, EngineError> {
    let _guard = engine.schema_management.lock().map_err(|_| {
        EngineError::BranchLayer("branch and Layer manager state is unavailable".to_owned())
    })?;
    let principal = engine.principal_id.ok_or_else(|| {
        EngineError::BranchLayer("an authenticated project session is required".to_owned())
    })?;
    let mut manager =
        FileProjectMetadataManager::open(engine._layout.clone(), &engine._writer_lock, principal)
            .map_err(|error| EngineError::BranchLayer(error.to_string()))?;

    match command {
        BranchLayerCommand::Snapshot { mode } => {
            let (mode, as_of) = selected_mode(mode)?;
            let snapshot = manager
                .snapshot_at(mode, as_of)
                .map_err(|error| EngineError::BranchLayer(error.to_string()))?;
            let base_layer_id = snapshot.layers().base_layer_id();
            let branches = snapshot
                .history_spaces()
                .definitions()
                .iter()
                .map(|branch| BranchView {
                    history_space_id: branch.history_space_id().to_string(),
                    parent_history_space_id: branch
                        .parent_history_space_id()
                        .map(|id| id.to_string()),
                    base_revision: branch.base_revision().value(),
                })
                .collect();
            let layers = snapshot
                .layers()
                .definitions()
                .iter()
                .map(|layer| LayerView {
                    layer_id: layer.layer_id().to_string(),
                    symbol: layer.symbol().as_str().to_owned(),
                    description: layer.description().map(str::to_owned),
                    precedence_rank: layer.precedence_rank(),
                    lifecycle: lifecycle_label(layer.lifecycle()).to_owned(),
                    is_base: layer.layer_id() == base_layer_id,
                })
                .collect();
            Ok(BranchLayerResponse::Snapshot(BranchLayerSnapshotView {
                revision: snapshot.revision().value(),
                branches,
                layers,
                base_layer_id: base_layer_id.to_string(),
            }))
        }
        BranchLayerCommand::CreateChild {
            expected_base_revision,
            parent_history_space_id,
            base_revision,
        } => {
            let parent = HistorySpaceId::from_str(&parent_history_space_id)
                .map_err(|_| EngineError::BranchLayer("invalid parent branch".to_owned()))?;
            let receipt =
                manager
                    .create_child(
                        revision_from_u64(expected_base_revision)?,
                        crate::requested_operation_id_or(
                            worlddb_core::storage_internal::generate_schema_management_operation_id,
                        )
                        .map_err(|_| {
                            EngineError::BranchLayer("operation identity is unavailable".to_owned())
                        })?,
                        worlddb_core::storage_internal::generate_project_bootstrap_id::<
                            HistorySpaceId,
                        >()
                        .map_err(|_| {
                            EngineError::BranchLayer("branch identity is unavailable".to_owned())
                        })?,
                        parent,
                        revision_from_u64(base_revision)?,
                    )
                    .map_err(|error| EngineError::BranchLayer(error.to_string()))?;
            Ok(BranchLayerResponse::Published(BranchLayerPublicationView {
                operation_id: receipt.operation_id().to_string(),
                revision: receipt.revision().value(),
                branch_created: true,
                layer_changed: false,
            }))
        }
        BranchLayerCommand::CreateLayer {
            expected_base_revision,
            symbol,
            description,
            precedence_rank,
        } => {
            let symbol = Symbol::new(symbol)
                .map_err(|_| EngineError::BranchLayer("invalid Layer symbol".to_owned()))?;
            let receipt = manager
                .create_layer(
                    revision_from_u64(expected_base_revision)?,
                    crate::requested_operation_id_or(
                        worlddb_core::storage_internal::generate_schema_management_operation_id,
                    )
                    .map_err(|_| {
                        EngineError::BranchLayer("operation identity is unavailable".to_owned())
                    })?,
                    worlddb_core::storage_internal::generate_project_bootstrap_id::<LayerId>()
                        .map_err(|_| {
                            EngineError::BranchLayer("Layer identity is unavailable".to_owned())
                        })?,
                    symbol,
                    description,
                    precedence_rank,
                )
                .map_err(|error| EngineError::BranchLayer(error.to_string()))?;
            Ok(BranchLayerResponse::Published(BranchLayerPublicationView {
                operation_id: receipt.operation_id().to_string(),
                revision: receipt.revision().value(),
                branch_created: false,
                layer_changed: true,
            }))
        }
        BranchLayerCommand::ReviseLayer {
            expected_base_revision,
            layer_id,
            description,
            precedence_rank,
            lifecycle,
            base_layer_id,
        } => {
            let layer_id = LayerId::from_str(&layer_id)
                .map_err(|_| EngineError::BranchLayer("invalid Layer identity".to_owned()))?;
            let base_layer_id = LayerId::from_str(&base_layer_id)
                .map_err(|_| EngineError::BranchLayer("invalid base Layer".to_owned()))?;
            let receipt = manager
                .revise_layer(
                    revision_from_u64(expected_base_revision)?,
                    crate::requested_operation_id_or(
                        worlddb_core::storage_internal::generate_schema_management_operation_id,
                    )
                    .map_err(|_| {
                        EngineError::BranchLayer("operation identity is unavailable".to_owned())
                    })?,
                    layer_id,
                    description,
                    precedence_rank,
                    Lifecycle::from(lifecycle),
                    base_layer_id,
                )
                .map_err(|error| EngineError::BranchLayer(error.to_string()))?;
            Ok(BranchLayerResponse::Published(BranchLayerPublicationView {
                operation_id: receipt.operation_id().to_string(),
                revision: receipt.revision().value(),
                branch_created: false,
                layer_changed: true,
            }))
        }
    }
}

fn selected_mode(input: SchemaModeInput) -> Result<(SchemaMode, Revision), EngineError> {
    match input {
        SchemaModeInput::Historical { recorded_as_of } => {
            Ok((SchemaMode::Historical, revision_from_u64(recorded_as_of)?))
        }
        SchemaModeInput::Current => Ok((SchemaMode::Current, Revision::GENESIS)),
        SchemaModeInput::Explicit { revision } => Ok((
            SchemaMode::Explicit(SchemaRevision::from_published_revision(revision_from_u64(
                revision,
            )?)),
            Revision::GENESIS,
        )),
    }
}

fn revision_from_u64(value: u64) -> Result<Revision, EngineError> {
    Revision::try_from(value)
        .map_err(|_| EngineError::BranchLayer("revision is outside the supported range".to_owned()))
}

fn lifecycle_label(lifecycle: Lifecycle) -> &'static str {
    match lifecycle {
        Lifecycle::Active => "active",
        Lifecycle::Deprecated => "deprecated",
        Lifecycle::Retired => "retired",
    }
}

#[cfg(test)]
mod tests {
    use super::{BranchLayerCommand, BranchLayerResponse};
    use crate::SchemaModeInput;

    #[test]
    fn branch_layer_wire_commands_are_typed_and_reject_extra_fields() -> Result<(), String> {
        let command = BranchLayerCommand::CreateChild {
            expected_base_revision: 4,
            parent_history_space_id: "00000000-0000-7000-8000-000000000001".to_owned(),
            base_revision: 2,
        };
        let encoded = serde_json::to_value(command).map_err(|error| error.to_string())?;
        if encoded
            != serde_json::json!({
                "command": "create_child",
                "expected_base_revision": 4,
                "parent_history_space_id": "00000000-0000-7000-8000-000000000001",
                "base_revision": 2
            })
        {
            return Err("branch create request wire shape changed".to_owned());
        }
        let invalid = serde_json::from_value::<BranchLayerCommand>(serde_json::json!({
            "command": "create_layer",
            "expected_base_revision": 1,
            "symbol": "overlay",
            "precedence_rank": 1,
            "renderer_supplied_layer_id": "00000000-0000-7000-8000-000000000002"
        }));
        if invalid.is_ok() {
            return Err("renderer supplied Layer identity was accepted".to_owned());
        }
        let mode = serde_json::to_value(SchemaModeInput::Explicit { revision: 9 })
            .map_err(|error| error.to_string())?;
        if mode != serde_json::json!({ "mode": "explicit", "revision": 9 }) {
            return Err("explicit branch/layer selector wire shape changed".to_owned());
        }
        let response = BranchLayerResponse::Published(crate::BranchLayerPublicationView {
            operation_id: "00000000-0000-7000-8000-000000000001".to_owned(),
            revision: 3,
            branch_created: false,
            layer_changed: true,
        });
        let encoded = serde_json::to_value(response).map_err(|error| error.to_string())?;
        if encoded
            != serde_json::json!({
                "kind": "published",
                "operation_id": "00000000-0000-7000-8000-000000000001",
                "revision": 3,
                "branch_created": false,
                "layer_changed": true
            })
        {
            return Err("branch/layer publication response wire shape changed".to_owned());
        }
        Ok(())
    }
}
