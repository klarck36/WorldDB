const role = new URLSearchParams(location.search).get("role") ?? "unknown";
const invoke = window.__TAURI__?.core?.invoke;
const windowStatus = document.querySelector("#window-role");
const projectStatus = document.querySelector("#project-status");
const projectDetails = document.querySelector("#project-details");
const operationStatus = document.querySelector("#operation-status");
const projectName = document.querySelector("#project-name");
const createButton = document.querySelector("#create-project");
const openButton = document.querySelector("#open-project");
const closeButton = document.querySelector("#close-project");

const schemaPanel = document.querySelector("#schema-panel");
const schemaStatus = document.querySelector("#schema-status");
const schemaDefinitions = document.querySelector("#schema-definitions");
const schemaLifecyclePending = document.querySelector("#schema-lifecycle-pending");
const schemaLifecyclePublish = document.querySelector("#schema-lifecycle-publish");
const schemaEditor = document.querySelector("#schema-editor");
const schemaViewMode = document.querySelector("#schema-view-mode");
const schemaViewRevision = document.querySelector("#schema-view-revision");
const schemaViewRevisionWrap = document.querySelector("#schema-view-revision-wrap");
const schemaViewRevisionLabel = document.querySelector("#schema-view-revision-label");
const schemaRefreshButton = document.querySelector("#schema-refresh");
const schemaCreateButton = document.querySelector("#schema-create");
const schemaFamily = document.querySelector("#schema-family");
const schemaSymbol = document.querySelector("#schema-symbol");
const schemaEntityTypeFields = document.querySelector("#entity-type-fields");
const schemaDescription = document.querySelector("#schema-description");
const schemaPredicateFields = document.querySelector("#predicate-fields");
const schemaEventKindFields = document.querySelector("#event-kind-fields");
const schemaSubjectType = document.querySelector("#schema-subject-type");
const schemaValueKind = document.querySelector("#schema-value-kind");
const schemaObjectTypeWrap = document.querySelector("#schema-object-type-wrap");
const schemaObjectType = document.querySelector("#schema-object-type");
const schemaCardinality = document.querySelector("#schema-cardinality");
const schemaResolution = document.querySelector("#schema-resolution");
const schemaConstraintKind = document.querySelector("#schema-constraint-kind");
const schemaConstraintFields = document.querySelector("#schema-constraint-fields");
const schemaDecimalMetadata = document.querySelector("#schema-decimal-metadata");
const decimalMetadataInputs = {
  display_precision: document.querySelector("#schema-display-precision"),
  measurement_precision: document.querySelector("#schema-measurement-precision"),
  currency_scale: document.querySelector("#schema-currency-scale"),
};
const eventTimeForm = document.querySelector("#event-time-form");
const eventMaxSpanEnabled = document.querySelector("#event-max-span-enabled");
const eventMaxSpan = document.querySelector("#event-max-span");
const eventRoleSymbol = document.querySelector("#event-role-symbol");
const eventRoleType = document.querySelector("#event-role-type");
const eventRoleMin = document.querySelector("#event-role-min");
const eventRoleMax = document.querySelector("#event-role-max");
const addEventRoleButton = document.querySelector("#add-event-role");
const eventRoleList = document.querySelector("#event-role-list");
const eventAttributeSymbol = document.querySelector("#event-attribute-symbol");
const eventAttributeKind = document.querySelector("#event-attribute-kind");
const eventAttributeTypeWrap = document.querySelector("#event-attribute-type-wrap");
const eventAttributeType = document.querySelector("#event-attribute-type");
const eventAttributeRequired = document.querySelector("#event-attribute-required");
const eventAttributeConstraintKind = document.querySelector("#event-attribute-constraint-kind");
const eventAttributeConstraintFields = document.querySelector("#event-attribute-constraint-fields");
const eventAttributeDecimalMetadata = document.querySelector("#event-attribute-decimal-metadata");
const addEventAttributeButton = document.querySelector("#add-event-attribute");
const eventAttributeList = document.querySelector("#event-attribute-list");
const eventAttributeDecimalMetadataInputs = {
  display_precision: document.querySelector("#event-attribute-display-precision"),
  measurement_precision: document.querySelector("#event-attribute-measurement-precision"),
  currency_scale: document.querySelector("#event-attribute-currency-scale"),
};
const entityPanel = document.querySelector("#entity-panel");
const entityStatus = document.querySelector("#entity-status");
const entityList = document.querySelector("#entity-list");
const entityEditor = document.querySelector("#entity-editor");
const entityViewMode = document.querySelector("#entity-view-mode");
const entityViewRevision = document.querySelector("#entity-view-revision");
const entityViewRevisionWrap = document.querySelector("#entity-view-revision-wrap");
const entityViewRevisionLabel = document.querySelector("#entity-view-revision-label");
const entityRefreshButton = document.querySelector("#entity-refresh");
const entityTypeSelect = document.querySelector("#entity-type-select");
const entityDeprecatedOptIn = document.querySelector("#entity-deprecated-opt-in");
const entityAcceptDeprecated = document.querySelector("#entity-accept-deprecated");
const entityDeprecatedWarning = document.querySelector("#entity-deprecated-warning");
const entityCreateButton = document.querySelector("#entity-create");
const securityPolicyPanel = document.querySelector("#security-policy-panel");
const securityPolicyStatus = document.querySelector("#security-policy-status");
const securityPolicyPrincipals = document.querySelector("#security-policy-principals");
const securityPolicyRoles = document.querySelector("#security-policy-roles");
const securityPolicyAssignments = document.querySelector("#security-policy-assignments");
const securityPolicyRules = document.querySelector("#security-policy-rules");
const securityPolicyRefresh = document.querySelector("#security-policy-refresh");
const policyPrincipalSelect = document.querySelector("#policy-principal-select");
const policyPrincipalState = document.querySelector("#policy-principal-state");
const policyPrincipalSave = document.querySelector("#policy-principal-save");
const policyAssignmentPrincipal = document.querySelector("#policy-assignment-principal");
const policyAssignmentRole = document.querySelector("#policy-assignment-role");
const policyRoleAssign = document.querySelector("#policy-role-assign");
const policyNewRoleSymbol = document.querySelector("#policy-new-role-symbol");
const policyRoleCreate = document.querySelector("#policy-role-create");
const policyRuleSubject = document.querySelector("#policy-rule-subject");
const policyRuleCapability = document.querySelector("#policy-rule-capability");
const policyRuleEffect = document.querySelector("#policy-rule-effect");
const policyRuleAdd = document.querySelector("#policy-rule-add");
const perspectivePanel = document.querySelector("#perspective-panel");
const perspectiveViewMode = document.querySelector("#perspective-view-mode");
const perspectiveViewRevision = document.querySelector("#perspective-view-revision");
const perspectiveViewRevisionWrap = document.querySelector("#perspective-view-revision-wrap");
const perspectiveViewRevisionLabel = document.querySelector("#perspective-view-revision-label");
const perspectiveRefreshButton = document.querySelector("#perspective-refresh");
const perspectiveStatus = document.querySelector("#perspective-status");
const perspectiveList = document.querySelector("#perspective-list");
const perspectiveSelect = document.querySelector("#perspective-select");
const perspectiveName = document.querySelector("#perspective-name");
const perspectiveDescription = document.querySelector("#perspective-description");
const perspectiveCreateButton = document.querySelector("#perspective-create");
const perspectiveUpdateButton = document.querySelector("#perspective-update");
const perspectiveRetireButton = document.querySelector("#perspective-retire");
const inputContextMode = document.querySelector("#input-context-mode");
const inputContextPerspective = document.querySelector("#input-context-perspective");
const inputContextValidate = document.querySelector("#input-context-validate");
const inputContextStatus = document.querySelector("#input-context-status");
const queryContextMode = document.querySelector("#query-context-mode");
const queryContextPerspective = document.querySelector("#query-context-perspective");
const queryContextValidate = document.querySelector("#query-context-validate");
const queryContextStatus = document.querySelector("#query-context-status");
const branchLayerPanel = document.querySelector("#branch-layer-panel");
const branchLayerStatus = document.querySelector("#branch-layer-status");
const branchTree = document.querySelector("#branch-tree");
const layerList = document.querySelector("#layer-list");
const branchLayerViewMode = document.querySelector("#branch-layer-view-mode");
const branchLayerViewRevision = document.querySelector("#branch-layer-view-revision");
const branchLayerViewRevisionWrap = document.querySelector("#branch-layer-view-revision-wrap");
const branchLayerViewRevisionLabel = document.querySelector("#branch-layer-view-revision-label");
const branchLayerRefreshButton = document.querySelector("#branch-layer-refresh");
const branchCreateEditor = document.querySelector("#branch-create-editor");
const branchParentSelect = document.querySelector("#branch-parent-select");
const branchCutoff = document.querySelector("#branch-cutoff");
const branchCreateButton = document.querySelector("#branch-create");
const layerCreateEditor = document.querySelector("#layer-create-editor");
const layerSymbol = document.querySelector("#layer-symbol");
const layerRank = document.querySelector("#layer-rank");
const layerDescription = document.querySelector("#layer-description");
const layerCreateButton = document.querySelector("#layer-create");
const layerEditEditor = document.querySelector("#layer-edit-editor");
const layerEditSelect = document.querySelector("#layer-edit-select");
const layerBaseSelect = document.querySelector("#layer-base-select");
const layerEditRank = document.querySelector("#layer-edit-rank");
const layerLifecycle = document.querySelector("#layer-lifecycle");
const layerEditDescription = document.querySelector("#layer-edit-description");
const layerUpdateButton = document.querySelector("#layer-update");
const transferPanel = document.querySelector("#history-space-transfer-panel");
const transferSource = document.querySelector("#transfer-source");
const transferTarget = document.querySelector("#transfer-target");
const transferRevision = document.querySelector("#transfer-revision");
const transferExternalPolicy = document.querySelector("#transfer-external-policy");
const transferLoadButton = document.querySelector("#transfer-load");
const transferStatus = document.querySelector("#transfer-status");
const transferPicker = document.querySelector("#transfer-content-picker");
const transferContentList = document.querySelector("#transfer-content-list");
const transferRelationList = document.querySelector("#transfer-relation-list");
const transferPreviewButton = document.querySelector("#transfer-preview");
const transferPreviewPanel = document.querySelector("#transfer-preview-panel");
const transferPreviewSummary = document.querySelector("#transfer-preview-summary");
const transferAcknowledge = document.querySelector("#transfer-acknowledge");
const transferCommitButton = document.querySelector("#transfer-commit");

const userMessages = {
  project_already_exists: "An diesem Ort gibt es bereits ein Projekt.",
  project_already_open: "Es ist bereits ein anderes Projekt geöffnet. Schließe es zuerst.",
  project_unavailable: "Dieses Konto hat keinen Zugriff auf das Projekt.",
  invalid_project: "Der ausgewählte Ordner enthält kein gültiges WorldDB-Projekt.",
  recovery_required: "Das Projekt benötigt eine Prüfung oder Wiederherstellung und wurde nicht geöffnet.",
  host_unavailable: "Der lokale WorldDB-Host ist gerade nicht verfügbar.",
  selection_cancelled: "Die Auswahl wurde abgebrochen.",
  invalid_request: "Bitte prüfe die Eingabe.",
  unknown_commit_outcome: "Der Speicherstatus ist unklar. Prüfe das Projekt, bevor du es erneut änderst.",
  schema_rejected: "Die Schema-Aktion wurde abgelehnt. Prüfe Eingaben, Berechtigung und aktuellen Projektstand.",
  entity_rejected: "Die Entitätsaktion wurde abgelehnt. Prüfe Eingaben, Berechtigung und aktuellen Projektstand.",
  branch_layer_rejected: "Die Branch- oder Layer-Aktion wurde abgelehnt. Prüfe Cutoff, Priorität, Berechtigung und aktuellen Projektstand.",
  history_space_transfer_rejected: "Die Übertragung wurde abgelehnt. Lade Quelle und Ziel neu und prüfe die Verweise sowie die Vorschau.",
  perspective_rejected: "Die Perspektivenaktion wurde abgelehnt. Prüfe Eingaben, Berechtigung und aktuellen Projektstand.",
  security_policy_rejected: "Die Rechteaktion wurde abgelehnt. Prüfe die erforderliche Berechtigung und lade den aktuellen Projektstand neu.",
};

let sessionId;
let projectOpen = false;
let projectRevision = null;
let projectBusy = false;
let schemaBusy = false;
let entityBusy = false;
let perspectiveBusy = false;
let securityPolicyBusy = false;
let securityPolicyUnavailable = false;
let branchLayerBusy = false;
let transferBusy = false;
let schemaCurrentMode = true;
let entityCurrentMode = true;
let selectedSchema = null;
let currentSchema = null;
let selectedEntities = null;
let selectedPerspectives = null;
let currentPerspectives = null;
let currentSecurityPolicy = null;
let selectedBranchLayers = null;
let branchLayerCurrentMode = true;
let transferCatalog = null;
let transferPreviewTicket = null;
let stagedEventRoles = [];
let stagedEventAttributes = [];
let stagedLifecycleChanges = [];

function errorCode(error) {
  if (typeof error === "string") return error;
  return error?.code ?? error?.message ?? "host_unavailable";
}

function showError(error) {
  const code = errorCode(error);
  return userMessages[code] ?? userMessages[code.split(":").at(-1)] ?? "Die Aktion konnte nicht abgeschlossen werden.";
}

function updateSchemaControls() {
  const canRead = projectOpen && !schemaBusy && !projectBusy && !entityBusy && !perspectiveBusy && !securityPolicyBusy && !branchLayerBusy && !transferBusy;
  const canMutate = canRead && schemaCurrentMode;
  schemaRefreshButton.disabled = !canRead;
  schemaCreateButton.disabled = !canMutate;
  for (const control of schemaEditor.querySelectorAll("input, select, textarea, button")) {
    control.disabled = schemaBusy || projectBusy || perspectiveBusy || securityPolicyBusy || branchLayerBusy || transferBusy;
  }
  schemaCreateButton.disabled = !canMutate;
  schemaLifecyclePublish.disabled = !canMutate || stagedLifecycleChanges.length === 0;
  for (const button of schemaDefinitions.querySelectorAll("button[data-lifecycle]")) {
    button.disabled = !canMutate;
  }
  for (const button of schemaLifecyclePending.querySelectorAll("button")) {
    button.disabled = schemaBusy || projectBusy;
  }
  updateEntityControls();
  updatePerspectiveControls();
  updateSecurityPolicyControls();
  updateBranchLayerControls();
  updateTransferControls();
  updateProjectControls();
}

function updateEntityControls() {
  if (!entityPanel) return;
  const canRead = projectOpen && !entityBusy && !projectBusy && !schemaBusy && !perspectiveBusy && !securityPolicyBusy && !branchLayerBusy && !transferBusy;
  const canMutate = canRead && entityCurrentMode;
  entityRefreshButton.disabled = !canRead;
  for (const control of entityEditor.querySelectorAll("input, select, button")) {
    control.disabled = entityBusy || projectBusy || schemaBusy || perspectiveBusy || securityPolicyBusy || branchLayerBusy || transferBusy;
  }
  entityCreateButton.disabled = !canMutate || !entityTypeSelect.value
    || (selectedEntityType()?.lifecycle === "deprecated" && !entityAcceptDeprecated.checked);
  for (const button of entityList.querySelectorAll("button[data-entity-retire]")) {
    button.disabled = !canMutate;
  }
}

function updatePerspectiveControls() {
  if (!perspectivePanel) return;
  const blocked = perspectiveBusy || securityPolicyBusy || projectBusy || schemaBusy || entityBusy || branchLayerBusy || transferBusy;
  const canRead = projectOpen && !blocked;
  const canMutate = canRead && perspectiveViewMode.value === "current";
  perspectiveRefreshButton.disabled = !canRead;
  perspectiveViewMode.disabled = blocked;
  perspectiveViewRevision.disabled = blocked;
  perspectiveCreateButton.disabled = !canMutate;
  const selected = currentPerspectives?.perspectives.find((item) => item.perspective_id === perspectiveSelect.value);
  perspectiveSelect.disabled = !canMutate || !currentPerspectives?.perspectives.length;
  perspectiveName.disabled = !canMutate;
  perspectiveDescription.disabled = !canMutate;
  perspectiveUpdateButton.disabled = !canMutate || !selected || selected.retired_revision != null;
  perspectiveRetireButton.disabled = !canMutate || !selected || selected.retired_revision != null;
  for (const [mode, select, button] of [
    [inputContextMode, inputContextPerspective, inputContextValidate],
    [queryContextMode, queryContextPerspective, queryContextValidate],
  ]) {
    mode.disabled = blocked;
    select.disabled = blocked || mode.value === "world_state" || select.options.length < 2;
    button.disabled = !canRead || (mode.value !== "world_state" && !select.value);
  }
}

function updateSecurityPolicyControls() {
  if (!securityPolicyPanel) return;
  const blocked = securityPolicyBusy || projectBusy || schemaBusy || entityBusy || perspectiveBusy || branchLayerBusy || transferBusy;
  const canRead = projectOpen && !blocked && !securityPolicyUnavailable;
  securityPolicyRefresh.disabled = !canRead;
  for (const editor of [
    securityPolicyPanel.querySelector("#security-policy-principal-editor"),
    securityPolicyPanel.querySelector("#security-policy-role-editor"),
    securityPolicyPanel.querySelector("#security-policy-rule-editor"),
  ]) {
    for (const control of editor.querySelectorAll("input, select, button")) control.disabled = !canRead;
  }
  policyPrincipalSave.disabled = !canRead || !policyPrincipalSelect.value;
  policyRoleAssign.disabled = !canRead || !policyAssignmentPrincipal.value || !policyAssignmentRole.value;
  policyRoleCreate.disabled = !canRead || !/^[a-z][a-z0-9_]*$/.test(policyNewRoleSymbol.value);
  policyRuleAdd.disabled = !canRead || !policyRuleSubject.value || !policyRuleCapability.value;
  for (const button of securityPolicyPanel.querySelectorAll("button[data-policy-revoke]")) {
    button.disabled = !canRead;
  }
}

function updateBranchLayerControls() {
  if (!branchLayerPanel) return;
  const canRead = projectOpen && !branchLayerBusy && !projectBusy && !schemaBusy && !entityBusy && !perspectiveBusy && !securityPolicyBusy && !transferBusy;
  const canMutate = canRead && branchLayerCurrentMode;
  branchLayerRefreshButton.disabled = !canRead;
  for (const editor of [branchCreateEditor, layerCreateEditor, layerEditEditor]) {
    for (const control of editor.querySelectorAll("input, select, button")) {
      control.disabled = !canMutate;
    }
  }
  const parent = selectedBranchLayers?.branches.find((item) => item.history_space_id === branchParentSelect.value);
  const cutoff = Number(branchCutoff.value);
  branchCreateButton.disabled = !canMutate || !parent || !Number.isSafeInteger(cutoff)
    || cutoff < parent.base_revision || cutoff > selectedBranchLayers.revision;
  const rank = Number(layerRank.value);
  let validSymbol = false;
  try { validateSymbol(layerSymbol.value); validSymbol = true; } catch {}
  layerCreateButton.disabled = !canMutate || !validSymbol || !Number.isInteger(rank)
    || rank <= (selectedBranchLayers?.layers.find((layer) => layer.is_base)?.precedence_rank ?? -2147483648);
  const edited = selectedBranchLayers?.layers.find((item) => item.layer_id === layerEditSelect.value);
  const editedRank = Number(layerEditRank.value);
  layerUpdateButton.disabled = !canMutate || !edited || !layerBaseSelect.value || !Number.isInteger(editedRank);
}

function updateTransferControls() {
  if (!transferPanel) return;
  const blocked = transferBusy || projectBusy || schemaBusy || entityBusy || perspectiveBusy || securityPolicyBusy || branchLayerBusy;
  const canRead = projectOpen && !blocked;
  transferLoadButton.disabled = !canRead || !transferSource.value || !transferTarget.value
    || transferSource.value === transferTarget.value;
  transferPicker.disabled = !canRead || !transferCatalog;
  transferPreviewButton.disabled = !canRead || !transferCatalog
    || transferContentList.querySelectorAll('input[type="checkbox"]:checked').length === 0;
  transferCommitButton.disabled = !canRead || !transferPreviewTicket || !transferAcknowledge.checked;
}

function updateProjectControls() {
  createButton.disabled = projectBusy || schemaBusy || entityBusy || perspectiveBusy || securityPolicyBusy || branchLayerBusy || transferBusy || projectOpen;
  openButton.disabled = projectBusy || schemaBusy || entityBusy || perspectiveBusy || securityPolicyBusy || branchLayerBusy || transferBusy || projectOpen;
  closeButton.disabled = projectBusy || schemaBusy || entityBusy || perspectiveBusy || securityPolicyBusy || branchLayerBusy || transferBusy || !projectOpen;
}

function setBusy(busy) {
  projectBusy = busy;
  updateSchemaControls();
}

function renderProject(project) {
  projectOpen = Boolean(project.project_open);
  schemaPanel.hidden = !projectOpen;
  entityPanel.hidden = !projectOpen;
  securityPolicyPanel.hidden = !projectOpen;
  perspectivePanel.hidden = !projectOpen;
  branchLayerPanel.hidden = !projectOpen;
  transferPanel.hidden = !projectOpen;
  projectRevision = projectOpen ? project.revision ?? null : null;
  if (!projectOpen) {
    projectStatus.textContent = "Kein Projekt geöffnet";
    projectDetails.textContent = "Lege ein Projekt an oder öffne einen vorhandenen Projektordner.";
    updateSchemaControls();
    return;
  }
  projectStatus.textContent = project.project_name ?? "WorldDB-Projekt geöffnet";
  projectDetails.textContent = `Rolle: ${project.role ?? "unbekannt"} · Stand: ${project.revision ?? "–"}`;
  updateSchemaControls();
}

async function getProject(activeSessionId) {
  return invoke("project_status", { sessionId: activeSessionId });
}

async function refreshProject(activeSessionId) {
  const wasOpen = projectOpen;
  const previousRevision = projectRevision;
  renderProject(await getProject(activeSessionId));
  if ((!wasOpen && projectOpen) || (wasOpen && projectOpen && projectRevision !== previousRevision && !schemaBusy)) {
    await refreshSchema(activeSessionId).catch((error) => {
      schemaStatus.textContent = showError(error);
    });
    await refreshEntities(activeSessionId).catch((error) => {
      entityStatus.textContent = showError(error);
    });
    await refreshSecurityPolicy(activeSessionId).catch((error) => {
      securityPolicyStatus.textContent = showError(error);
    });
    await refreshPerspectives(activeSessionId).catch((error) => {
      perspectiveStatus.textContent = showError(error);
    });
    await refreshBranchLayers(activeSessionId).catch((error) => {
      branchLayerStatus.textContent = showError(error);
    });
    await refreshTransferCatalog(activeSessionId).catch((error) => {
      transferStatus.textContent = showError(error);
    });
  }
  if (!projectOpen) {
    selectedSchema = null;
    currentSchema = null;
    selectedEntities = null;
    selectedPerspectives = null;
    currentPerspectives = null;
    currentSecurityPolicy = null;
    securityPolicyUnavailable = false;
    selectedBranchLayers = null;
    transferCatalog = null;
    transferPreviewTicket = null;
    schemaDefinitions.replaceChildren();
    entityList.replaceChildren();
    securityPolicyPrincipals.replaceChildren();
    securityPolicyRoles.replaceChildren();
    securityPolicyAssignments.replaceChildren();
    securityPolicyRules.replaceChildren();
    perspectiveList.replaceChildren();
    branchTree.replaceChildren();
    layerList.replaceChildren();
    transferContentList.replaceChildren();
    transferRelationList.replaceChildren();
    transferPreviewPanel.hidden = true;
    entityTypeSelect.replaceChildren();
    entityAcceptDeprecated.checked = false;
    updateEntityControls();
  }
}

async function runSecurityProbes(activeSessionId) {
  const rejects = (promise) => promise.then(() => false, () => true);
  const invalidSessionRejected = await rejects(invoke("health", {
    request: { protocol_version: 1, session_id: "00".repeat(16) },
  }));
  const rendererPathRejected = await rejects(invoke("begin_transfer", {
    sessionId: activeSessionId,
    request: {
      protocol_version: 1,
      total_bytes: 4,
      chunk_bytes: 4,
      project_path: "C:/renderer/chosen/database",
    },
  }));
  const rendererIdentityRejected = await rejects(invoke("close_project", {
    sessionId: activeSessionId,
    request: {
      protocol_version: 1,
      project_path: "C:/renderer/chosen/database",
      principal: "renderer-selected-principal",
    },
  }));
  const filesystemCommandRejected = await rejects(invoke("plugin:fs|read_text_file", {
    path: "C:/Windows/win.ini",
  }));
  if (!invalidSessionRejected || !rendererPathRejected || !rendererIdentityRejected || !filesystemCommandRejected) {
    throw new Error("A renderer security probe was unexpectedly accepted");
  }
}

async function runProjectSmoke(activeSessionId) {
  if (role === "primary") {
    await invoke("create_project", {
      sessionId: activeSessionId,
      request: { protocol_version: 1, project_name: "IPC-Smoke" },
    });
    return;
  }
  const deadline = Date.now() + 20000;
  let project;
  do {
    project = await getProject(activeSessionId);
    if (project.project_open) {
      await invoke("open_project", {
        sessionId: activeSessionId,
        request: { protocol_version: 1 },
      });
      return;
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  } while (Date.now() < deadline);
  throw new Error("Das primäre Fenster hat das Testprojekt nicht angelegt.");
}

function schemaModeInput() {
  const mode = schemaViewMode.value;
  if (mode === "current") return { mode: "current" };
  const revision = Number(schemaViewRevision.value);
  if (!Number.isSafeInteger(revision) || revision < 0) throw new Error("invalid_request");
  return mode === "historical"
    ? { mode: "historical", recorded_as_of: revision }
    : { mode: "explicit", revision };
}

async function manageSchema(command, activeSessionId = sessionId) {
  const response = await invoke("manage_schema", {
    request: { protocol_version: 1, session_id: activeSessionId, command },
  });
  if (response.protocol_version !== 1) throw new Error("unsupported_protocol");
  return response.result;
}

function entityModeInput() {
  const mode = entityViewMode.value;
  if (mode === "current") return { mode: "current" };
  const revision = Number(entityViewRevision.value);
  if (!Number.isSafeInteger(revision) || revision < 0) throw new Error("invalid_request");
  return mode === "historical"
    ? { mode: "historical", recorded_as_of: revision }
    : { mode: "explicit", revision };
}

async function manageEntities(command, activeSessionId = sessionId) {
  const response = await invoke("manage_entities", {
    request: { protocol_version: 1, session_id: activeSessionId, command },
  });
  if (response.protocol_version !== 1) throw new Error("unsupported_protocol");
  return response.result;
}

function branchLayerModeInput() {
  const mode = branchLayerViewMode.value;
  if (mode === "current") return { mode: "current" };
  const revision = Number(branchLayerViewRevision.value);
  if (!Number.isSafeInteger(revision) || revision < 0) throw new Error("invalid_request");
  return mode === "historical"
    ? { mode: "historical", recorded_as_of: revision }
    : { mode: "explicit", revision };
}

async function manageBranchLayers(command, activeSessionId = sessionId) {
  const response = await invoke("manage_branch_layers", {
    request: { protocol_version: 1, session_id: activeSessionId, command },
  });
  if (response.protocol_version !== 1) throw new Error("unsupported_protocol");
  return response.result;
}

async function manageHistorySpaceTransfer(command, activeSessionId = sessionId) {
  const response = await invoke("manage_history_space_transfer", {
    request: { protocol_version: 1, session_id: activeSessionId, command },
  });
  if (response.protocol_version !== 1) throw new Error("unsupported_protocol");
  return response.result;
}

function clearTransferPreview() {
  transferPreviewTicket = null;
  transferPreviewPanel.hidden = true;
  transferPreviewSummary.replaceChildren();
  transferAcknowledge.checked = false;
  updateTransferControls();
}

function updateTransferBranchSelectors(snapshot) {
  const labels = branchLabels(snapshot);
  const priorSource = transferSource.value;
  const priorTarget = transferTarget.value;
  for (const select of [transferSource, transferTarget]) select.replaceChildren();
  for (const branch of snapshot.branches) {
    const label = labels.get(branch.history_space_id) ?? "Branch";
    for (const select of [transferSource, transferTarget]) {
      const option = document.createElement("option");
      option.value = branch.history_space_id;
      option.textContent = label;
      select.append(option);
    }
  }
  if (snapshot.branches.some((item) => item.history_space_id === priorSource)) {
    transferSource.value = priorSource;
  }
  if (snapshot.branches.some((item) => item.history_space_id === priorTarget)) {
    transferTarget.value = priorTarget;
  } else if (snapshot.branches.length > 1) {
    transferTarget.selectedIndex = 1;
  }
  if (transferSource.value === transferTarget.value && snapshot.branches.length > 1) {
    transferTarget.selectedIndex = snapshot.branches.findIndex((item) => item.history_space_id !== transferSource.value);
  }
  transferRevision.max = String(snapshot.revision);
  if (!Number.isSafeInteger(Number(transferRevision.value)) || Number(transferRevision.value) > snapshot.revision) {
    transferRevision.value = String(snapshot.revision);
  }
}

async function refreshTransferCatalog(activeSessionId = sessionId) {
  if (!projectOpen || !activeSessionId || transferBusy) return;
  transferBusy = true;
  updateSchemaControls();
  transferStatus.textContent = "Quellinhalte werden geladen …";
  clearTransferPreview();
  transferCatalog = null;
  transferPicker.disabled = true;
  try {
    const branches = await manageBranchLayers({ command: "snapshot", mode: { mode: "current" } }, activeSessionId);
    if (branches.kind !== "snapshot") throw new Error("unsupported_protocol");
    updateTransferBranchSelectors(branches);
    if (!transferSource.value || !transferTarget.value || transferSource.value === transferTarget.value) {
      transferStatus.textContent = "Lege zuerst einen zweiten Branch an, um Inhalte übertragen zu können.";
      updateTransferControls();
      return;
    }
    await loadTransferContents(activeSessionId);
  } catch (error) {
    transferStatus.textContent = showError(error);
    transferContentList.replaceChildren();
    transferRelationList.replaceChildren();
    throw error;
  } finally {
    transferBusy = false;
    updateSchemaControls();
  }
}

async function loadTransferContents(activeSessionId = sessionId) {
  if (!activeSessionId || !transferSource.value || !transferTarget.value
    || transferSource.value === transferTarget.value) return;
  clearTransferPreview();
  transferCatalog = null;
  transferPicker.disabled = true;
  const asOf = Number(transferRevision.value);
  if (!Number.isSafeInteger(asOf) || asOf < 0) throw new Error("invalid_request");
  const result = await manageHistorySpaceTransfer({
    command: "list",
    source_history_space_id: transferSource.value,
    target_history_space_id: transferTarget.value,
    source_recorded_as_of: asOf,
  }, activeSessionId);
  if (result.kind !== "catalog") throw new Error("unsupported_protocol");
  transferCatalog = result;
  transferContentList.replaceChildren();
  transferRelationList.replaceChildren();
  if (result.records.length === 0) appendText(transferContentList, "p", "In diesem Quellstand sind keine übertragbaren Datensätze sichtbar.", "muted");
  for (const item of result.records) {
    const card = document.createElement("article");
    card.className = "definition";
    const row = document.createElement("div");
    row.className = "inline compact";
    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    checkbox.disabled = !item.selectable;
    checkbox.dataset.family = item.family;
    checkbox.dataset.recordId = item.id;
    checkbox.setAttribute("aria-label", `${item.label}, Revision ${item.recorded_revision}`);
    checkbox.addEventListener("change", updateTransferControls);
    row.append(checkbox);
    appendText(row, "span", `${item.label} · Revision ${item.recorded_revision}${item.archived ? " · archiviert" : ""}`);
    card.append(row);
    if (!item.selectable) appendText(card, "p", "Dieser Lebenszyklusdatensatz kann nicht in derselben Transferrevision kopiert werden.", "muted");
    transferContentList.append(card);
  }
  if (result.event_relations.length === 0) {
    appendText(transferRelationList, "p", "Keine übertragbaren Ereignisverknüpfungen.", "muted");
  }
  for (const item of result.event_relations) {
    const row = document.createElement("div");
    row.className = "inline compact definition";
    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    checkbox.dataset.relationId = item.id;
    checkbox.setAttribute("aria-label", `Ereignisverknüpfung: ${item.label}`);
    checkbox.addEventListener("change", updateTransferControls);
    row.append(checkbox);
    const flags = [item.archived ? "archiviert" : null, item.retracted ? "zurückgenommen" : null]
      .filter(Boolean).join(" · ");
    appendText(row, "span", `Ereignisverknüpfung · ${item.label}${flags ? ` · ${flags}` : ""}`);
    transferRelationList.append(row);
  }
  transferStatus.textContent = `Quellrevision ${result.current_revision} · Quellstand ${result.source_head} · Zielstand ${result.target_head}`;
  transferPicker.disabled = false;
  updateTransferControls();
}

async function previewTransfer() {
  if (!sessionId || !projectOpen || transferBusy || !transferCatalog) return;
  const selectedRecords = [...transferContentList.querySelectorAll('input[type="checkbox"]:checked')]
    .map((checkbox) => ({ family: checkbox.dataset.family, id: checkbox.dataset.recordId }));
  const selectedRelations = [...transferRelationList.querySelectorAll('input[type="checkbox"]:checked')]
    .map((checkbox) => checkbox.dataset.relationId);
  if (selectedRecords.length === 0) {
    transferStatus.textContent = "Wähle mindestens einen Datensatz aus.";
    return;
  }
  transferBusy = true;
  updateSchemaControls();
  transferStatus.textContent = "Übertragung und Verweise werden geprüft …";
  clearTransferPreview();
  try {
    const result = await manageHistorySpaceTransfer({
      command: "preview",
      source_history_space_id: transferSource.value,
      target_history_space_id: transferTarget.value,
      source_recorded_as_of: Number(transferRevision.value),
      selected_records: selectedRecords,
      selected_event_relations: selectedRelations,
      external_reference_policy: transferExternalPolicy.value,
    });
    if (result.kind !== "preview") throw new Error("unsupported_protocol");
    transferPreviewTicket = result.preview_ticket;
    const lines = [
      `Quellrevision ${result.source_revision} · Zielstand ${result.target_head}`,
      `${result.copied_record_count} Datensatz/Datensätze und ${result.copied_relation_count} Ereignisverknüpfung(en) werden kopiert.`,
      `${result.omitted_lifecycle_count} effektive Lebenszykluswirkung(en) und ${result.omitted_relation_retraction_count} Ereignisverknüpfungs-Zurücknahme(n) werden ausgelassen.`,
      `${result.archived_records_start_unarchived} archivierte Datensätze und ${result.archived_relations_start_unarchived} archivierte Verknüpfungen starten in der Kopie unarchiviert.`,
      "Die Vorschau gilt fünf Minuten und wird beim Veröffentlichen erneut gegen den aktuellen Zielstand geprüft.",
    ];
    transferPreviewSummary.replaceChildren();
    for (const line of lines) appendText(transferPreviewSummary, "p", line);
    transferPreviewPanel.hidden = false;
    transferStatus.textContent = "Die Vorschau ist bereit. Prüfe die Auswirkungen und bestätige sie vor der Veröffentlichung.";
  } catch (error) {
    transferStatus.textContent = showError(error);
    throw error;
  } finally {
    transferBusy = false;
    updateSchemaControls();
  }
}

async function reloadTransferContents() {
  if (!projectOpen || !sessionId || transferBusy) return;
  transferBusy = true;
  updateSchemaControls();
  try {
    await loadTransferContents();
  } catch (error) {
    transferStatus.textContent = showError(error);
  } finally {
    transferBusy = false;
    updateSchemaControls();
  }
}

async function commitTransfer() {
  if (!sessionId || !projectOpen || transferBusy || !transferPreviewTicket || !transferAcknowledge.checked) return;
  transferBusy = true;
  updateSchemaControls();
  transferStatus.textContent = "Die geprüfte Übertragung wird veröffentlicht …";
  try {
    const result = await manageHistorySpaceTransfer({
      command: "commit",
      preview_ticket: transferPreviewTicket,
      acknowledge_lifecycle_omissions: transferAcknowledge.checked,
    });
    if (result.kind !== "published") throw new Error("unsupported_protocol");
    transferPreviewTicket = null;
    transferStatus.textContent = `Übertragung veröffentlicht · Datenrevision ${result.revision} · ${result.copied_record_count} Datensatz/Datensätze kopiert.`;
    await refreshProject(sessionId);
    await loadTransferContents(sessionId);
  } catch (error) {
    transferStatus.textContent = showError(error);
  } finally {
    transferBusy = false;
    updateSchemaControls();
  }
}

async function refreshBranchLayers(activeSessionId = sessionId) {
  if (!projectOpen || !activeSessionId || branchLayerBusy) return;
  branchLayerBusy = true;
  updateSchemaControls();
  branchLayerStatus.textContent = "Branches und Layer werden geladen …";
  try {
    const result = await manageBranchLayers({ command: "snapshot", mode: branchLayerModeInput() }, activeSessionId);
    if (result.kind !== "snapshot") throw new Error("unsupported_protocol");
    selectedBranchLayers = result;
    branchLayerCurrentMode = branchLayerViewMode.value === "current";
    renderBranchLayers(result);
    updateBranchLayerControls();
  } catch (error) {
    branchLayerStatus.textContent = showError(error);
    branchTree.replaceChildren();
    layerList.replaceChildren();
    updateBranchLayerControls();
    throw error;
  } finally {
    branchLayerBusy = false;
    updateSchemaControls();
  }
}

function branchLabels(snapshot) {
  const byId = new Map(snapshot.branches.map((branch) => [branch.history_space_id, branch]));
  const roots = snapshot.branches.filter((branch) => branch.parent_history_space_id == null);
  const labels = new Map();
  function getLabel(branch, path = new Set()) {
    if (labels.has(branch.history_space_id)) return labels.get(branch.history_space_id);
    if (path.has(branch.history_space_id)) return "Ungültige Verzweigung";
    path.add(branch.history_space_id);
    let label;
    if (branch.parent_history_space_id == null) {
      const rootIndex = roots.findIndex((item) => item.history_space_id === branch.history_space_id);
      label = roots.length <= 1 ? "Hauptbereich" : `Hauptbereich ${rootIndex + 1}`;
    } else {
      const parent = byId.get(branch.parent_history_space_id);
      if (!parent) label = "Unbekannter Parent";
      else {
        const siblings = snapshot.branches
          .filter((item) => item.parent_history_space_id === branch.parent_history_space_id)
          .sort((left, right) => left.history_space_id.localeCompare(right.history_space_id));
        const siblingIndex = siblings.findIndex((item) => item.history_space_id === branch.history_space_id);
        label = `${getLabel(parent, path)} › Abzweig ${siblingIndex + 1}`;
      }
    }
    path.delete(branch.history_space_id);
    labels.set(branch.history_space_id, label);
    return label;
  }
  for (const branch of snapshot.branches) getLabel(branch);
  return labels;
}

function renderBranchLayers(snapshot) {
  const labels = branchLabels(snapshot);
  branchLayerStatus.textContent = `Datenrevision ${snapshot.revision} · ${snapshot.branches.length} Branch(es) · ${snapshot.layers.length} Layer`;
  branchTree.replaceChildren();
  if (snapshot.branches.length === 0) appendText(branchTree, "p", "In diesem Stand gibt es noch keine Branches.", "muted");
  for (const branch of snapshot.branches) {
    const card = document.createElement("article");
    card.className = "definition";
    appendText(card, "h4", labels.get(branch.history_space_id) ?? "Branch");
    const parent = branch.parent_history_space_id == null
      ? "Kein Parent (Hauptbereich)"
      : `Parent: ${labels.get(branch.parent_history_space_id) ?? "nicht verfügbar"}`;
    appendText(card, "p", `${parent} · Parent-Cutoff: Revision ${branch.base_revision}`);
    branchTree.append(card);
  }

  layerList.replaceChildren();
  const layers = [...snapshot.layers].sort((left, right) => left.precedence_rank - right.precedence_rank || left.symbol.localeCompare(right.symbol));
  for (const layer of layers) {
    const card = document.createElement("article");
    card.className = "definition";
    const baseLabel = layer.is_base ? " · Basis" : " · Overlay";
    appendText(card, "h4", `${layer.symbol}${baseLabel}`);
    appendText(card, "p", `Priorität ${layer.precedence_rank} · ${lifecycleLabel(layer.lifecycle)}`);
    if (layer.description) appendText(card, "p", layer.description, "muted");
    layerList.append(card);
  }
  updateBranchLayerSelectors(snapshot, labels);
}

function updateBranchLayerSelectors(snapshot, labels) {
  const priorParent = branchParentSelect.value;
  branchParentSelect.replaceChildren();
  for (const branch of snapshot.branches) {
    const option = document.createElement("option");
    option.value = branch.history_space_id;
    option.textContent = `${labels.get(branch.history_space_id) ?? "Branch"} · Cutoff ${branch.base_revision}`;
    branchParentSelect.append(option);
  }
  if (snapshot.branches.some((branch) => branch.history_space_id === priorParent)) branchParentSelect.value = priorParent;
  updateBranchCutoffBounds();

  const priorLayer = layerEditSelect.value;
  layerEditSelect.replaceChildren();
  for (const layer of snapshot.layers) {
    const option = document.createElement("option");
    option.value = layer.layer_id;
    option.textContent = `${layer.symbol}${layer.is_base ? " · Basis" : ""} · ${lifecycleLabel(layer.lifecycle)}`;
    layerEditSelect.append(option);
  }
  if (snapshot.layers.some((layer) => layer.layer_id === priorLayer)) layerEditSelect.value = priorLayer;
  else if (snapshot.layers.length) layerEditSelect.value = snapshot.base_layer_id;

  const priorBase = layerBaseSelect.value;
  layerBaseSelect.replaceChildren();
  for (const layer of snapshot.layers.filter((item) => item.lifecycle === "active")) {
    const option = document.createElement("option");
    option.value = layer.layer_id;
    option.textContent = layer.symbol;
    layerBaseSelect.append(option);
  }
  if ([...layerBaseSelect.options].some((option) => option.value === priorBase)) layerBaseSelect.value = priorBase;
  else if ([...layerBaseSelect.options].some((option) => option.value === snapshot.base_layer_id)) layerBaseSelect.value = snapshot.base_layer_id;
  loadLayerEditForm();
}

function updateBranchCutoffBounds() {
  const parent = selectedBranchLayers?.branches.find((branch) => branch.history_space_id === branchParentSelect.value);
  if (!parent) return;
  branchCutoff.min = String(parent.base_revision);
  branchCutoff.max = String(selectedBranchLayers.revision);
  const value = Number(branchCutoff.value);
  if (!Number.isSafeInteger(value) || value < parent.base_revision || value > selectedBranchLayers.revision) {
    branchCutoff.value = String(Math.max(parent.base_revision, selectedBranchLayers.revision));
  }
  updateBranchLayerControls();
}

function loadLayerEditForm() {
  const layer = selectedBranchLayers?.layers.find((item) => item.layer_id === layerEditSelect.value);
  if (!layer) return;
  layerEditRank.value = String(layer.precedence_rank);
  layerEditDescription.value = layer.description ?? "";
  layerLifecycle.value = layer.lifecycle;
  updateBranchLayerControls();
}

async function publishBranchLayer(command, successMessage) {
  if (!sessionId || !projectOpen || !branchLayerCurrentMode || branchLayerBusy || !selectedBranchLayers) return;
  branchLayerBusy = true;
  updateSchemaControls();
  branchLayerStatus.textContent = "Änderung wird geprüft und gespeichert …";
  try {
    const result = await manageBranchLayers({
      ...command,
      expected_base_revision: selectedBranchLayers.revision,
    });
    if (result.kind !== "published") throw new Error("unsupported_protocol");
    branchLayerStatus.textContent = `${successMessage} · Datenrevision ${result.revision}`;
    branchLayerViewMode.value = "current";
    branchLayerCurrentMode = true;
    await refreshBranchLayers();
  } catch (error) {
    branchLayerStatus.textContent = showError(error);
  } finally {
    branchLayerBusy = false;
    updateSchemaControls();
  }
}

function createChildBranch() {
  if (!branchParentSelect.value) return;
  return publishBranchLayer({
    command: "create_child",
    parent_history_space_id: branchParentSelect.value,
    base_revision: Number(branchCutoff.value),
  }, "Child-Branch angelegt");
}

function createOverlayLayer() {
  let symbol;
  try { symbol = validateSymbol(layerSymbol.value); } catch {
    branchLayerStatus.textContent = "Das Layer-Symbol muss mit einem Kleinbuchstaben beginnen und darf nur Kleinbuchstaben, Zahlen und Unterstriche enthalten.";
    return;
  }
  const rank = Number(layerRank.value);
  if (!Number.isInteger(rank)) {
    branchLayerStatus.textContent = "Bitte gib eine gültige Layer-Priorität ein.";
    return;
  }
  return publishBranchLayer({
    command: "create_layer",
    symbol,
    description: layerDescription.value.trim() || null,
    precedence_rank: rank,
  }, "Overlay-Layer angelegt");
}

function reviseSelectedLayer() {
  const layer = selectedBranchLayers?.layers.find((item) => item.layer_id === layerEditSelect.value);
  if (!layer) return;
  const newBase = layerBaseSelect.value;
  if (layerLifecycle.value === "retired" && layer.lifecycle !== "retired"
    && !window.confirm("Soll dieser Layer dauerhaft stillgelegt werden? Die frühere Layer-Historie bleibt erhalten.")) return;
  if (newBase !== selectedBranchLayers.base_layer_id
    && !window.confirm("Soll der ausgewählte Layer die neue Basis werden? Die beiden Prioritäten werden dafür getauscht.")) return;
  return publishBranchLayer({
    command: "revise_layer",
    layer_id: layer.layer_id,
    description: layerEditDescription.value.trim() || null,
    precedence_rank: Number(layerEditRank.value),
    lifecycle: layerLifecycle.value,
    base_layer_id: newBase,
  }, "Layer-Änderung veröffentlicht");
}

async function invokeEntitiesFor(activeSessionId, mode) {
  const result = await manageEntities({ command: "snapshot", mode }, activeSessionId);
  if (result.kind !== "snapshot") throw new Error("unsupported_protocol");
  return result;
}

async function refreshEntities(activeSessionId = sessionId) {
  if (!projectOpen || !activeSessionId) return;
  entityStatus.textContent = "Entitäten werden geladen …";
  entityRefreshButton.disabled = true;
  try {
    selectedEntities = await invokeEntitiesFor(activeSessionId, entityModeInput());
    entityCurrentMode = entityViewMode.value === "current";
    renderEntities(selectedEntities);
    updateEntityTypeSelect(selectedEntities);
    updateEntityControls();
  } catch (error) {
    entityStatus.textContent = showError(error);
    entityList.replaceChildren();
    updateEntityControls();
    throw error;
  }
}

function selectedEntityType() {
  return selectedEntities?.entity_types.find((item) => item.entity_type_id === entityTypeSelect.value) ?? null;
}

function updateEntityTypeSelect(snapshot) {
  const previous = entityTypeSelect.value;
  const available = snapshot.entity_types.filter((item) => item.lifecycle !== "retired");
  entityTypeSelect.replaceChildren();
  for (const type of available) {
    const option = document.createElement("option");
    option.value = type.entity_type_id;
    option.textContent = `${type.symbol} · ${lifecycleLabel(type.lifecycle)}`;
    entityTypeSelect.append(option);
  }
  if (available.some((item) => item.entity_type_id === previous)) entityTypeSelect.value = previous;
  if (available.length === 0) {
    const option = document.createElement("option");
    option.value = "";
    option.textContent = "Kein verwendbarer EntityType vorhanden";
    entityTypeSelect.append(option);
  }
  updateEntityTypeSelectionState();
}

function updateEntityTypeSelectionState() {
  const deprecated = selectedEntityType()?.lifecycle === "deprecated";
  entityDeprecatedOptIn.hidden = !deprecated;
  entityDeprecatedWarning.hidden = !deprecated;
  if (!deprecated) entityAcceptDeprecated.checked = false;
  updateEntityControls();
}

function renderEntities(snapshot) {
  entityStatus.textContent = `Datenrevision ${snapshot.revision} · ${snapshot.entities.length} Entität(en)`;
  entityList.replaceChildren();
  if (snapshot.entities.length === 0) {
    appendText(entityList, "p", "In diesem Stand sind noch keine Entitäten vorhanden.", "muted");
    return;
  }
  for (const entity of snapshot.entities) {
    const card = document.createElement("article");
    card.className = "definition";
    appendText(card, "h3", `Entität · ${entity.entity_type_symbol}`);
    const status = entity.retired_revision == null
      ? "Aktiv"
      : `Stillgelegt in Datenrevision ${entity.retired_revision}`;
    appendText(card, "p", `${status} · angelegt in Datenrevision ${entity.created_revision}`);
    appendText(card, "p", `EntityType: ${entity.entity_type_symbol} · ${lifecycleLabel(entity.entity_type_lifecycle)}`, "muted");
    if (entityCurrentMode && entity.retired_revision == null) {
      const retire = document.createElement("button");
      retire.type = "button";
      retire.dataset.entityRetire = "true";
      retire.textContent = "Entität stilllegen";
      retire.addEventListener("click", () => retireEntity(entity.entity_id));
      card.append(retire);
    }
    entityList.append(card);
  }
}

async function invokeSecurityPolicyFor(activeSessionId, command) {
  const response = await invoke("manage_security_policy", {
    request: { protocol_version: 1, session_id: activeSessionId, command },
  });
  if (response.protocol_version !== 1 || !response.result?.kind) throw new Error("unsupported_protocol");
  return response.result;
}

async function refreshSecurityPolicy(activeSessionId = sessionId) {
  if (!activeSessionId || !projectOpen || securityPolicyBusy) return;
  securityPolicyBusy = true;
  updateSchemaControls();
  securityPolicyStatus.textContent = "Berechtigungen werden geladen …";
  try {
    const result = await invokeSecurityPolicyFor(activeSessionId, { command: "snapshot" });
    if (result.kind !== "snapshot") throw new Error("unsupported_protocol");
    currentSecurityPolicy = result;
    securityPolicyUnavailable = false;
    renderSecurityPolicy(result);
    updateSecurityPolicyControls();
  } catch (error) {
    currentSecurityPolicy = null;
    securityPolicyUnavailable = true;
    securityPolicyPrincipals.replaceChildren();
    securityPolicyRoles.replaceChildren();
    securityPolicyAssignments.replaceChildren();
    securityPolicyRules.replaceChildren();
    securityPolicyStatus.textContent = showError(error);
    throw error;
  } finally {
    securityPolicyBusy = false;
    updateSchemaControls();
  }
}

function renderSecurityPolicy(snapshot) {
  securityPolicyStatus.textContent = `Projektrevision ${snapshot.revision} · SecurityEpoch ${snapshot.security_epoch}`;
  securityPolicyPrincipals.replaceChildren();
  securityPolicyRoles.replaceChildren();
  securityPolicyAssignments.replaceChildren();
  securityPolicyRules.replaceChildren();

  const previousPrincipal = policyPrincipalSelect.value;
  const previousAssignmentPrincipal = policyAssignmentPrincipal.value;
  const previousRole = policyAssignmentRole.value;
  const previousSubject = policyRuleSubject.value;
  const previousCapability = policyRuleCapability.value;
  fillPolicySelect(policyPrincipalSelect, snapshot.principals.map((item) => ({
    value: item.principal_id,
    label: `${item.principal_id} · ${principalStateLabel(item.state)}`,
  })), previousPrincipal);
  fillPolicySelect(policyAssignmentPrincipal, snapshot.principals
    .filter((item) => item.state === "active")
    .map((item) => ({ value: item.principal_id, label: item.principal_id })), previousAssignmentPrincipal);
  fillPolicySelect(policyAssignmentRole, snapshot.roles.map((item) => ({
    value: item.role_id,
    label: `${item.symbol} · ${item.role_id}`,
  })), previousRole);
  const subjects = [
    ...snapshot.principals.map((item) => ({
      value: `principal:${item.principal_id}`,
      label: `Benutzerkonto · ${item.principal_id}`,
    })),
    ...snapshot.roles.map((item) => ({
      value: `role:${item.role_id}`,
      label: `Rolle · ${item.symbol}`,
    })),
  ];
  fillPolicySelect(policyRuleSubject, subjects, previousSubject);
  fillPolicySelect(policyRuleCapability, snapshot.capabilities.map((item) => ({
    value: item,
    label: capabilityLabel(item),
  })), previousCapability);

  if (snapshot.principals.length === 0) {
    appendText(securityPolicyPrincipals, "p", "Keine registrierten Benutzerkonten.", "muted");
  }
  for (const principal of snapshot.principals) {
    const card = document.createElement("article");
    card.className = "definition";
    appendText(card, "p", `${principalStateLabel(principal.state)} · ${principal.principal_id}`);
    const assignments = snapshot.assignments
      .filter((item) => item.principal_id === principal.principal_id)
      .map((item) => snapshot.roles.find((roleItem) => roleItem.role_id === item.role_id)?.symbol ?? "unbekannte Rolle");
    appendText(card, "p", assignments.length ? `Rollen: ${assignments.join(", ")}` : "Keine Rolle zugewiesen.", "muted");
    securityPolicyPrincipals.append(card);
  }

  if (snapshot.roles.length === 0) appendText(securityPolicyRoles, "p", "Keine registrierten Rollen.", "muted");
  for (const roleItem of snapshot.roles) {
    const card = document.createElement("article");
    card.className = "definition";
    appendText(card, "h3", `${roleItem.symbol} · Policy-Bundle`);
    appendText(card, "p", `ID: ${roleItem.role_id}`, "muted");
    const granted = roleItem.bundle.map((item) => `${capabilityLabel(item.capability)} (${effectLabel(item.effect)})`);
    appendText(card, "p", granted.length ? `Basis-Bundle: ${granted.join(", ")}` : "Basis-Bundle ist leer.", "muted");
    const hasAdminRaw = roleItem.bundle.some((item) => item.capability === "admin_raw_read" && item.effect === "allow");
    const hasRawHistory = roleItem.bundle.some((item) => item.capability === "raw_history_read" && item.effect === "allow");
    appendText(card, "p", `AdminRawRead: ${hasAdminRaw ? "explizit erlaubt" : "nicht im Basis-Bundle"} · RawHistoryRead: ${hasRawHistory ? "im Basis-Bundle" : "nicht im Basis-Bundle"}`, "muted");
    const extraRules = snapshot.explicit_rules.filter((rule) => rule.subject_kind === "role" && rule.subject_id === roleItem.role_id);
    appendText(card, "p", `Zusätzliche explizite Regeln: ${extraRules.length}${extraRules.length ? ` · ${extraRules.map((item) => `${capabilityLabel(item.capability)} (${effectLabel(item.effect)})`).join(", ")}` : ""}`, "muted");
    securityPolicyRoles.append(card);
  }

  if (snapshot.assignments.length === 0) appendText(securityPolicyAssignments, "p", "Noch keine Rollen zugewiesen.", "muted");
  for (const assignment of snapshot.assignments) {
    const row = document.createElement("div");
    row.className = "definition";
    const roleItem = snapshot.roles.find((item) => item.role_id === assignment.role_id);
    appendText(row, "p", `${assignment.principal_id} → ${roleItem?.symbol ?? "unbekannte Rolle"} · Bereich: ${assignment.scope}`);
    const revoke = document.createElement("button");
    revoke.type = "button";
    revoke.dataset.policyRevoke = "assignment";
    revoke.textContent = "Rolle entziehen";
    revoke.addEventListener("click", () => revokePolicyAssignment(assignment.assignment_id));
    row.append(revoke);
    securityPolicyAssignments.append(row);
  }

  if (snapshot.explicit_rules.length === 0) appendText(securityPolicyRules, "p", "Noch keine zusätzlichen Capabilities eingetragen.", "muted");
  for (const rule of snapshot.explicit_rules) {
    const row = document.createElement("div");
    row.className = "definition";
    const subjectLabel = rule.subject_kind === "role"
      ? snapshot.roles.find((item) => item.role_id === rule.subject_id)?.symbol ?? rule.subject_id
      : rule.subject_id;
    appendText(row, "p", `${rule.subject_kind === "role" ? "Rolle" : "Benutzerkonto"} ${subjectLabel} · ${capabilityLabel(rule.capability)} · ${effectLabel(rule.effect)} · ${rule.scope}`);
    appendText(row, "p", `Regel-ID: ${rule.rule_id}`, "muted");
    const revoke = document.createElement("button");
    revoke.type = "button";
    revoke.dataset.policyRevoke = "rule";
    revoke.textContent = "Regel entziehen";
    revoke.addEventListener("click", () => revokePolicyRule(rule.rule_id));
    row.append(revoke);
    securityPolicyRules.append(row);
  }
  if (snapshot.capabilities.includes("admin_raw_read")) {
    appendText(securityPolicyStatus, "p", "AdminRawRead ist eine eigene Capability und wird keiner GM-Rolle automatisch erteilt.", "muted");
  }
  policyPrincipalState.value = snapshot.principals.find((item) => item.principal_id === policyPrincipalSelect.value)?.state ?? "active";
  updateSecurityPolicyControls();
}

function fillPolicySelect(select, entries, previousValue) {
  select.replaceChildren();
  for (const entry of entries) {
    const option = document.createElement("option");
    option.value = entry.value;
    option.textContent = entry.label;
    select.append(option);
  }
  if (entries.some((entry) => entry.value === previousValue)) select.value = previousValue;
  else if (entries.length > 0) select.value = entries[0].value;
}

function principalStateLabel(value) {
  return ({ active: "Aktiv", disabled: "Deaktiviert", retired: "Dauerhaft stillgelegt" })[value] ?? value;
}

function capabilityLabel(value) {
  return value.replaceAll("_", " ");
}

function effectLabel(value) {
  return value === "allow" ? "Erlauben" : "Verweigern";
}

async function publishPolicyChange(command, successText) {
  if (!currentSecurityPolicy) throw new Error("security_policy_rejected");
  securityPolicyBusy = true;
  updateSchemaControls();
  securityPolicyStatus.textContent = "Rechteänderung wird geprüft und atomar veröffentlicht …";
  try {
    const result = await invokeSecurityPolicyFor(sessionId, {
      ...command,
      expected_base_revision: currentSecurityPolicy.revision,
    });
    if (result.kind !== "published") throw new Error("unsupported_protocol");
    securityPolicyStatus.textContent = `${successText} · Revision ${result.revision} · SecurityEpoch ${result.security_epoch}.`;
    try {
      const snapshot = await invokeSecurityPolicyFor(sessionId, { command: "snapshot" });
      if (snapshot.kind !== "snapshot") throw new Error("unsupported_protocol");
      currentSecurityPolicy = snapshot;
      securityPolicyUnavailable = false;
      renderSecurityPolicy(snapshot);
    } catch {
      currentSecurityPolicy = null;
      securityPolicyUnavailable = true;
      securityPolicyStatus.textContent += " Die neue Richtlinie gilt bereits; dieses Konto darf die aktuelle Richtlinie nicht mehr lesen.";
    }
    await refreshProject(sessionId).catch(() => {});
  } catch (error) {
    securityPolicyStatus.textContent = showError(error);
  } finally {
    securityPolicyBusy = false;
    updateSchemaControls();
  }
}

function savePrincipalState() {
  const principal = currentSecurityPolicy?.principals.find((item) => item.principal_id === policyPrincipalSelect.value);
  const state = policyPrincipalState.value;
  if (!principal || principal.state === state) return;
  if (state === "retired" && !window.confirm("Dieses Benutzerkonto wird dauerhaft stillgelegt und kann nicht wieder aktiviert werden. Fortfahren?")) return;
  return publishPolicyChange({
    command: "set_principal_state",
    principal_id: principal.principal_id,
    state,
  }, "Benutzerkonto aktualisiert");
}

function createPolicyRole() {
  const symbol = policyNewRoleSymbol.value.trim();
  if (!/^[a-z][a-z0-9_]*$/.test(symbol)) {
    securityPolicyStatus.textContent = "Das Rollensymbol muss [a-z][a-z0-9_]* entsprechen.";
    return;
  }
  return publishPolicyChange({ command: "register_role", symbol }, "Leere Rolle angelegt");
}

function assignPolicyRole() {
  if (!policyAssignmentPrincipal.value || !policyAssignmentRole.value) return;
  return publishPolicyChange({
    command: "assign_role",
    principal_id: policyAssignmentPrincipal.value,
    role_id: policyAssignmentRole.value,
  }, "Rolle zugewiesen");
}

function addPolicyRule() {
  const [subjectKind, subjectId] = policyRuleSubject.value.split(":", 2);
  if (!subjectId || !policyRuleCapability.value) return;
  return publishPolicyChange({
    command: "add_capability_rule",
    subject_kind: subjectKind,
    subject_id: subjectId,
    capability: policyRuleCapability.value,
    effect: policyRuleEffect.value,
  }, "Explizite Berechtigung veröffentlicht");
}

function revokePolicyAssignment(assignmentId) {
  return publishPolicyChange({ command: "revoke_role_assignment", assignment_id: assignmentId }, "Rolle entzogen");
}

function revokePolicyRule(ruleId) {
  return publishPolicyChange({ command: "revoke_capability_rule", rule_id: ruleId }, "Explizite Berechtigung entzogen");
}

function perspectiveModeInput() {
  if (perspectiveViewMode.value === "current") return { mode: "current" };
  const revision = Number(perspectiveViewRevision.value);
  if (!Number.isSafeInteger(revision) || revision < 0) throw new Error("invalid_request");
  return perspectiveViewMode.value === "historical"
    ? { mode: "historical", recorded_as_of: revision }
    : { mode: "explicit", revision };
}

async function invokePerspectivesFor(activeSessionId, command) {
  const response = await invoke("manage_perspectives", {
    request: { protocol_version: 1, session_id: activeSessionId, command },
  });
  if (response.protocol_version !== 1 || !response.result?.kind) throw new Error("unsupported_protocol");
  return response.result;
}

async function invokePerspectiveSnapshot(activeSessionId, mode) {
  const result = await invokePerspectivesFor(activeSessionId, { command: "snapshot", mode });
  if (result.kind !== "snapshot") throw new Error("unsupported_protocol");
  return result;
}

function perspectiveNameFor(view) {
  return view.display_name || `Unbenannte Perspektive · Revision ${view.created_revision}`;
}

function fillPerspectiveOptions(select, perspectives, previousValue) {
  select.replaceChildren();
  const empty = document.createElement("option");
  empty.value = "";
  empty.textContent = "Keine Perspektive ausgewählt";
  select.append(empty);
  const active = perspectives.filter((item) => item.retired_revision == null);
  for (const item of active) {
    const option = document.createElement("option");
    option.value = item.perspective_id;
    option.textContent = perspectiveNameFor(item);
    select.append(option);
  }
  if (active.some((item) => item.perspective_id === previousValue)) select.value = previousValue;
  else select.value = "";
}

function renderPerspectiveSnapshot(snapshot) {
  perspectiveStatus.textContent = `Katalogrevision ${snapshot.revision} · ${snapshot.perspectives.length} Perspektive(n)`;
  perspectiveList.replaceChildren();
  if (snapshot.perspectives.length === 0) {
    appendText(perspectiveList, "p", "In diesem Stand sind noch keine Perspektiven vorhanden.", "muted");
    return;
  }
  for (const item of snapshot.perspectives) {
    const card = document.createElement("article");
    card.className = "definition";
    appendText(card, "h3", perspectiveNameFor(item));
    appendText(card, "p", item.retired_revision == null
      ? `Aktiv · angelegt in Revision ${item.created_revision}`
      : `Stillgelegt in Revision ${item.retired_revision} · angelegt in Revision ${item.created_revision}`);
    if (item.description) appendText(card, "p", item.description, "muted");
    if (item.metadata_history.length > 1) {
      appendText(card, "p", "Metadatenverlauf", "muted");
      for (const revision of item.metadata_history) {
        const details = [
          revision.display_name || "Unbenannte Perspektive",
          revision.description,
        ].filter(Boolean).join(" · ");
        appendText(card, "p", `Revision ${revision.recorded_revision}: ${details}`, "muted");
      }
    }
    perspectiveList.append(card);
  }
}

function renderPerspectiveChoices() {
  const previousEditorValue = perspectiveSelect.value;
  const previousInputValue = inputContextPerspective.value;
  const previousQueryValue = queryContextPerspective.value;
  const active = currentPerspectives?.perspectives ?? [];
  fillPerspectiveOptions(perspectiveSelect, active, previousEditorValue);
  fillPerspectiveOptions(inputContextPerspective, active, previousInputValue);
  fillPerspectiveOptions(queryContextPerspective, active, previousQueryValue);
  inputContextStatus.textContent = "Kontext noch nicht geprüft.";
  queryContextStatus.textContent = "Kontext noch nicht geprüft.";
  const selected = active.find((item) => item.perspective_id === perspectiveSelect.value);
  if (selected) {
    perspectiveName.value = selected.display_name ?? "";
    perspectiveDescription.value = selected.description ?? "";
  } else if (!perspectiveBusy) {
    perspectiveName.value = "";
    perspectiveDescription.value = "";
  }
  updatePerspectiveControls();
}

async function refreshPerspectives(activeSessionId = sessionId) {
  if (!projectOpen || !activeSessionId) return;
  perspectiveStatus.textContent = "Perspektiven werden geladen …";
  perspectiveRefreshButton.disabled = true;
  try {
    selectedPerspectives = await invokePerspectiveSnapshot(activeSessionId, perspectiveModeInput());
    currentPerspectives = perspectiveViewMode.value === "current"
      ? selectedPerspectives
      : await invokePerspectiveSnapshot(activeSessionId, { mode: "current" });
    renderPerspectiveSnapshot(selectedPerspectives);
    renderPerspectiveChoices();
  } catch (error) {
    perspectiveStatus.textContent = showError(error);
    perspectiveList.replaceChildren();
    throw error;
  } finally {
    updatePerspectiveControls();
  }
}

async function publishPerspectiveMutation(command, successText) {
  if (!sessionId || !projectOpen || perspectiveBusy || perspectiveViewMode.value !== "current") return;
  perspectiveBusy = true;
  perspectiveStatus.textContent = "Perspektivenänderung wird geprüft und veröffentlicht …";
  updateSchemaControls();
  try {
    const response = await invokePerspectivesFor(sessionId, command);
    if (response.kind !== "published") throw new Error("unsupported_protocol");
    perspectiveStatus.textContent = `${successText} Katalogrevision ${response.revision}.`;
    await refreshPerspectives(sessionId);
  } catch (error) {
    perspectiveStatus.textContent = showError(error);
  } finally {
    perspectiveBusy = false;
    updateSchemaControls();
  }
}

async function createPerspective() {
  if (!sessionId || !projectOpen || perspectiveBusy) return;
  try {
    const latest = await invokePerspectiveSnapshot(sessionId, { mode: "current" });
    await publishPerspectiveMutation({
      command: "create",
      expected_base_revision: latest.revision,
      display_name: perspectiveName.value.trim() || null,
      description: perspectiveDescription.value.trim() || null,
    }, "Perspektive angelegt.");
  } catch (error) {
    perspectiveStatus.textContent = showError(error);
  }
}

async function updatePerspective() {
  if (!sessionId || !projectOpen || perspectiveBusy || !perspectiveSelect.value) return;
  try {
    const latest = await invokePerspectiveSnapshot(sessionId, { mode: "current" });
    const selected = latest.perspectives.find((item) => item.perspective_id === perspectiveSelect.value && item.retired_revision == null);
    if (!selected) throw new Error("perspective_rejected");
    await publishPerspectiveMutation({
      command: "update",
      expected_base_revision: latest.revision,
      perspective_id: selected.perspective_id,
      display_name: perspectiveName.value.trim() || null,
      description: perspectiveDescription.value.trim() || null,
    }, "Perspektivendaten aktualisiert.");
  } catch (error) {
    perspectiveStatus.textContent = showError(error);
  }
}

async function retirePerspective() {
  if (!sessionId || !projectOpen || perspectiveBusy || !perspectiveSelect.value) return;
  try {
    const latest = await invokePerspectiveSnapshot(sessionId, { mode: "current" });
    const selected = latest.perspectives.find((item) => item.perspective_id === perspectiveSelect.value && item.retired_revision == null);
    if (!selected) throw new Error("perspective_rejected");
    await publishPerspectiveMutation({
      command: "retire",
      expected_base_revision: latest.revision,
      perspective_id: selected.perspective_id,
    }, "Perspektive stillgelegt.");
  } catch (error) {
    perspectiveStatus.textContent = showError(error);
  }
}

async function validateContext(modeSelect, perspectiveControl, status, label) {
  if (!sessionId || !projectOpen) return;
  const epistemicMode = modeSelect.value;
  const perspectiveId = epistemicMode === "world_state" ? null : perspectiveControl.value || null;
  if (epistemicMode !== "world_state" && perspectiveId == null) {
    status.textContent = "Wähle für Knows, Believes oder Claims ausdrücklich eine aktive Perspektive.";
    updatePerspectiveControls();
    return;
  }
  status.textContent = `${label} wird geprüft …`;
  try {
    const result = await invokePerspectivesFor(sessionId, {
      command: "validate_context",
      epistemic_mode: epistemicMode,
      perspective_id: perspectiveId,
    });
    if (result.kind !== "context_bound") throw new Error("unsupported_protocol");
    status.textContent = result.perspective_label
      ? `${label} geprüft: ${epistemicModeLabel(epistemicMode)} · ${result.perspective_label}`
      : `${label} geprüft: WorldState ohne Perspektive`;
  } catch (error) {
    status.textContent = showError(error);
  }
}

function epistemicModeLabel(value) {
  return ({ world_state: "WorldState", knows: "Knows", believes: "Believes", claims: "Claims" })[value] ?? value;
}

async function refreshSchema(activeSessionId = sessionId) {
  if (!projectOpen || !activeSessionId) return;
  schemaStatus.textContent = "Schema wird geladen …";
  schemaRefreshButton.disabled = true;
  try {
    const selected = await invokeSchemaFor(activeSessionId, schemaModeInput());
    selectedSchema = selected;
    schemaCurrentMode = schemaViewMode.value === "current";
    currentSchema = schemaCurrentMode
      ? selected
      : await invokeSchemaFor(activeSessionId, { mode: "current" });
    renderSchema(selected);
    updateEntityTypeChoices();
    updateSchemaControls();
  } catch (error) {
    schemaStatus.textContent = showError(error);
    schemaDefinitions.replaceChildren();
    updateSchemaControls();
    throw error;
  }
}

async function invokeSchemaFor(activeSessionId, mode) {
  const response = await invoke("manage_schema", {
    request: {
      protocol_version: 1,
      session_id: activeSessionId,
      command: { command: "snapshot", mode },
    },
  });
  if (response.protocol_version !== 1 || response.result.kind !== "snapshot") throw new Error("unsupported_protocol");
  return response.result;
}

async function runSchemaSmoke(activeSessionId) {
  await refreshProject(activeSessionId);
  schemaViewMode.value = "current";
  schemaCurrentMode = true;
  schemaFamily.value = "entity_type";
  schemaSymbol.value = "ipc_smoke_entity";
  schemaDescription.value = "Temporary schema definition for authenticated IPC verification.";
  const baseline = await invokeSchemaFor(activeSessionId, { mode: "current" });
  await publishDefinition();
  const definition = selectedSchema?.definitions.find((item) => item.symbol === "ipc_smoke_entity");
  if (!definition) throw new Error("schema create did not publish its definition");
  const createdRevision = selectedSchema.revision;

  const historical = await invokeSchemaFor(activeSessionId, {
    mode: "historical",
    recorded_as_of: baseline.revision,
  });
  const explicit = await invokeSchemaFor(activeSessionId, {
    mode: "explicit",
    revision: createdRevision,
  });
  if (historical.definitions.some((item) => item.identity === definition.identity)) {
    throw new Error("historical schema view included a later definition");
  }
  if (!explicit.definitions.some((item) => item.identity === definition.identity)) {
    throw new Error("explicit schema view omitted the published definition");
  }

  stageDefinitionLifecycle(definition, "deprecated");
  await publishLifecycleBatch();
  const deprecatedDefinition = selectedSchema?.definitions.find((item) => item.identity === definition.identity);
  if (deprecatedDefinition?.lifecycle !== "deprecated") {
    throw new Error("schema deprecation did not publish the requested lifecycle state");
  }
  stageDefinitionLifecycle(deprecatedDefinition, "retired");
  await publishLifecycleBatch();
  const retiredDefinition = selectedSchema?.definitions.find((item) => item.identity === definition.identity);
  if (retiredDefinition?.lifecycle !== "retired") {
    throw new Error("schema retirement did not publish the requested lifecycle state");
  }

  await createSmokeEntityType(activeSessionId, "ipc_smoke_entity_available");
  const deprecatedType = await createSmokeEntityType(activeSessionId, "ipc_smoke_entity_deprecated");
  const current = await invokeSchemaFor(activeSessionId, { mode: "current" });
  await manageSchema({
    command: "set_lifecycle",
    expected_base_revision: current.revision,
    family: "entity_type",
    identity: deprecatedType.identity,
    lifecycle: "deprecated",
  });
}

async function createSmokeEntityType(activeSessionId, symbol) {
  const current = await invokeSchemaFor(activeSessionId, { mode: "current" });
  const result = await manageSchema({
    command: "create",
    expected_base_revision: current.revision,
    definition: {
      family: "entity_type",
      symbol,
      description: "Entity catalog smoke verification type.",
    },
  }, activeSessionId);
  if (result.kind !== "published") throw new Error(`schema did not publish ${symbol}`);
  const published = await invokeSchemaFor(activeSessionId, { mode: "current" });
  const definition = published.definitions.find((item) => item.symbol === symbol);
  if (!definition || definition.lifecycle !== "active") throw new Error(`schema did not expose active ${symbol}`);
  return definition;
}

async function runEntitySmoke(activeSessionId) {
  entityViewMode.value = "current";
  entityCurrentMode = true;
  await refreshEntities(activeSessionId);
  const beforeCreate = selectedEntities;
  const activeType = beforeCreate.entity_types.find((item) => item.symbol === "ipc_smoke_entity_available" && item.lifecycle === "active");
  const deprecatedType = beforeCreate.entity_types.find((item) => item.symbol === "ipc_smoke_entity_deprecated" && item.lifecycle === "deprecated");
  if (!activeType || !deprecatedType) throw new Error("Entity smoke EntityTypes were not published with the required lifecycles");

  entityTypeSelect.value = activeType.entity_type_id;
  entityAcceptDeprecated.checked = false;
  updateEntityTypeSelectionState();
  const created = await createEntity({ propagateErrors: true });
  if (created.kind !== "published" || !created.entity_id) throw new Error("active Entity creation did not publish");
  const activeCreationRevision = created.revision;
  entityViewMode.value = "historical";
  entityViewRevision.value = String(beforeCreate.revision);
  await refreshEntities(activeSessionId);
  const historicalBefore = selectedEntities;
  if (historicalBefore.entities.some((item) => item.entity_id === created.entity_id)
    || !entityCreateButton.disabled
    || entityList.querySelector("button[data-entity-retire]")) {
    throw new Error("historical Entity view exposed a later Entity or live mutation control");
  }
  entityViewMode.value = "explicit";
  entityViewRevision.value = String(activeCreationRevision);
  await refreshEntities(activeSessionId);
  const explicitCreation = selectedEntities;
  if (!explicitCreation.entities.some((item) => item.entity_id === created.entity_id)) {
    throw new Error("Entity creation was not visible at the correct historical/explicit revisions");
  }

  entityViewMode.value = "current";
  entityCurrentMode = true;
  await refreshEntities(activeSessionId);
  entityTypeSelect.value = deprecatedType.entity_type_id;
  entityAcceptDeprecated.checked = true;
  updateEntityTypeSelectionState();
  if (entityDeprecatedOptIn.hidden || entityDeprecatedWarning.hidden || entityCreateButton.disabled) {
    throw new Error("Deprecated EntityType opt-in was not presented by the entity form");
  }
  const deprecatedCreated = await createEntity({ propagateErrors: true });
  if (deprecatedCreated.kind !== "published" || deprecatedCreated.warning?.code !== "deprecated_entity_type") {
    throw new Error("Deprecated EntityType opt-in did not return the typed warning");
  }

  const retired = await retireEntity(created.entity_id, { confirm: false, propagateErrors: true });
  if (retired.kind !== "published") throw new Error("Entity retirement did not publish");
  entityViewMode.value = "current";
  await refreshEntities(activeSessionId);
  const currentEntity = selectedEntities.entities.find((item) => item.entity_id === created.entity_id);
  entityViewMode.value = "historical";
  entityViewRevision.value = String(activeCreationRevision);
  await refreshEntities(activeSessionId);
  const historicalEntity = selectedEntities.entities.find((item) => item.entity_id === created.entity_id);
  entityViewMode.value = "explicit";
  entityViewRevision.value = String(retired.revision);
  await refreshEntities(activeSessionId);
  const explicitEntity = selectedEntities.entities.find((item) => item.entity_id === created.entity_id);
  if (currentEntity?.retired_revision !== retired.revision
    || historicalEntity?.retired_revision != null
    || explicitEntity?.retired_revision !== retired.revision) {
    throw new Error("Entity retirement did not preserve its append-only historical state");
  }
  entityViewMode.value = "current";
  entityCurrentMode = true;
  await refreshEntities(activeSessionId);
}

async function runBranchLayerSmoke(activeSessionId) {
  const initial = await manageBranchLayers({ command: "snapshot", mode: { mode: "current" } }, activeSessionId);
  if (initial.kind !== "snapshot" || initial.branches.length !== 1 || initial.layers.length !== 1) {
    throw new Error("project bootstrap did not publish one root branch and one base Layer");
  }
  const root = initial.branches.find((branch) => branch.parent_history_space_id == null);
  const base = initial.layers.find((layer) => layer.is_base);
  if (!root || !base) throw new Error("branch/layer bootstrap snapshot is incomplete");

  const child = await manageBranchLayers({
    command: "create_child",
    expected_base_revision: initial.revision,
    parent_history_space_id: root.history_space_id,
    base_revision: initial.revision,
  }, activeSessionId);
  if (child.kind !== "published") throw new Error("child branch did not publish");
  const afterChild = await manageBranchLayers({ command: "snapshot", mode: { mode: "current" } }, activeSessionId);
  if (afterChild.branches.length !== 2) throw new Error("child branch was not visible in the current tree");
  const childBranch = afterChild.branches.find((branch) => branch.parent_history_space_id === root.history_space_id);
  if (!childBranch || childBranch.base_revision !== initial.revision) {
    throw new Error("child branch did not keep its selected fixed parent cutoff");
  }
  const historicalBefore = await manageBranchLayers({
    command: "snapshot",
    mode: { mode: "explicit", revision: initial.revision },
  }, activeSessionId);
  if (historicalBefore.branches.length !== 1) throw new Error("historical tree included a later child branch");

  const createdLayer = await manageBranchLayers({
    command: "create_layer",
    expected_base_revision: afterChild.revision,
    symbol: "ipc_smoke_overlay",
    description: "Temporary overlay for branch/layer IPC verification.",
    precedence_rank: base.precedence_rank + 1,
  }, activeSessionId);
  if (createdLayer.kind !== "published") throw new Error("overlay Layer did not publish");
  const afterLayer = await manageBranchLayers({ command: "snapshot", mode: { mode: "current" } }, activeSessionId);
  const overlay = afterLayer.layers.find((layer) => layer.symbol === "ipc_smoke_overlay");
  if (!overlay || overlay.is_base) throw new Error("created overlay Layer is missing or became the base unexpectedly");

  const switched = await manageBranchLayers({
    command: "revise_layer",
    expected_base_revision: afterLayer.revision,
    layer_id: overlay.layer_id,
    description: overlay.description,
    precedence_rank: overlay.precedence_rank,
    lifecycle: "active",
    base_layer_id: overlay.layer_id,
  }, activeSessionId);
  if (switched.kind !== "published") throw new Error("base Layer switch did not publish");
  const current = await manageBranchLayers({ command: "snapshot", mode: { mode: "current" } }, activeSessionId);
  if (current.base_layer_id !== overlay.layer_id
    || !current.layers.find((layer) => layer.layer_id === overlay.layer_id)?.is_base) {
    throw new Error("base Layer switch did not update the current schema snapshot");
  }
  const beforeSwitch = await manageBranchLayers({
    command: "snapshot",
    mode: { mode: "explicit", revision: createdLayer.revision },
  }, activeSessionId);
  if (beforeSwitch.base_layer_id !== base.layer_id) {
    throw new Error("historical Layer selection did not preserve the prior base designation");
  }
  const stale = await manageBranchLayers({
    command: "create_child",
    expected_base_revision: initial.revision,
    parent_history_space_id: root.history_space_id,
    base_revision: initial.revision,
  }, activeSessionId).then(() => false, () => true);
  if (!stale) throw new Error("stale branch update was not rejected");
  const unchanged = await manageBranchLayers({ command: "snapshot", mode: { mode: "current" } }, activeSessionId);
  if (unchanged.revision !== current.revision || unchanged.branches.length !== 2) {
    throw new Error("rejected stale branch update changed persisted project state");
  }
  branchLayerViewMode.value = "current";
  await refreshBranchLayers(activeSessionId);

  const transferCatalog = await manageHistorySpaceTransfer({
    command: "list",
    source_history_space_id: root.history_space_id,
    target_history_space_id: childBranch.history_space_id,
    source_recorded_as_of: current.revision,
  }, activeSessionId);
  if (transferCatalog.kind !== "catalog"
    || !Array.isArray(transferCatalog.records)
    || !Array.isArray(transferCatalog.event_relations)
    || !Number.isSafeInteger(transferCatalog.current_revision)) {
    throw new Error("HistorySpace transfer catalog did not return a pinned typed inventory");
  }
}

async function runPerspectiveSmoke(activeSessionId) {
  const baseline = await invokePerspectiveSnapshot(activeSessionId, { mode: "current" });
  const created = await invokePerspectivesFor(activeSessionId, {
    command: "create",
    expected_base_revision: baseline.revision,
    display_name: "IPC-Prüfung Stadtwache",
    description: "Temporäre Perspektive für die authentisierte Desktopprüfung.",
  });
  if (created.kind !== "published" || !created.perspective_id) {
    throw new Error("Perspective creation did not publish its private identity");
  }
  const id = created.perspective_id;
  const beforeCreation = await invokePerspectiveSnapshot(activeSessionId, {
    mode: "historical", recorded_as_of: baseline.revision,
  });
  if (beforeCreation.perspectives.some((item) => item.perspective_id === id)) {
    throw new Error("historical Perspective snapshot included a later definition");
  }
  const atCreation = await invokePerspectiveSnapshot(activeSessionId, {
    mode: "explicit", revision: created.revision,
  });
  if (!atCreation.perspectives.some((item) => item.perspective_id === id)) {
    throw new Error("explicit Perspective snapshot omitted the published definition");
  }

  const bound = await invokePerspectivesFor(activeSessionId, {
    command: "validate_context",
    epistemic_mode: "knows",
    perspective_id: id,
  });
  if (bound.kind !== "context_bound" || !bound.perspective_label) {
    throw new Error("active Perspective did not bind a Knows context");
  }
  const worldBound = await invokePerspectivesFor(activeSessionId, {
    command: "validate_context",
    epistemic_mode: "world_state",
    perspective_id: null,
  });
  if (worldBound.kind !== "context_bound" || worldBound.perspective_label != null) {
    throw new Error("WorldState context was not kept Perspective-free");
  }
  const invalidWorldState = await invokePerspectivesFor(activeSessionId, {
    command: "validate_context",
    epistemic_mode: "world_state",
    perspective_id: id,
  }).then(() => false, () => true);
  const missingPerspective = await invokePerspectivesFor(activeSessionId, {
    command: "validate_context",
    epistemic_mode: "believes",
    perspective_id: null,
  }).then(() => false, () => true);
  if (!invalidWorldState || !missingPerspective) {
    throw new Error("invalid epistemic mode and Perspective pair was accepted");
  }

  inputContextMode.value = "knows";
  inputContextPerspective.value = id;
  queryContextMode.value = "world_state";
  queryContextPerspective.value = "";
  updatePerspectiveControls();
  if (inputContextMode.value !== "knows"
    || inputContextPerspective.value !== id
    || queryContextMode.value !== "world_state"
    || queryContextPerspective.value !== ""
    || !queryContextPerspective.disabled) {
    throw new Error("input and query context selections were not kept independent");
  }

  const updated = await invokePerspectivesFor(activeSessionId, {
    command: "update",
    expected_base_revision: created.revision,
    perspective_id: id,
    display_name: "IPC-Prüfung Wache",
    description: "Aktualisierte temporäre Perspektive.",
  });
  if (updated.kind !== "published") throw new Error("Perspective metadata update did not publish");
  const beforeUpdate = await invokePerspectiveSnapshot(activeSessionId, {
    mode: "explicit", revision: created.revision,
  });
  if (beforeUpdate.perspectives.find((item) => item.perspective_id === id)?.display_name !== "IPC-Prüfung Stadtwache") {
    throw new Error("Perspective update changed an earlier catalog revision");
  }
  const retired = await invokePerspectivesFor(activeSessionId, {
    command: "retire",
    expected_base_revision: updated.revision,
    perspective_id: id,
  });
  if (retired.kind !== "published") throw new Error("Perspective retirement did not publish");
  const retiredCannotBind = await invokePerspectivesFor(activeSessionId, {
    command: "validate_context",
    epistemic_mode: "claims",
    perspective_id: id,
  }).then(() => false, () => true);
  if (!retiredCannotBind) throw new Error("retired Perspective remained available to a new context");
  const atRetirement = await invokePerspectiveSnapshot(activeSessionId, {
    mode: "explicit", revision: retired.revision,
  });
  if (atRetirement.perspectives.find((item) => item.perspective_id === id)?.retired_revision !== retired.revision) {
    throw new Error("Perspective retirement was not retained in catalog history");
  }
  perspectiveViewMode.value = "current";
  await refreshPerspectives(activeSessionId);
}

async function runSecurityPolicySmoke(activeSessionId) {
  securityPolicyBusy = true;
  updateSchemaControls();
  try {
    const baseline = await invokeSecurityPolicyFor(activeSessionId, { command: "snapshot" });
    if (baseline.kind !== "snapshot") throw new Error("policy snapshot was not returned");
    const gm = baseline.roles.find((item) => item.symbol === "gm");
    const player = baseline.roles.find((item) => item.symbol === "player");
    if (!gm || !player) throw new Error("project bootstrap omitted the GM or Player role");
    if (gm.bundle.some((item) => item.capability === "admin_raw_read" && item.effect === "allow")) {
      throw new Error("GM base bundle implicitly grants AdminRawRead");
    }
    const actor = baseline.principals.find((item) => item.state === "active");
    if (!actor) throw new Error("project bootstrap omitted the active creator principal");

    const assigned = await invokeSecurityPolicyFor(activeSessionId, {
      command: "assign_role",
      expected_base_revision: baseline.revision,
      principal_id: actor.principal_id,
      role_id: player.role_id,
    });
    if (assigned.kind !== "published" || assigned.security_epoch !== baseline.security_epoch + 1) {
      throw new Error("role assignment did not advance SecurityEpoch exactly once");
    }
    const afterAssignment = await invokeSecurityPolicyFor(activeSessionId, { command: "snapshot" });
    if (!afterAssignment.assignments.some((item) => item.principal_id === actor.principal_id && item.role_id === player.role_id)) {
      throw new Error("published role assignment is missing from the next policy snapshot");
    }

    const revokedAssignment = await invokeSecurityPolicyFor(activeSessionId, {
      command: "revoke_role_assignment",
      expected_base_revision: assigned.revision,
      assignment_id: afterAssignment.assignments.find((item) => item.principal_id === actor.principal_id && item.role_id === player.role_id).assignment_id,
    });
    if (revokedAssignment.kind !== "published" || revokedAssignment.security_epoch !== assigned.security_epoch + 1) {
      throw new Error("role assignment revocation did not advance SecurityEpoch exactly once");
    }

    const denyRule = await invokeSecurityPolicyFor(activeSessionId, {
      command: "add_capability_rule",
      expected_base_revision: revokedAssignment.revision,
      subject_kind: "role",
      subject_id: gm.role_id,
      capability: "admin_raw_read",
      effect: "deny",
    });
    if (denyRule.kind !== "published" || denyRule.security_epoch !== revokedAssignment.security_epoch + 1) {
      throw new Error("explicit capability rule did not advance SecurityEpoch exactly once");
    }
    const afterRule = await invokeSecurityPolicyFor(activeSessionId, { command: "snapshot" });
    const explicitRule = afterRule.explicit_rules.find((item) => item.subject_kind === "role"
      && item.subject_id === gm.role_id && item.capability === "admin_raw_read" && item.effect === "deny");
    if (!explicitRule) throw new Error("explicit GM AdminRawRead deny was not published as a separate rule");
    const revokedRule = await invokeSecurityPolicyFor(activeSessionId, {
      command: "revoke_capability_rule",
      expected_base_revision: denyRule.revision,
      rule_id: explicitRule.rule_id,
    });
    if (revokedRule.kind !== "published" || revokedRule.security_epoch !== denyRule.security_epoch + 1) {
      throw new Error("capability-rule revocation did not advance SecurityEpoch exactly once");
    }
  } finally {
    securityPolicyBusy = false;
    updateSchemaControls();
    await refreshSecurityPolicy(activeSessionId).catch(() => {});
  }
}

async function createEntity({ propagateErrors = false } = {}) {
  if (!sessionId || !projectOpen || !entityCurrentMode || entityBusy) return;
  entityBusy = true;
  updateEntityControls();
  entityStatus.textContent = "Entität wird geprüft und angelegt …";
  try {
    const latest = await invokeEntitiesFor(sessionId, { mode: "current" });
    const selected = latest.entity_types.find((item) => item.entity_type_id === entityTypeSelect.value);
    if (!selected || selected.lifecycle === "retired") throw new Error("invalid_request");
    const acceptDeprecated = selected.lifecycle === "deprecated" && entityAcceptDeprecated.checked;
    const result = await manageEntities({
      command: "create",
      expected_base_revision: latest.revision,
      entity_type_id: selected.entity_type_id,
      accept_deprecated_type: acceptDeprecated,
    });
    if (result.kind !== "published") throw new Error("unsupported_protocol");
    entityStatus.textContent = result.warning?.message
      ?? `Entität veröffentlicht. Aktuelle Datenrevision ${result.revision}.`;
    await refreshEntities();
    return result;
  } catch (error) {
    entityStatus.textContent = showError(error);
    if (propagateErrors) throw error;
  } finally {
    entityBusy = false;
    updateEntityControls();
  }
}

async function retireEntity(entityId, { confirm = true, propagateErrors = false } = {}) {
  if (!sessionId || !projectOpen || !entityCurrentMode || entityBusy) return;
  if (confirm && !window.confirm("Diese Entität endgültig stilllegen? Das kann nicht rückgängig gemacht werden. Vorhandene Aussagen bleiben erhalten.")) return;
  entityBusy = true;
  updateEntityControls();
  entityStatus.textContent = "Entität wird stillgelegt …";
  try {
    const latest = await invokeEntitiesFor(sessionId, { mode: "current" });
    const selected = latest.entities.find((item) => item.entity_id === entityId && item.retired_revision == null);
    if (!selected) throw new Error("invalid_request");
    const result = await manageEntities({
      command: "retire",
      expected_base_revision: latest.revision,
      entity_id: selected.entity_id,
    });
    if (result.kind !== "published") throw new Error("unsupported_protocol");
    entityStatus.textContent = `Entität stillgelegt. Datenrevision ${result.revision}.`;
    await refreshEntities();
    return result;
  } catch (error) {
    entityStatus.textContent = showError(error);
    if (propagateErrors) throw error;
  } finally {
    entityBusy = false;
    updateEntityControls();
  }
}

function appendText(parent, tagName, value, className) {
  const element = document.createElement(tagName);
  element.textContent = value;
  if (className) element.className = className;
  parent.append(element);
  return element;
}

function lifecycleLabel(value) {
  return ({ active: "Aktiv", deprecated: "Veraltet", retired: "Stillgelegt" })[value] ?? value;
}

function familyLabel(value) {
  return ({ entity_type: "EntityType", predicate: "Prädikat", event_kind: "Ereignistyp", layer: "Layer", layer_snapshot: "Layer-Stand" })[value] ?? value;
}

function entityConstraintText(value) {
  return value === "any_entity" ? "beliebiger EntityType" : value.replace(/^entity_type:/, "EntityType ");
}

function renderSchema(snapshot) {
  schemaStatus.textContent = `Schema-Revision ${snapshot.revision} · ${snapshot.definitions.length} Definition(en)`;
  schemaDefinitions.replaceChildren();
  if (snapshot.definitions.length === 0) {
    appendText(schemaDefinitions, "p", "In diesem Stand sind noch keine Schema-Definitionen vorhanden.", "muted");
    renderStagedLifecycleChanges();
    return;
  }
  for (const definition of snapshot.definitions) {
    const card = document.createElement("article");
    card.className = "definition";
    appendText(card, "h3", `${familyLabel(definition.family)} · ${definition.symbol}`);
    appendText(card, "p", `${lifecycleLabel(definition.lifecycle)} · ab Revision ${definition.created_revision}`);
    appendText(card, "p", definition.lifecycle_help, "muted");
    if (definition.description) appendText(card, "p", definition.description);
    for (const detail of describeDefinition(definition)) appendText(card, "p", detail, "muted");
    const lifecycleManagedFamily = ["entity_type", "predicate", "event_kind"].includes(definition.family);
    const nextLifecycle = lifecycleManagedFamily
      ? definition.lifecycle === "active" ? "deprecated" : definition.lifecycle === "deprecated" ? "retired" : null
      : null;
    if (nextLifecycle && schemaCurrentMode) {
      const button = document.createElement("button");
      button.type = "button";
      button.dataset.lifecycle = "true";
      const alreadyStaged = stagedLifecycleChanges.some((item) => item.family === definition.family && item.identity === definition.identity);
      button.textContent = alreadyStaged ? "Änderung vorgemerkt" : nextLifecycle === "deprecated" ? "Als veraltet vormerken" : "Stilllegung vormerken";
      button.disabled = !projectOpen || schemaBusy || alreadyStaged;
      button.addEventListener("click", () => stageDefinitionLifecycle(definition, nextLifecycle));
      card.append(button);
    }
    schemaDefinitions.append(card);
  }
  renderStagedLifecycleChanges();
}

function describeDefinition(definition) {
  const details = definition.details ?? {};
  if (definition.family === "entity_type") return [];
  if (definition.family === "predicate") {
    const parts = [
      `Subjekt: ${entityConstraintText(details.subject_constraint)}`,
      `Wertart: ${valueKindLabel(details.value_kind)}`,
      `Anzahl: ${details.cardinality === "single" ? "ein Wert" : "mehrere Werte"}`,
      `Gleichrangige Werte: ${resolutionLabel(details.resolution_policy)}`,
    ];
    if (details.object_constraint) parts.push(`Ziel: ${entityConstraintText(details.object_constraint)}`);
    if (details.constraints?.length) parts.push(`Beschränkung: ${details.constraints.join("; ")}`);
    const metadata = details.decimal_metadata;
    if (metadata) parts.push(`Dezimalstellen: ${Object.entries(metadata).filter(([, value]) => value != null).map(([key, value]) => `${metadataLabel(key)} ${value}`).join(", ")}`);
    return parts;
  }
  if (definition.family === "event_kind") {
    const parts = [];
    for (const item of details.roles ?? []) {
      const maximum = item.max_participants == null ? "unbegrenzt" : item.max_participants;
      parts.push(`Rolle ${item.symbol}: ${entityConstraintText(item.entity_constraint)}, ${item.min_participants}–${maximum} Teilnehmende`);
    }
    for (const item of details.attributes ?? []) {
      const target = item.object_constraint ? `, Ziel ${entityConstraintText(item.object_constraint)}` : "";
      const required = item.required ? "verpflichtend" : "optional";
      const constraints = item.constraints?.length ? `, ${item.constraints.join("; ")}` : "";
      parts.push(`Attribut ${item.symbol}: ${valueKindLabel(item.value_kind)}${target}, ${required}${constraints}`);
    }
    const time = details.event_time ?? {};
    const timeLabels = { instant_only: "nur Zeitpunkt", span_only: "Zeitspanne", instant_or_span: "Zeitpunkt oder abgeschlossene Zeitspanne", open_span_allowed: "auch offene Zeitspanne" };
    parts.push(`Ereigniszeit: ${timeLabels[time.form] ?? time.form}`);
    if (time.max_calendar_span) {
      const span = time.max_calendar_span;
      parts.push(`Maximale Kalenderdauer: ${span.years} Jahre, ${span.months} Monate, ${span.days} Tage`);
    }
    return parts;
  }
  return [];
}

function valueKindLabel(value) {
  return ({ bool: "Boolesch", int: "ganze Zahl", uint: "positive Zahl", decimal: "Dezimalzahl", string: "Text", symbol: "Symbol", entity: "Entity-Verweis", time: "Zeitpunkt", duration: "Zeitdauer", bytes: "Binärdaten" })[value] ?? value;
}

function resolutionLabel(value) {
  return ({ single_value_replace: "Einzelwert ersetzen", multi_value_overlay: "Mehrere Werte überlagern", multi_value_replace: "Wertemenge ersetzen" })[value] ?? value;
}

function metadataLabel(value) {
  return ({ display_precision: "Anzeige", measurement_precision: "Messgenauigkeit", currency_scale: "Währung" })[value] ?? value;
}

function entityTypeDefinitions() {
  return currentSchema?.definitions?.filter((definition) => definition.family === "entity_type" && definition.lifecycle !== "retired") ?? [];
}

function fillEntityTypeSelect(select, allowAny) {
  const previous = select.value;
  select.replaceChildren();
  if (allowAny) {
    const option = document.createElement("option");
    option.value = "any";
    option.textContent = "Beliebiger EntityType";
    select.append(option);
  }
  for (const definition of entityTypeDefinitions()) {
    const option = document.createElement("option");
    option.value = definition.identity;
    option.textContent = `${definition.symbol}${definition.lifecycle === "deprecated" ? " (veraltet)" : ""}`;
    select.append(option);
  }
  if ([...select.options].some((option) => option.value === previous)) select.value = previous;
  else if (select.options.length) select.selectedIndex = 0;
}

function updateEntityTypeChoices() {
  fillEntityTypeSelect(schemaSubjectType, true);
  fillEntityTypeSelect(schemaObjectType, false);
  fillEntityTypeSelect(eventRoleType, true);
  fillEntityTypeSelect(eventAttributeType, false);
}

function entityConstraintFrom(select) {
  return select.value === "any" ? { kind: "any_entity" } : { kind: "exact", entity_type_id: select.value };
}

function updateSchemaFormVisibility() {
  const family = schemaFamily.value;
  schemaEntityTypeFields.hidden = family !== "entity_type";
  schemaPredicateFields.hidden = family !== "predicate";
  schemaEventKindFields.hidden = family !== "event_kind";
  schemaObjectTypeWrap.hidden = schemaValueKind.value !== "entity";
  schemaDecimalMetadata.hidden = schemaValueKind.value !== "decimal";
  eventAttributeTypeWrap.hidden = eventAttributeKind.value !== "entity";
  eventAttributeDecimalMetadata.hidden = eventAttributeKind.value !== "decimal";
  eventMaxSpan.hidden = !eventMaxSpanEnabled.checked;
  const multi = schemaCardinality.value === "multi";
  for (const option of schemaResolution.options) {
    option.hidden = multi ? option.value === "single_value_replace" : option.value !== "single_value_replace";
  }
  const validResolution = [...schemaResolution.options].find((option) => !option.hidden);
  if (validResolution && schemaResolution.selectedOptions[0]?.hidden) schemaResolution.value = validResolution.value;
  configureConstraintSelector(schemaConstraintKind, schemaConstraintFields, schemaValueKind.value, "predicate_constraint");
  configureConstraintSelector(eventAttributeConstraintKind, eventAttributeConstraintFields, eventAttributeKind.value, "event_attribute_constraint");
}

const constraintDescriptions = {
  bool: [["bool_set", "Zulässige Wahrheitswerte"]],
  int: [["int_range", "Ganzzahlbereich"]],
  uint: [["uint_range", "Nichtnegativer Zahlenbereich"]],
  decimal: [["decimal_range", "Dezimalbereich"]],
  string: [["string_byte_length", "Textlänge in Bytes"]],
  symbol: [["symbol_set", "Zulässige Symbole"]],
  time: [["time_range", "Zeitbereich"]],
  duration: [["duration_range", "Dauerbereich in Nanosekunden"]],
  bytes: [["bytes_length", "Datenlänge in Bytes"]],
};

function configureConstraintSelector(select, fields, valueKind, prefix) {
  if (!select || !fields) return;
  const oldValue = select.value;
  select.replaceChildren();
  const none = document.createElement("option");
  none.value = "none";
  none.textContent = "Keine";
  select.append(none);
  for (const [value, label] of constraintDescriptions[valueKind] ?? []) {
    const option = document.createElement("option");
    option.value = value;
    option.textContent = label;
    select.append(option);
  }
  if ([...select.options].some((option) => option.value === oldValue)) select.value = oldValue;
  renderConstraintInputs(fields, select.value, prefix);
  select.onchange = () => renderConstraintInputs(fields, select.value, prefix);
}

function renderConstraintInputs(fields, kind, prefix) {
  fields.replaceChildren();
  const addInput = (name, label, type = "text", placeholder = "Optional") => {
    const wrapper = document.createElement("div");
    const caption = document.createElement("label");
    caption.textContent = label;
    const input = document.createElement("input");
    input.type = type;
    input.dataset.field = name;
    input.id = `${prefix}-${name}`;
    input.placeholder = placeholder;
    caption.htmlFor = input.id;
    wrapper.append(caption, input);
    fields.append(wrapper);
  };
  if (kind === "bool_set") {
    for (const [value, label] of [["false", "Falsch zulassen"], ["true", "Wahr zulassen"]]) {
      const wrapper = document.createElement("div");
      wrapper.className = "inline";
      const input = document.createElement("input");
      input.type = "checkbox";
      input.dataset.field = `value-${value}`;
      input.id = `${prefix}-value-${value}`;
      input.checked = true;
      const caption = document.createElement("label");
      caption.htmlFor = input.id;
      caption.textContent = label;
      wrapper.append(input, caption);
      fields.append(wrapper);
    }
  } else if (["int_range", "uint_range", "decimal_range", "string_byte_length", "duration_range", "bytes_length"].includes(kind)) {
    const numeric = kind === "int_range" ? "Ganzzahl" : kind === "uint_range" ? "Nichtnegative Zahl" : kind === "decimal_range" ? "Dezimalzahl" : kind === "duration_range" ? "Nanosekunden" : "Bytes";
    const minName = kind === "duration_range" ? "min_ns" : "min";
    const maxName = kind === "duration_range" ? "max_ns" : "max";
    addInput(minName, `Minimum ${numeric.toLowerCase()}`);
    addInput(maxName, `Maximum ${numeric.toLowerCase()}`);
  } else if (kind === "symbol_set") {
    const wrapper = document.createElement("div");
    const caption = document.createElement("label");
    caption.textContent = "Zulässige Symbole (je Zeile ein Symbol)";
    const textarea = document.createElement("textarea");
    textarea.dataset.field = "values";
    textarea.rows = 3;
    wrapper.append(caption, textarea);
    fields.append(wrapper);
  } else if (kind === "time_range") {
    addInput("timeline_id", "Timeline-ID");
    addInput("min_ticks", "Früheste Ticks");
    addInput("max_ticks", "Späteste Ticks");
    addInput("unit", "Zeiteinheit-Symbol");
  }
}

function constraintDraft(select, fields) {
  const kind = select.value;
  if (kind === "none") return null;
  const value = (name) => fields.querySelector(`[data-field="${name}"]`)?.value.trim() ?? "";
  const nullable = (name) => value(name) || null;
  if (kind === "bool_set") {
    const values = ["false", "true"].filter((item) => fields.querySelector(`[data-field="value-${item}"]`)?.checked).map((item) => item === "true");
    if (!values.length) throw new Error("invalid_request");
    return { kind, values };
  }
  if (["int_range", "uint_range", "decimal_range", "string_byte_length", "bytes_length"].includes(kind)) {
    return { kind, min: nullable("min"), max: nullable("max") };
  }
  if (kind === "duration_range") return { kind, min_ns: nullable("min_ns"), max_ns: nullable("max_ns") };
  if (kind === "symbol_set") {
    const values = value("values").split(/\r?\n/).map((item) => item.trim()).filter(Boolean);
    if (!values.length) throw new Error("invalid_request");
    return { kind, values };
  }
  if (kind === "time_range") {
    const timelineId = value("timeline_id");
    const unit = value("unit");
    const minTicks = value("min_ticks");
    const maxTicks = value("max_ticks");
    if ((!minTicks && !maxTicks) || !timelineId || !unit) throw new Error("invalid_request");
    const bound = (ticks) => ticks ? { timeline_id: timelineId, ticks, unit } : null;
    return { kind, min: bound(minTicks), max: bound(maxTicks) };
  }
  throw new Error("invalid_request");
}

function parseCount(input, required) {
  const text = input.value.trim();
  if (!text && !required) return null;
  if (!/^\d+$/.test(text)) throw new Error("invalid_request");
  const value = Number(text);
  if (!Number.isSafeInteger(value) || value > 4294967295) throw new Error("invalid_request");
  return value;
}

function validateSymbol(value) {
  const symbol = value.trim();
  if (!/^[a-z][a-z0-9_]*$/.test(symbol)) throw new Error("invalid_request");
  return symbol;
}

function decimalMetadataDraft(inputs = decimalMetadataInputs) {
  const metadata = {};
  for (const [key, input] of Object.entries(inputs)) {
    metadata[key] = input.value === "" ? null : parseCount(input, true);
  }
  return Object.values(metadata).some((value) => value != null) ? metadata : null;
}

function buildDefinitionDraft() {
  const symbol = validateSymbol(schemaSymbol.value);
  if (schemaFamily.value === "entity_type") {
    return { family: "entity_type", symbol, description: schemaDescription.value.trim() || null };
  }
  if (schemaFamily.value === "predicate") {
    const valueKind = schemaValueKind.value;
    const constraint = constraintDraft(schemaConstraintKind, schemaConstraintFields);
    const decimal = valueKind === "decimal" ? decimalMetadataDraft() : null;
    return {
      family: "predicate",
      symbol,
      subject_constraint: entityConstraintFrom(schemaSubjectType),
      value_kind: valueKind,
      object_constraint: valueKind === "entity" ? entityConstraintFrom(schemaObjectType) : null,
      cardinality: schemaCardinality.value,
      resolution_policy: schemaResolution.value,
      constraints: constraint ? [constraint] : [],
      decimal_metadata: decimal,
    };
  }
  const eventMaxCalendarSpan = eventMaxSpanEnabled.checked ? {
    years: parseCount(document.querySelector("#event-max-years"), true),
    months: parseCount(document.querySelector("#event-max-months"), true),
    days: parseCount(document.querySelector("#event-max-days"), true),
  } : null;
  return {
    family: "event_kind",
    symbol,
    roles: stagedEventRoles,
    attributes: stagedEventAttributes,
    event_time: { form: eventTimeForm.value, max_calendar_span: eventMaxCalendarSpan },
  };
}

function clearDefinitionForm() {
  schemaSymbol.value = "";
  schemaDescription.value = "";
  for (const input of Object.values(decimalMetadataInputs)) input.value = "";
  stagedEventRoles = [];
  stagedEventAttributes = [];
  renderStagedEventItems();
  for (const input of Object.values(eventAttributeDecimalMetadataInputs)) input.value = "";
  schemaConstraintKind.value = "none";
  updateSchemaFormVisibility();
}

async function publishDefinition() {
  if (!sessionId || !projectOpen || !schemaCurrentMode) return;
  schemaBusy = true;
  updateSchemaControls();
  schemaStatus.textContent = "Definition wird geprüft und veröffentlicht …";
  try {
    const latest = await invokeSchemaFor(sessionId, { mode: "current" });
    currentSchema = latest;
    const definition = buildDefinitionDraft();
    await manageSchema({ command: "create", expected_base_revision: latest.revision, definition });
    schemaViewMode.value = "current";
    schemaCurrentMode = true;
    clearDefinitionForm();
    await refreshSchema(sessionId);
    schemaStatus.textContent = `Definition veröffentlicht. Aktuelle Schema-Revision ${selectedSchema.revision}.`;
  } catch (error) {
    schemaStatus.textContent = showError(error);
  } finally {
    schemaBusy = false;
    updateSchemaControls();
  }
}

function stageDefinitionLifecycle(definition, lifecycle) {
  if (stagedLifecycleChanges.some((item) => item.family === definition.family && item.identity === definition.identity)) return;
  stagedLifecycleChanges.push({
    family: definition.family,
    identity: definition.identity,
    lifecycle,
    symbol: definition.symbol,
  });
  if (selectedSchema) renderSchema(selectedSchema);
  updateSchemaControls();
}

function renderStagedLifecycleChanges() {
  schemaLifecyclePending.replaceChildren();
  if (stagedLifecycleChanges.length === 0) {
    appendText(schemaLifecyclePending, "p", "Keine Änderungen vorgemerkt.", "muted");
    return;
  }
  for (const [index, item] of stagedLifecycleChanges.entries()) {
    const row = document.createElement("p");
    row.className = "inline";
    appendText(row, "span", `${familyLabel(item.family)} · ${item.symbol} → ${lifecycleLabel(item.lifecycle)}`);
    const remove = document.createElement("button");
    remove.type = "button";
    remove.textContent = "Entfernen";
    remove.disabled = schemaBusy;
    remove.addEventListener("click", () => {
      stagedLifecycleChanges.splice(index, 1);
      if (selectedSchema) renderSchema(selectedSchema);
      updateSchemaControls();
    });
    row.append(remove);
    schemaLifecyclePending.append(row);
  }
}

async function publishLifecycleBatch() {
  if (!sessionId || !projectOpen || !schemaCurrentMode || stagedLifecycleChanges.length === 0) return;
  schemaBusy = true;
  updateSchemaControls();
  schemaStatus.textContent = "Vorgemerkte Zustandsänderungen werden gemeinsam geprüft und veröffentlicht …";
  try {
    const latest = await invokeSchemaFor(sessionId, { mode: "current" });
    currentSchema = latest;
    await manageSchema({
      command: "set_lifecycle_batch",
      expected_base_revision: latest.revision,
      updates: stagedLifecycleChanges.map(({ family, identity, lifecycle }) => ({
        family,
        identity,
        lifecycle,
      })),
    });
    stagedLifecycleChanges = [];
    await refreshSchema(sessionId);
    schemaStatus.textContent = `Zustandsänderungen gemeinsam veröffentlicht. Aktuelle Schema-Revision ${selectedSchema.revision}.`;
  } catch (error) {
    schemaStatus.textContent = showError(error);
  } finally {
    schemaBusy = false;
    updateSchemaControls();
  }
}

function renderStagedEventItems() {
  eventRoleList.replaceChildren();
  if (stagedEventRoles.length === 0) appendText(eventRoleList, "p", "Noch keine Rollen hinzugefügt.", "muted");
  stagedEventRoles.forEach((item, index) => {
    const row = document.createElement("p");
    row.className = "inline";
    appendText(row, "span", `${item.symbol}: ${entityConstraintText(item.entity_constraint)}, ${item.min_participants}–${item.max_participants ?? "unbegrenzt"}`);
    const remove = document.createElement("button");
    remove.type = "button";
    remove.textContent = "Entfernen";
    remove.addEventListener("click", () => {
      stagedEventRoles.splice(index, 1);
      renderStagedEventItems();
    });
    row.append(remove);
    eventRoleList.append(row);
  });
  eventAttributeList.replaceChildren();
  if (stagedEventAttributes.length === 0) appendText(eventAttributeList, "p", "Noch keine Attribute hinzugefügt.", "muted");
  stagedEventAttributes.forEach((item, index) => {
    const row = document.createElement("p");
    row.className = "inline";
    appendText(row, "span", `${item.symbol}: ${valueKindLabel(item.value_kind)}${item.required ? " · verpflichtend" : " · optional"}`);
    const remove = document.createElement("button");
    remove.type = "button";
    remove.textContent = "Entfernen";
    remove.addEventListener("click", () => {
      stagedEventAttributes.splice(index, 1);
      renderStagedEventItems();
    });
    row.append(remove);
    eventAttributeList.append(row);
  });
}

function addEventRole() {
  const maximum = parseCount(eventRoleMax, false);
  const minimum = parseCount(eventRoleMin, true);
  if (maximum != null && maximum < minimum) throw new Error("invalid_request");
  stagedEventRoles.push({
    symbol: validateSymbol(eventRoleSymbol.value),
    entity_constraint: entityConstraintFrom(eventRoleType),
    min_participants: minimum,
    max_participants: maximum,
  });
  eventRoleSymbol.value = "";
  renderStagedEventItems();
}

function addEventAttribute() {
  const valueKind = eventAttributeKind.value;
  const constraint = constraintDraft(eventAttributeConstraintKind, eventAttributeConstraintFields);
  const decimal = valueKind === "decimal" ? decimalMetadataDraft(eventAttributeDecimalMetadataInputs) : null;
  stagedEventAttributes.push({
    symbol: validateSymbol(eventAttributeSymbol.value),
    value_kind: valueKind,
    object_constraint: valueKind === "entity" ? entityConstraintFrom(eventAttributeType) : null,
    constraints: constraint ? [constraint] : [],
    decimal_metadata: decimal,
    required: eventAttributeRequired.checked,
  });
  eventAttributeSymbol.value = "";
  eventAttributeRequired.checked = false;
  renderStagedEventItems();
}

if (!invoke || !["primary", "secondary"].includes(role)) {
  windowStatus.textContent = "Dieses Fenster kann den lokalen WorldDB-Host nicht verwenden.";
} else {
  invoke("open_host_session")
    .then(async (ticket) => {
      if (ticket.protocol_version !== 1) throw new Error("unsupported_protocol");
      sessionId = ticket.session_id;
      windowStatus.textContent = `Fenster ${role}: lokale Sitzung verbunden.`;

      const listen = window.__TAURI__?.event?.listen;
      if (listen) {
        await listen("project-state-changed", () => {
          refreshProject(sessionId).catch(() => {});
          refreshEntities(sessionId).catch((error) => {
            entityStatus.textContent = showError(error);
          });
          if (!securityPolicyBusy) refreshSecurityPolicy(sessionId).catch((error) => {
            securityPolicyStatus.textContent = showError(error);
          });
          if (!perspectiveBusy) refreshPerspectives(sessionId).catch((error) => {
            perspectiveStatus.textContent = showError(error);
          });
          if (!branchLayerBusy) refreshBranchLayers(sessionId).catch((error) => {
            branchLayerStatus.textContent = showError(error);
          });
          if (!transferBusy) refreshTransferCatalog(sessionId).catch((error) => {
            transferStatus.textContent = showError(error);
          });
        });
      }

      const [securityMode, projectMode] = await Promise.all([
        invoke("security_smoke_mode"),
        invoke("project_dialog_mode"),
      ]);
      if (securityMode.protocol_version !== 1 || projectMode.protocol_version !== 1) {
        throw new Error("unsupported_protocol");
      }
      if (securityMode.enabled) await runSecurityProbes(sessionId);

      await invoke("health", {
        request: { protocol_version: 1, session_id: sessionId },
      });
      await refreshProject(sessionId);

      if (projectMode.enabled) {
        operationStatus.textContent = "Zwei-Fenster-Projektprüfung läuft …";
        await runProjectSmoke(sessionId);
        if (role === "primary") {
          operationStatus.textContent = "Schema-, Entitäts-, Perspektiven-, Rechte-, Branch- und Layerprüfung läuft …";
          await runSchemaSmoke(sessionId);
          await runEntitySmoke(sessionId);
          await runBranchLayerSmoke(sessionId);
          await runPerspectiveSmoke(sessionId);
          await runSecurityPolicySmoke(sessionId);
        }
        operationStatus.textContent = "Projektprüfung abgeschlossen.";
        await refreshProject(sessionId);
      }

      window.setInterval(() => refreshProject(sessionId).catch(() => {}), 1200);
    })
    .catch((error) => {
      windowStatus.textContent = "Die lokale Host-Sitzung konnte nicht eingerichtet werden.";
      operationStatus.textContent = showError(error);
    });

  createButton.addEventListener("click", async () => {
    if (!sessionId) return;
    setBusy(true);
    operationStatus.textContent = "Projekt wird angelegt …";
    try {
      await invoke("create_project", {
        sessionId,
        request: { protocol_version: 1, project_name: projectName.value },
      });
      operationStatus.textContent = "Projekt wurde angelegt.";
      await refreshProject(sessionId);
    } catch (error) {
      operationStatus.textContent = showError(error);
    } finally {
      setBusy(false);
      await refreshProject(sessionId).catch(() => {});
    }
  });

  openButton.addEventListener("click", async () => {
    if (!sessionId) return;
    setBusy(true);
    operationStatus.textContent = "Projektordner wird ausgewählt …";
    try {
      await invoke("open_project", { sessionId, request: { protocol_version: 1 } });
      operationStatus.textContent = "Projekt wurde geöffnet.";
      await refreshProject(sessionId);
    } catch (error) {
      operationStatus.textContent = showError(error);
    } finally {
      setBusy(false);
      await refreshProject(sessionId).catch(() => {});
    }
  });

  closeButton.addEventListener("click", async () => {
    if (!sessionId) return;
    setBusy(true);
    operationStatus.textContent = "Projekt wird geschlossen …";
    try {
      await invoke("close_project", { sessionId, request: { protocol_version: 1 } });
      operationStatus.textContent = "Projekt wurde geschlossen.";
    } catch (error) {
      operationStatus.textContent = showError(error);
    } finally {
      setBusy(false);
      await refreshProject(sessionId).catch(() => {});
    }
  });

  schemaRefreshButton.addEventListener("click", () => refreshSchema().catch(() => {}));
  schemaCreateButton.addEventListener("click", publishDefinition);
  schemaLifecyclePublish.addEventListener("click", publishLifecycleBatch);
  schemaViewMode.addEventListener("change", () => {
    schemaViewRevisionWrap.hidden = schemaViewMode.value === "current";
    schemaViewRevisionLabel.textContent = schemaViewMode.value === "historical" ? "Datenrevision (RecordedAsOf)" : "Schematrevision";
    schemaCurrentMode = schemaViewMode.value === "current";
    updateSchemaControls();
    if (projectOpen) refreshSchema().catch(() => {});
  });
  schemaViewRevision.addEventListener("change", () => refreshSchema().catch(() => {}));
  entityRefreshButton.addEventListener("click", () => refreshEntities().catch(() => {}));
  entityViewMode.addEventListener("change", () => {
    entityViewRevisionWrap.hidden = entityViewMode.value === "current";
    entityViewRevisionLabel.textContent = entityViewMode.value === "historical"
      ? "Datenrevision (RecordedAsOf)"
      : "Datenrevision";
    entityCurrentMode = entityViewMode.value === "current";
    updateEntityControls();
    if (projectOpen) refreshEntities().catch(() => {});
  });
  entityViewRevision.addEventListener("change", () => refreshEntities().catch(() => {}));
  entityTypeSelect.addEventListener("change", updateEntityTypeSelectionState);
  entityAcceptDeprecated.addEventListener("change", updateEntityControls);
  entityCreateButton.addEventListener("click", createEntity);
  securityPolicyRefresh.addEventListener("click", () => refreshSecurityPolicy().catch(() => {}));
  policyPrincipalSelect.addEventListener("change", () => {
    const selected = currentSecurityPolicy?.principals.find((item) => item.principal_id === policyPrincipalSelect.value);
    policyPrincipalState.value = selected?.state ?? "active";
    updateSecurityPolicyControls();
  });
  policyPrincipalState.addEventListener("change", updateSecurityPolicyControls);
  policyPrincipalSave.addEventListener("click", () => savePrincipalState());
  policyRoleAssign.addEventListener("click", () => assignPolicyRole());
  policyNewRoleSymbol.addEventListener("input", updateSecurityPolicyControls);
  policyRoleCreate.addEventListener("click", () => createPolicyRole());
  policyRuleAdd.addEventListener("click", () => addPolicyRule());
  policyRuleSubject.addEventListener("change", updateSecurityPolicyControls);
  policyRuleCapability.addEventListener("change", updateSecurityPolicyControls);
  policyRuleEffect.addEventListener("change", updateSecurityPolicyControls);
  perspectiveRefreshButton.addEventListener("click", () => refreshPerspectives().catch(() => {}));
  perspectiveViewMode.addEventListener("change", () => {
    perspectiveViewRevisionWrap.hidden = perspectiveViewMode.value === "current";
    perspectiveViewRevisionLabel.textContent = perspectiveViewMode.value === "historical"
      ? "Datenrevision (RecordedAsOf)"
      : "Katalogrevision";
    updatePerspectiveControls();
    if (projectOpen) refreshPerspectives().catch(() => {});
  });
  perspectiveViewRevision.addEventListener("change", () => refreshPerspectives().catch(() => {}));
  perspectiveSelect.addEventListener("change", () => {
    const selected = currentPerspectives?.perspectives.find((item) => item.perspective_id === perspectiveSelect.value);
    perspectiveName.value = selected?.display_name ?? "";
    perspectiveDescription.value = selected?.description ?? "";
    updatePerspectiveControls();
  });
  perspectiveCreateButton.addEventListener("click", createPerspective);
  perspectiveUpdateButton.addEventListener("click", updatePerspective);
  perspectiveRetireButton.addEventListener("click", retirePerspective);
  for (const [mode, select, status] of [
    [inputContextMode, inputContextPerspective, inputContextStatus],
    [queryContextMode, queryContextPerspective, queryContextStatus],
  ]) {
    mode.addEventListener("change", () => {
      if (mode.value === "world_state") select.value = "";
      status.textContent = "Kontext noch nicht geprüft.";
      updatePerspectiveControls();
    });
    select.addEventListener("change", () => {
      status.textContent = "Kontext noch nicht geprüft.";
      updatePerspectiveControls();
    });
  }
  inputContextValidate.addEventListener("click", () => validateContext(
    inputContextMode, inputContextPerspective, inputContextStatus, "Eingabe-Kontext",
  ));
  queryContextValidate.addEventListener("click", () => validateContext(
    queryContextMode, queryContextPerspective, queryContextStatus, "Abfrage-Kontext",
  ));
  branchLayerRefreshButton.addEventListener("click", () => refreshBranchLayers().catch(() => {}));
  transferLoadButton.addEventListener("click", reloadTransferContents);
  transferSource.addEventListener("change", reloadTransferContents);
  transferTarget.addEventListener("change", reloadTransferContents);
  transferRevision.addEventListener("change", reloadTransferContents);
  transferExternalPolicy.addEventListener("change", clearTransferPreview);
  transferPreviewButton.addEventListener("click", previewTransfer);
  transferAcknowledge.addEventListener("change", updateTransferControls);
  transferCommitButton.addEventListener("click", commitTransfer);
  branchLayerViewMode.addEventListener("change", () => {
    branchLayerViewRevisionWrap.hidden = branchLayerViewMode.value === "current";
    branchLayerViewRevisionLabel.textContent = branchLayerViewMode.value === "historical"
      ? "Datenrevision (RecordedAsOf)"
      : "Datenrevision";
    branchLayerCurrentMode = branchLayerViewMode.value === "current";
    updateBranchLayerControls();
    if (projectOpen) refreshBranchLayers().catch(() => {});
  });
  branchLayerViewRevision.addEventListener("change", () => refreshBranchLayers().catch(() => {}));
  branchParentSelect.addEventListener("change", updateBranchCutoffBounds);
  branchCutoff.addEventListener("input", updateBranchLayerControls);
  branchCreateButton.addEventListener("click", createChildBranch);
  layerSymbol.addEventListener("input", updateBranchLayerControls);
  layerRank.addEventListener("input", updateBranchLayerControls);
  layerCreateButton.addEventListener("click", createOverlayLayer);
  layerEditSelect.addEventListener("change", loadLayerEditForm);
  layerBaseSelect.addEventListener("change", updateBranchLayerControls);
  layerEditRank.addEventListener("input", updateBranchLayerControls);
  layerUpdateButton.addEventListener("click", reviseSelectedLayer);
  schemaFamily.addEventListener("change", updateSchemaFormVisibility);
  schemaValueKind.addEventListener("change", updateSchemaFormVisibility);
  schemaCardinality.addEventListener("change", updateSchemaFormVisibility);
  eventAttributeKind.addEventListener("change", updateSchemaFormVisibility);
  eventMaxSpanEnabled.addEventListener("change", updateSchemaFormVisibility);
  addEventRoleButton.addEventListener("click", () => {
    try {
      addEventRole();
      schemaStatus.textContent = "Ereignisrolle vorgemerkt.";
    } catch (error) {
      schemaStatus.textContent = showError(error);
    }
  });
  addEventAttributeButton.addEventListener("click", () => {
    try {
      addEventAttribute();
      schemaStatus.textContent = "Ereignisattribut vorgemerkt.";
    } catch (error) {
      schemaStatus.textContent = showError(error);
    }
  });

  updateSchemaFormVisibility();
  renderStagedEventItems();
}
