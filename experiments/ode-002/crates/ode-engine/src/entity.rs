//! Typed Entity catalog commands for the authenticated desktop host.

use std::str::FromStr;

use serde::{Deserialize, Serialize};
use worlddb_core::{
    EntityCatalogSnapshot, EntityId, EntityTypeId, Lifecycle, Revision, SchemaDefinition,
    SchemaMode, SchemaRevision,
};
use worlddb_storage_file::FileEntityManager;

use crate::{EngineError, EngineHost};

/// Entity catalog operation sent by a host window.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum EntityCommand {
    /// Reads the requested shared-revision Entity and EntityType view.
    Snapshot { mode: EntityModeInput },
    /// Creates an Entity with a host-generated ID and permanent EntityType.
    Create {
        expected_base_revision: u64,
        entity_type_id: String,
        #[serde(default)]
        accept_deprecated_type: bool,
    },
    /// Retires one existing Entity. Retirement is irreversible and append-only.
    Retire {
        expected_base_revision: u64,
        entity_id: String,
    },
}

/// Current, historical, or explicitly selected shared revision.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum EntityModeInput {
    /// Entity catalog state visible at the query's `RecordedAsOf` revision.
    Historical { recorded_as_of: u64 },
    /// Latest Entity catalog state at the live database head.
    Current,
    /// Exact previously committed shared revision.
    Explicit { revision: u64 },
}

/// Result of one typed Entity catalog command.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EntityResponse {
    /// Historical/current catalog plus type choices at the same revision.
    Snapshot(EntitySnapshotView),
    /// One committed creation or retirement.
    Published(EntityPublicationView),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EntitySnapshotView {
    pub revision: u64,
    pub entities: Vec<EntityView>,
    pub entity_types: Vec<EntityTypeView>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EntityView {
    /// Private IPC identity used by the host to address a selected Entity.
    /// The desktop does not render this implementation identity to the user.
    pub entity_id: String,
    pub entity_type_id: String,
    pub entity_type_symbol: String,
    pub entity_type_lifecycle: String,
    pub created_revision: u64,
    pub retired_revision: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EntityTypeView {
    /// Private IPC identity used to create a typed Entity.
    pub entity_type_id: String,
    pub symbol: String,
    pub description: Option<String>,
    pub lifecycle: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EntityPublicationView {
    pub operation_id: String,
    pub revision: u64,
    pub entity_id: Option<String>,
    pub retirement_id: Option<String>,
    pub warning: Option<EntityWarningView>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EntityWarningView {
    pub code: String,
    pub message: String,
}

impl EngineHost {
    /// Executes an authenticated Entity catalog read or mutation.
    pub fn entities(&self, command: EntityCommand) -> Result<EntityResponse, EngineError> {
        let _guard = self.schema_management.lock().map_err(|_| {
            EngineError::Entity("Entity catalog manager state is unavailable".to_owned())
        })?;
        let principal = self.principal_id.ok_or_else(|| {
            EngineError::Entity("an authenticated project session is required".to_owned())
        })?;
        let mut manager =
            FileEntityManager::open(self._layout.clone(), &self._writer_lock, principal)
                .map_err(|error| EngineError::Entity(error.to_string()))?;

        match command {
            EntityCommand::Snapshot { mode } => {
                let (mode, recorded_as_of) = entity_mode(mode)?;
                let (catalog, schema) = manager
                    .snapshot_at(mode, recorded_as_of)
                    .map_err(|error| EngineError::Entity(error.to_string()))?;
                Ok(EntityResponse::Snapshot(entity_snapshot_view(
                    &catalog, &schema,
                )?))
            }
            EntityCommand::Create {
                expected_base_revision,
                entity_type_id,
                accept_deprecated_type,
            } => {
                let expected_base = revision_from_u64(expected_base_revision)?;
                let type_id = EntityTypeId::from_str(&entity_type_id)
                    .map_err(|_| EngineError::Entity("invalid EntityType identity".to_owned()))?;
                let schema = manager
                    .schema_at(SchemaMode::Current, Revision::GENESIS)
                    .map_err(|error| EngineError::Entity(error.to_string()))?;
                let type_definition = schema
                    .definitions()
                    .iter()
                    .find_map(|definition| match definition {
                        SchemaDefinition::EntityType(value)
                            if value.entity_type_id() == type_id =>
                        {
                            Some(value)
                        }
                        _ => None,
                    })
                    .ok_or_else(|| {
                        EngineError::Entity("EntityType is unknown or not visible".to_owned())
                    })?;
                let entity_id =
                    worlddb_core::storage_internal::generate_entity_management_entity_id()
                        .map_err(|error| EngineError::Entity(error.to_string()))?;
                let operation_id =
                    worlddb_core::storage_internal::generate_entity_management_operation_id()
                        .map_err(|error| EngineError::Entity(error.to_string()))?;
                let receipt = manager
                    .create(
                        expected_base,
                        operation_id,
                        entity_id,
                        type_id,
                        accept_deprecated_type,
                    )
                    .map_err(|error| EngineError::Entity(error.to_string()))?;
                let warning = receipt.used_deprecated_type().then(|| EntityWarningView {
                    code: "deprecated_entity_type".to_owned(),
                    message: format!(
                        "Der EntityType „{}“ ist stillgelegt. Diese neue Entität wurde nach deiner ausdrücklichen Freigabe angelegt.",
                        type_definition.symbol().as_str()
                    ),
                });
                Ok(EntityResponse::Published(EntityPublicationView {
                    operation_id: receipt.operation_id().to_string(),
                    revision: receipt.revision().value(),
                    entity_id: receipt
                        .entity()
                        .map(|entity| entity.entity_id().to_string()),
                    retirement_id: None,
                    warning,
                }))
            }
            EntityCommand::Retire {
                expected_base_revision,
                entity_id,
            } => {
                let expected_base = revision_from_u64(expected_base_revision)?;
                let entity_id = EntityId::from_str(&entity_id)
                    .map_err(|_| EngineError::Entity("invalid Entity identity".to_owned()))?;
                let retirement_id =
                    worlddb_core::storage_internal::generate_entity_management_retirement_id()
                        .map_err(|error| EngineError::Entity(error.to_string()))?;
                let operation_id =
                    worlddb_core::storage_internal::generate_entity_management_operation_id()
                        .map_err(|error| EngineError::Entity(error.to_string()))?;
                let receipt = manager
                    .retire(expected_base, operation_id, retirement_id, entity_id)
                    .map_err(|error| EngineError::Entity(error.to_string()))?;
                Ok(EntityResponse::Published(EntityPublicationView {
                    operation_id: receipt.operation_id().to_string(),
                    revision: receipt.revision().value(),
                    entity_id: None,
                    retirement_id: receipt
                        .retirement()
                        .map(|retirement| retirement.entity_retirement_id().to_string()),
                    warning: None,
                }))
            }
        }
    }
}

fn entity_mode(input: EntityModeInput) -> Result<(SchemaMode, Revision), EngineError> {
    match input {
        EntityModeInput::Historical { recorded_as_of } => {
            Ok((SchemaMode::Historical, revision_from_u64(recorded_as_of)?))
        }
        EntityModeInput::Current => Ok((SchemaMode::Current, Revision::GENESIS)),
        EntityModeInput::Explicit { revision } => Ok((
            SchemaMode::Explicit(SchemaRevision::from_published_revision(revision_from_u64(
                revision,
            )?)),
            Revision::GENESIS,
        )),
    }
}

fn revision_from_u64(value: u64) -> Result<Revision, EngineError> {
    Revision::try_from(value)
        .map_err(|_| EngineError::Entity("revision is outside the supported range".to_owned()))
}

fn entity_snapshot_view(
    catalog: &EntityCatalogSnapshot,
    schema: &worlddb_core::SchemaSnapshot,
) -> Result<EntitySnapshotView, EngineError> {
    let entity_types = schema
        .definitions()
        .iter()
        .filter_map(|definition| match definition {
            SchemaDefinition::EntityType(value) => Some(EntityTypeView {
                entity_type_id: value.entity_type_id().to_string(),
                symbol: value.symbol().as_str().to_owned(),
                description: value.description().map(str::to_owned),
                lifecycle: lifecycle_name(value.lifecycle()).to_owned(),
            }),
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut entities = catalog.entities().to_vec();
    entities.sort_by_key(|entity| (entity.created_revision(), entity.entity_id()));
    let mut views = Vec::new();
    views
        .try_reserve_exact(entities.len())
        .map_err(|_| EngineError::Entity("Entity view allocation failed".to_owned()))?;
    for entity in entities {
        let entity_type = schema
            .definitions()
            .iter()
            .find_map(|definition| match definition {
                SchemaDefinition::EntityType(value)
                    if value.entity_type_id() == entity.entity_type_id() =>
                {
                    Some(value)
                }
                _ => None,
            })
            .ok_or_else(|| {
                EngineError::Entity("EntityType is missing at the selected revision".to_owned())
            })?;
        let retired_revision = catalog
            .retirements()
            .iter()
            .find(|retirement| retirement.entity_id() == entity.entity_id())
            .map(|retirement| retirement.created_revision().value());
        views.push(EntityView {
            entity_id: entity.entity_id().to_string(),
            entity_type_id: entity.entity_type_id().to_string(),
            entity_type_symbol: entity_type.symbol().as_str().to_owned(),
            entity_type_lifecycle: lifecycle_name(entity_type.lifecycle()).to_owned(),
            created_revision: entity.created_revision().value(),
            retired_revision,
        });
    }
    Ok(EntitySnapshotView {
        revision: catalog.revision().value(),
        entities: views,
        entity_types,
    })
}

fn lifecycle_name(value: Lifecycle) -> &'static str {
    match value {
        Lifecycle::Active => "active",
        Lifecycle::Deprecated => "deprecated",
        Lifecycle::Retired => "retired",
    }
}

#[cfg(test)]
mod wire_tests {
    use super::{
        EntityCommand, EntityModeInput, EntityResponse, EntitySnapshotView, EntityTypeView,
    };

    #[test]
    fn entity_commands_keep_a_closed_stable_json_shape() -> Result<(), String> {
        let command = EntityCommand::Create {
            expected_base_revision: 7,
            entity_type_id: "type-id".to_owned(),
            accept_deprecated_type: true,
        };
        let encoded = serde_json::to_value(command).map_err(|error| error.to_string())?;
        let expected = serde_json::json!({
            "command": "create",
            "expected_base_revision": 7,
            "entity_type_id": "type-id",
            "accept_deprecated_type": true,
        });
        if encoded != expected {
            return Err("Entity Create command wire shape changed".to_owned());
        }
        if serde_json::from_value::<EntityCommand>(serde_json::json!({
            "command": "snapshot",
            "mode": { "mode": "current" },
            "renderer_selected_identity": "not-accepted",
        }))
        .is_ok()
        {
            return Err("Entity command accepted an unknown renderer field".to_owned());
        }

        let mode = serde_json::to_value(EntityModeInput::Historical { recorded_as_of: 5 })
            .map_err(|error| error.to_string())?;
        if mode != serde_json::json!({ "mode": "historical", "recorded_as_of": 5 }) {
            return Err("Historical Entity selector wire shape changed".to_owned());
        }
        Ok(())
    }

    #[test]
    fn entity_snapshot_response_exposes_typed_views_and_private_identity_fields()
    -> Result<(), String> {
        let response = EntityResponse::Snapshot(EntitySnapshotView {
            revision: 4,
            entities: Vec::new(),
            entity_types: vec![EntityTypeView {
                entity_type_id: "private-type-id".to_owned(),
                symbol: "person".to_owned(),
                description: None,
                lifecycle: "active".to_owned(),
            }],
        });
        let encoded = serde_json::to_value(response).map_err(|error| error.to_string())?;
        let expected = serde_json::json!({
            "kind": "snapshot",
            "revision": 4,
            "entities": [],
            "entity_types": [{
                "entity_type_id": "private-type-id",
                "symbol": "person",
                "description": null,
                "lifecycle": "active",
            }],
        });
        if encoded != expected {
            return Err("Entity snapshot response wire shape changed".to_owned());
        }
        Ok(())
    }
}
