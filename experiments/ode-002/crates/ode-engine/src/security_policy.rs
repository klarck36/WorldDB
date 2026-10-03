//! Host-authenticated project policy administration.

use std::str::FromStr;

use serde::{Deserialize, Serialize};
use worlddb_core::{
    Capability, GrantEffect, PolicySubject, PrincipalId, PrincipalState, RoleDefinition,
    SecurityEpoch, SecurityPolicySnapshot,
};
use worlddb_storage_file::FileSecurityPolicyManager;

use crate::EngineHost;

/// One closed policy action from an authenticated project window.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum SecurityPolicyCommand {
    /// Reads the current policy view; requires SecurityPolicyRead.
    Snapshot,
    /// Changes a known principal's state as an append-only policy event.
    SetPrincipalState {
        expected_base_revision: u64,
        principal_id: String,
        state: PrincipalStateInput,
    },
    /// Registers an empty role bundle with a validated lowercase symbol.
    RegisterRole {
        expected_base_revision: u64,
        symbol: String,
    },
    /// Assigns a registered role to a registered principal at project scope.
    AssignRole {
        expected_base_revision: u64,
        principal_id: String,
        role_id: String,
    },
    /// Revokes one existing role assignment.
    RevokeRoleAssignment {
        expected_base_revision: u64,
        assignment_id: String,
    },
    /// Adds one explicit project-scoped capability allow or deny rule.
    AddCapabilityRule {
        expected_base_revision: u64,
        subject_kind: PolicySubjectKindInput,
        subject_id: String,
        capability: String,
        effect: GrantEffectInput,
    },
    /// Revokes one explicit capability rule.
    RevokeCapabilityRule {
        expected_base_revision: u64,
        rule_id: String,
    },
}

/// Principal lifecycle state accepted by the typed host command.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalStateInput {
    Active,
    Disabled,
    Retired,
}

/// Closed subject class for an explicit capability rule.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicySubjectKindInput {
    Principal,
    Role,
}

/// Effect of one explicit capability rule.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantEffectInput {
    Allow,
    Deny,
}

/// Safe result of one policy operation.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SecurityPolicyResponse {
    Snapshot(SecurityPolicySnapshotView),
    Published(SecurityPolicyPublicationView),
}

/// Current principal, role, assignment, and explicit-rule inventory.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SecurityPolicySnapshotView {
    pub revision: u64,
    pub security_epoch: u64,
    pub capabilities: Vec<String>,
    pub principals: Vec<PrincipalView>,
    pub roles: Vec<RoleView>,
    pub assignments: Vec<RoleAssignmentView>,
    pub explicit_rules: Vec<CapabilityRuleView>,
}

/// Registered host-authenticated principal and current lifecycle state.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PrincipalView {
    pub principal_id: String,
    pub state: String,
}

/// Role bundle as explicitly stored in the role definition.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RoleView {
    pub role_id: String,
    pub symbol: String,
    pub bundle: Vec<CapabilityBundleEntryView>,
}

/// One grant or deny in a role's versioned bundle.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CapabilityBundleEntryView {
    pub capability: String,
    pub effect: String,
}

/// Principal-to-role assignment and its scope.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RoleAssignmentView {
    pub assignment_id: String,
    pub principal_id: String,
    pub role_id: String,
    pub scope: String,
}

/// One typed rule outside a role's initial policy bundle.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CapabilityRuleView {
    pub rule_id: String,
    pub subject_kind: String,
    pub subject_id: String,
    pub capability: String,
    pub effect: String,
    pub scope: String,
}

/// Safe outcome for one committed policy change.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SecurityPolicyPublicationView {
    pub operation_id: String,
    pub revision: u64,
    pub security_epoch: u64,
}

impl EngineHost {
    /// Executes a policy action using only the principal bound by the host session.
    pub fn security_policy(
        &self,
        command: SecurityPolicyCommand,
    ) -> Result<SecurityPolicyResponse, crate::EngineError> {
        let _guard = self.schema_management.lock().map_err(|_| {
            crate::EngineError::SecurityPolicy(
                "security policy manager state is unavailable".to_owned(),
            )
        })?;
        let principal = self.principal_id.ok_or_else(|| {
            crate::EngineError::SecurityPolicy(
                "an authenticated project session is required".to_owned(),
            )
        })?;
        let mut manager =
            FileSecurityPolicyManager::open(self._layout.clone(), &self._writer_lock, principal)
                .map_err(|error| crate::EngineError::SecurityPolicy(error.to_string()))?;

        match command {
            SecurityPolicyCommand::Snapshot => {
                let policy = manager
                    .policy_snapshot()
                    .map_err(|error| crate::EngineError::SecurityPolicy(error.to_string()))?;
                let epoch = manager
                    .security_epoch()
                    .map_err(|error| crate::EngineError::SecurityPolicy(error.to_string()))?;
                Ok(SecurityPolicyResponse::Snapshot(policy_view(
                    manager.revision(),
                    epoch,
                    policy,
                )))
            }
            SecurityPolicyCommand::SetPrincipalState {
                expected_base_revision,
                principal_id,
                state,
            } => {
                let principal_id = parse_id::<PrincipalId>(&principal_id)?;
                let receipt = manager
                    .set_principal_state(
                        revision_from_u64(expected_base_revision)?,
                        operation_id()?,
                        principal_id,
                        principal_state(state),
                    )
                    .map_err(|error| crate::EngineError::SecurityPolicy(error.to_string()))?;
                Ok(SecurityPolicyResponse::Published(publication_view(
                    receipt.operation_id(),
                    receipt.revision().value(),
                    receipt.epoch(),
                )))
            }
            SecurityPolicyCommand::RegisterRole {
                expected_base_revision,
                symbol,
            } => {
                let symbol = worlddb_core::Symbol::new(symbol).map_err(|_| {
                    crate::EngineError::SecurityPolicy("invalid role symbol".to_owned())
                })?;
                let role_id = worlddb_core::storage_internal::generate_project_bootstrap_id()
                    .map_err(|_| {
                        crate::EngineError::SecurityPolicy(
                            "role identity is unavailable".to_owned(),
                        )
                    })?;
                let receipt = manager
                    .register_role(
                        revision_from_u64(expected_base_revision)?,
                        operation_id()?,
                        role_id,
                        symbol,
                    )
                    .map_err(|error| crate::EngineError::SecurityPolicy(error.to_string()))?;
                Ok(SecurityPolicyResponse::Published(publication_view(
                    receipt.operation_id(),
                    receipt.revision().value(),
                    receipt.epoch(),
                )))
            }
            SecurityPolicyCommand::AssignRole {
                expected_base_revision,
                principal_id,
                role_id,
            } => {
                let principal_id = parse_id::<PrincipalId>(&principal_id)?;
                let role_id = parse_id::<worlddb_core::RoleId>(&role_id)?;
                let assignment_id = worlddb_core::storage_internal::generate_project_bootstrap_id()
                    .map_err(|_| {
                        crate::EngineError::SecurityPolicy(
                            "assignment identity is unavailable".to_owned(),
                        )
                    })?;
                let receipt = manager
                    .assign_role(
                        revision_from_u64(expected_base_revision)?,
                        operation_id()?,
                        assignment_id,
                        principal_id,
                        role_id,
                    )
                    .map_err(|error| crate::EngineError::SecurityPolicy(error.to_string()))?;
                Ok(SecurityPolicyResponse::Published(publication_view(
                    receipt.operation_id(),
                    receipt.revision().value(),
                    receipt.epoch(),
                )))
            }
            SecurityPolicyCommand::RevokeRoleAssignment {
                expected_base_revision,
                assignment_id,
            } => {
                let assignment_id = parse_id::<worlddb_core::RoleAssignmentId>(&assignment_id)?;
                let receipt = manager
                    .revoke_role_assignment(
                        revision_from_u64(expected_base_revision)?,
                        operation_id()?,
                        assignment_id,
                    )
                    .map_err(|error| crate::EngineError::SecurityPolicy(error.to_string()))?;
                Ok(SecurityPolicyResponse::Published(publication_view(
                    receipt.operation_id(),
                    receipt.revision().value(),
                    receipt.epoch(),
                )))
            }
            SecurityPolicyCommand::AddCapabilityRule {
                expected_base_revision,
                subject_kind,
                subject_id,
                capability,
                effect,
            } => {
                let subject = match subject_kind {
                    PolicySubjectKindInput::Principal => {
                        PolicySubject::Principal(parse_id::<PrincipalId>(&subject_id)?)
                    }
                    PolicySubjectKindInput::Role => {
                        PolicySubject::Role(parse_id::<worlddb_core::RoleId>(&subject_id)?)
                    }
                };
                let capability = parse_capability(&capability)?;
                let effect = grant_effect(effect);
                let rule_id = worlddb_core::storage_internal::generate_project_bootstrap_id()
                    .map_err(|_| {
                        crate::EngineError::SecurityPolicy(
                            "rule identity is unavailable".to_owned(),
                        )
                    })?;
                let receipt = manager
                    .add_capability_rule(
                        revision_from_u64(expected_base_revision)?,
                        operation_id()?,
                        rule_id,
                        subject,
                        capability,
                        effect,
                    )
                    .map_err(|error| crate::EngineError::SecurityPolicy(error.to_string()))?;
                Ok(SecurityPolicyResponse::Published(publication_view(
                    receipt.operation_id(),
                    receipt.revision().value(),
                    receipt.epoch(),
                )))
            }
            SecurityPolicyCommand::RevokeCapabilityRule {
                expected_base_revision,
                rule_id,
            } => {
                let rule_id = parse_id::<worlddb_core::PolicyRuleId>(&rule_id)?;
                let receipt = manager
                    .revoke_capability_rule(
                        revision_from_u64(expected_base_revision)?,
                        operation_id()?,
                        rule_id,
                    )
                    .map_err(|error| crate::EngineError::SecurityPolicy(error.to_string()))?;
                Ok(SecurityPolicyResponse::Published(publication_view(
                    receipt.operation_id(),
                    receipt.revision().value(),
                    receipt.epoch(),
                )))
            }
        }
    }
}

fn policy_view(
    revision: worlddb_core::Revision,
    epoch: SecurityEpoch,
    policy: &SecurityPolicySnapshot,
) -> SecurityPolicySnapshotView {
    SecurityPolicySnapshotView {
        revision: revision.value(),
        security_epoch: epoch.value(),
        capabilities: Capability::ALL.into_iter().map(capability_symbol).collect(),
        principals: policy
            .principals()
            .iter()
            .map(|principal| PrincipalView {
                principal_id: principal.id().to_string(),
                state: principal_state_symbol(principal.state()).to_owned(),
            })
            .collect(),
        roles: policy.roles().iter().map(role_view).collect(),
        assignments: policy
            .assignments()
            .iter()
            .map(|assignment| RoleAssignmentView {
                assignment_id: assignment.id().to_string(),
                principal_id: assignment.principal().to_string(),
                role_id: assignment.role().to_string(),
                scope: scope_symbol(assignment.scope()),
            })
            .collect(),
        explicit_rules: policy
            .rules()
            .iter()
            .map(|rule| {
                let (subject_kind, subject_id) = match rule.subject() {
                    PolicySubject::Principal(id) => ("principal", id.to_string()),
                    PolicySubject::Role(id) => ("role", id.to_string()),
                };
                CapabilityRuleView {
                    rule_id: rule.id().to_string(),
                    subject_kind: subject_kind.to_owned(),
                    subject_id,
                    capability: capability_symbol(rule.grant().capability()),
                    effect: grant_effect_symbol(rule.grant().effect()).to_owned(),
                    scope: scope_symbol(rule.scope()),
                }
            })
            .collect(),
    }
}

fn role_view(role: &RoleDefinition) -> RoleView {
    RoleView {
        role_id: role.id().to_string(),
        symbol: role.symbol().to_owned(),
        bundle: role
            .bundle()
            .iter()
            .map(|grant| CapabilityBundleEntryView {
                capability: capability_symbol(grant.capability()),
                effect: grant_effect_symbol(grant.effect()).to_owned(),
            })
            .collect(),
    }
}

fn scope_symbol(scope: worlddb_core::PolicyScope) -> String {
    if scope.history_space().is_none()
        && scope.layer().is_none()
        && scope.record().is_none()
        && scope.field().is_none()
        && scope.relationship().is_none()
    {
        "project".to_owned()
    } else {
        "restricted".to_owned()
    }
}

fn capability_symbol(capability: Capability) -> String {
    let debug = format!("{capability:?}");
    let mut symbol = String::with_capacity(debug.len() + 8);
    for (index, character) in debug.chars().enumerate() {
        if character.is_ascii_uppercase() {
            if index != 0 {
                symbol.push('_');
            }
            symbol.push(character.to_ascii_lowercase());
        } else {
            symbol.push(character);
        }
    }
    symbol
}

fn parse_capability(value: &str) -> Result<Capability, crate::EngineError> {
    Capability::ALL
        .into_iter()
        .find(|capability| capability_symbol(*capability) == value)
        .ok_or_else(|| crate::EngineError::SecurityPolicy("unknown capability".to_owned()))
}

fn principal_state(input: PrincipalStateInput) -> PrincipalState {
    match input {
        PrincipalStateInput::Active => PrincipalState::Active,
        PrincipalStateInput::Disabled => PrincipalState::Disabled,
        PrincipalStateInput::Retired => PrincipalState::Retired,
    }
}

fn principal_state_symbol(state: PrincipalState) -> &'static str {
    match state {
        PrincipalState::Active => "active",
        PrincipalState::Disabled => "disabled",
        PrincipalState::Retired => "retired",
    }
}

fn grant_effect(input: GrantEffectInput) -> GrantEffect {
    match input {
        GrantEffectInput::Allow => GrantEffect::Allow,
        GrantEffectInput::Deny => GrantEffect::Deny,
    }
}

fn grant_effect_symbol(effect: GrantEffect) -> &'static str {
    match effect {
        GrantEffect::Allow => "allow",
        GrantEffect::Deny => "deny",
    }
}

fn publication_view(
    operation_id: worlddb_core::OperationId,
    revision: u64,
    epoch: SecurityEpoch,
) -> SecurityPolicyPublicationView {
    SecurityPolicyPublicationView {
        operation_id: operation_id.to_string(),
        revision,
        security_epoch: epoch.value(),
    }
}

fn operation_id() -> Result<worlddb_core::OperationId, crate::EngineError> {
    worlddb_core::storage_internal::generate_security_policy_operation_id().map_err(|_| {
        crate::EngineError::SecurityPolicy("operation identity is unavailable".to_owned())
    })
}

fn parse_id<T: worlddb_core::DomainId + FromStr>(value: &str) -> Result<T, crate::EngineError> {
    T::from_str(value)
        .map_err(|_| crate::EngineError::SecurityPolicy("invalid policy identity".to_owned()))
}

fn revision_from_u64(value: u64) -> Result<worlddb_core::Revision, crate::EngineError> {
    worlddb_core::Revision::try_from(value).map_err(|_| {
        crate::EngineError::SecurityPolicy("revision is outside the supported range".to_owned())
    })
}
