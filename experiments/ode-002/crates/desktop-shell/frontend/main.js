const role = new URLSearchParams(location.search).get("role") ?? "unknown";
const invoke = window.__TAURI__?.core?.invoke;
const windowStatus = document.querySelector("#window-role");
const projectStatus = document.querySelector("#project-status");
const projectDetails = document.querySelector("#project-details");
const operationStatus = document.querySelector("#operation-status");
const reconcileOperationsButton = document.querySelector("#reconcile-operations");
const diagnosticPanel = document.querySelector("#diagnostic-panel");
const diagnosticExportButton = document.querySelector("#diagnostic-export");
const diagnosticStatus = document.querySelector("#diagnostic-status");
const projectName = document.querySelector("#project-name");
const createButton = document.querySelector("#create-project");
const openButton = document.querySelector("#open-project");
const closeButton = document.querySelector("#close-project");
const jobsPanel = document.querySelector("#jobs-panel");
const jobsStatus = document.querySelector("#jobs-status");
const jobsList = document.querySelector("#jobs-list");
const jobsRefreshButton = document.querySelector("#jobs-refresh");
const jobsCloseProjectButton = document.querySelector("#jobs-close-project");
const jobsShutdownStatus = document.querySelector("#jobs-shutdown-status");
const recoveryPanel = document.querySelector("#recovery-panel");
const recoveryInspectButton = document.querySelector("#recovery-inspect");
const recoveryOpenCleanButton = document.querySelector("#recovery-open-clean");
const recoveryStatus = document.querySelector("#recovery-status");
const recoveryReportElement = document.querySelector("#recovery-report");
const recoveryFindings = document.querySelector("#recovery-findings");
const recoveryActions = document.querySelector("#recovery-actions");
const recoveryNextActions = document.querySelector("#recovery-next-actions");
const recoveryKeepReadOnlyButton = document.querySelector("#recovery-keep-readonly");
const recoveryRunButton = document.querySelector("#recovery-run");
const recoveryRestoreButton = document.querySelector("#recovery-restore");
const recoveryArchiveName = document.querySelector("#recovery-archive-name");
const recoverySalvageFields = document.querySelector("#recovery-salvage-fields");
const recoverySalvageButton = document.querySelector("#recovery-salvage");
const backupPanel = document.querySelector("#backup-panel");
const backupProfile = document.querySelector("#backup-profile");
const backupProfileDetails = document.querySelector("#backup-profile-details");
const backupCreateButton = document.querySelector("#backup-create");
const backupVerifyButton = document.querySelector("#backup-verify");
const backupRestoreButton = document.querySelector("#backup-restore");
const backupStatus = document.querySelector("#backup-status");
const backupResult = document.querySelector("#backup-result");
const exportImportPanel = document.querySelector("#export-import-panel");
const exportKind = document.querySelector("#export-kind");
const exportFromRevision = document.querySelector("#export-from-revision");
const exportThroughRevision = document.querySelector("#export-through-revision");
const exportHistorySpaces = document.querySelector("#export-history-spaces");
const exportRecordClasses = document.querySelector("#export-record-classes");
const exportRunButton = document.querySelector("#export-run");
const importRemappings = document.querySelector("#import-remappings");
const importPlanButton = document.querySelector("#import-plan-create");
const importPrepareButton = document.querySelector("#import-prepare");
const exportImportStatus = document.querySelector("#export-import-status");
const exportImportResult = document.querySelector("#export-import-result");
const purgePanel = document.querySelector("#purge-panel");
const purgeTargets = document.querySelector("#purge-targets");
const purgeMode = document.querySelector("#purge-mode");
const purgeExternalComplete = document.querySelector("#purge-external-complete");
const purgeKnownCopies = document.querySelector("#purge-known-copies");
const purgePreviewButton = document.querySelector("#purge-preview");
const purgeExecuteButton = document.querySelector("#purge-execute");
const purgeDiscardButton = document.querySelector("#purge-discard");
const purgeStatus = document.querySelector("#purge-status");
const purgeResult = document.querySelector("#purge-result");
const migrationPanel = document.querySelector("#migration-panel");
const migrationSelectPlanButton = document.querySelector("#migration-select-plan");
const migrationPreviewButton = document.querySelector("#migration-preview");
const migrationCancelButton = document.querySelector("#migration-cancel");
const migrationRunButton = document.querySelector("#migration-run");
const migrationResumeButton = document.querySelector("#migration-resume");
const migrationStatus = document.querySelector("#migration-status");
const migrationPlanSummary = document.querySelector("#migration-plan-summary");
const migrationPreviewSummary = document.querySelector("#migration-preview-summary");

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
const schemaTimelineFields = document.querySelector("#timeline-fields");
const timelineCalendarProfile = document.querySelector("#timeline-calendar-profile");
const timelineEpochWrap = document.querySelector("#timeline-epoch-wrap");
const timelineEpochUnixNanoseconds = document.querySelector("#timeline-epoch-unix-nanoseconds");
const schemaTimeUnitFields = document.querySelector("#time-unit-fields");
const timeUnitNanosecondsPerTick = document.querySelector("#time-unit-nanoseconds-per-tick");
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
const factsPanel = document.querySelector("#facts-panel");
const factsHistorySpace = document.querySelector("#facts-history-space");
const factsLayer = document.querySelector("#facts-layer");
const factsEpistemicMode = document.querySelector("#facts-epistemic-mode");
const factsPerspectiveWrap = document.querySelector("#facts-perspective-wrap");
const factsPerspective = document.querySelector("#facts-perspective");
const factsSubject = document.querySelector("#facts-subject");
const factsPredicate = document.querySelector("#facts-predicate");
const factsContextNote = document.querySelector("#facts-context-note");
const factsPolarity = document.querySelector("#facts-polarity");
const factsValueKindLabel = document.querySelector("#facts-value-kind-label");
const factsValueTextWrap = document.querySelector("#facts-value-text-wrap");
const factsValueText = document.querySelector("#facts-value-text");
const factsValueBoolWrap = document.querySelector("#facts-value-bool-wrap");
const factsValueBool = document.querySelector("#facts-value-bool");
const factsValueEntityWrap = document.querySelector("#facts-value-entity-wrap");
const factsValueEntity = document.querySelector("#facts-value-entity");
const factsTimeValueFields = document.querySelector("#facts-time-value-fields");
const factsTimeValueTimeline = document.querySelector("#facts-time-value-timeline");
const factsTimeValueTicks = document.querySelector("#facts-time-value-ticks");
const factsTimeValueUnit = document.querySelector("#facts-time-value-unit");
const factsValidityTimeline = document.querySelector("#facts-validity-timeline");
const factsValidityEnabled = document.querySelector("#facts-validity-enabled");
const factsValidityRange = document.querySelector("#facts-validity-range");
const factsValidityStart = document.querySelector("#facts-validity-start");
const factsValidityEnd = document.querySelector("#facts-validity-end");
const factsCreateAssertion = document.querySelector("#facts-create-assertion");
const factsMaskSelector = document.querySelector("#facts-mask-selector");
const factsMaskExactWrap = document.querySelector("#facts-mask-exact-wrap");
const factsMaskAssertionId = document.querySelector("#facts-mask-assertion-id");
const factsMaskSelectorNote = document.querySelector("#facts-mask-selector-note");
const factsCreateMask = document.querySelector("#facts-create-mask");
const factsBoundaryNote = document.querySelector("#facts-boundary-note");
const factsCreateBoundary = document.querySelector("#facts-create-boundary");
const factsRecordCatalog = document.querySelector("#facts-record-catalog");
const factsMetaCatalog = document.querySelector("#facts-meta-catalog");
const factsSourceKind = document.querySelector("#facts-source-kind");
const factsSourceLocator = document.querySelector("#facts-source-locator");
const factsSourceDigest = document.querySelector("#facts-source-digest");
const factsSourceMetadataKey = document.querySelector("#facts-source-metadata-key");
const factsSourceMetadataValue = document.querySelector("#facts-source-metadata-value");
const factsSourceSupersedeTarget = document.querySelector("#facts-source-supersede-target");
const factsSourceCreate = document.querySelector("#facts-source-create");
const factsSourceSupersede = document.querySelector("#facts-source-supersede");
const factsEvidenceSource = document.querySelector("#facts-evidence-source");
const factsEvidenceTarget = document.querySelector("#facts-evidence-target");
const factsEvidenceRelation = document.querySelector("#facts-evidence-relation");
const factsEvidenceCreate = document.querySelector("#facts-evidence-create");
const factsEvidenceRetractTarget = document.querySelector("#facts-evidence-retract-target");
const factsEvidenceRetractReason = document.querySelector("#facts-evidence-retract-reason");
const factsEvidenceRetract = document.querySelector("#facts-evidence-retract");
const factsProvenanceFrom = document.querySelector("#facts-provenance-from");
const factsProvenanceTo = document.querySelector("#facts-provenance-to");
const factsProvenanceRelation = document.querySelector("#facts-provenance-relation");
const factsProvenanceCreate = document.querySelector("#facts-provenance-create");
const factsProvenanceRetractTarget = document.querySelector("#facts-provenance-retract-target");
const factsProvenanceRetractReason = document.querySelector("#facts-provenance-retract-reason");
const factsProvenanceRetract = document.querySelector("#facts-provenance-retract");
const factsCorrectionTarget = document.querySelector("#facts-correction-target");
const factsCorrectionReason = document.querySelector("#facts-correction-reason");
const factsCorrectionPreviewButton = document.querySelector("#facts-correction-preview");
const factsCorrectionPreviewResult = document.querySelector("#facts-correction-preview-result");
const factsCorrectionCommitButton = document.querySelector("#facts-correction-commit");
const factsEventCorrectionTarget = document.querySelector("#facts-event-correction-target");
const factsEventCorrectionDraft = document.querySelector("#facts-event-correction-draft");
const factsEventCorrectionPreviewButton = document.querySelector("#facts-event-correction-preview");
const factsEventCorrectionPreviewResult = document.querySelector("#facts-event-correction-preview-result");
const factsEventCorrectionCommitButton = document.querySelector("#facts-event-correction-commit");
const factsEventKind = document.querySelector("#facts-event-kind");
const factsEventDraft = document.querySelector("#facts-event-draft");
const factsEventCreate = document.querySelector("#facts-event-create");
const factsEventMaskTarget = document.querySelector("#facts-event-mask-target");
const factsEventMaskCreate = document.querySelector("#facts-event-mask-create");
const factsEventRelationFrom = document.querySelector("#facts-event-relation-from");
const factsEventRelationTo = document.querySelector("#facts-event-relation-to");
const factsEventRelationKind = document.querySelector("#facts-event-relation-kind");
const factsEventRelationCreate = document.querySelector("#facts-event-relation-create");
const factsEventGraphGuidance = document.querySelector("#facts-event-graph-guidance");
const factsEventSpanCloseTarget = document.querySelector("#facts-event-span-close-target");
const factsEventSpanCloseTimeline = document.querySelector("#facts-event-span-close-timeline");
const factsEventSpanCloseNanoseconds = document.querySelector("#facts-event-span-close-nanoseconds");
const factsEventSpanClose = document.querySelector("#facts-event-span-close");
const factsLifecycleTarget = document.querySelector("#facts-lifecycle-target");
const factsLifecycleAction = document.querySelector("#facts-lifecycle-action");
const factsLifecycleReason = document.querySelector("#facts-lifecycle-reason");
const factsLifecyclePreviewButton = document.querySelector("#facts-lifecycle-preview");
const factsLifecyclePreviewResult = document.querySelector("#facts-lifecycle-preview-result");
const factsLifecycleCommitButton = document.querySelector("#facts-lifecycle-commit");
const factsWriteStatus = document.querySelector("#facts-write-status");
const factsQueryTimeMode = document.querySelector("#facts-query-time-mode");
const factsQueryPointWrap = document.querySelector("#facts-query-point-wrap");
const factsQueryTimeline = document.querySelector("#facts-query-timeline");
const factsQueryNanosecondsWrap = document.querySelector("#facts-query-nanoseconds-wrap");
const factsQueryNanoseconds = document.querySelector("#facts-query-nanoseconds");
const factsQueryOperation = document.querySelector("#facts-query-operation");
const factsQueryRecordedAsOf = document.querySelector("#facts-query-recorded-as-of");
const factsQuerySchemaMode = document.querySelector("#facts-query-schema-mode");
const factsQuerySchemaRevisionWrap = document.querySelector("#facts-query-schema-revision-wrap");
const factsQuerySchemaRevision = document.querySelector("#facts-query-schema-revision");
const factsQuerySearchControls = document.querySelector("#facts-query-search-controls");
const factsQuerySearchTerms = document.querySelector("#facts-query-search-terms");
const factsQuerySearchMatch = document.querySelector("#facts-query-search-match");
const factsQueryPageSize = document.querySelector("#facts-query-page-size");
const factsQueryGraphControls = document.querySelector("#facts-query-graph-controls");
const factsQueryGraphRootFamily = document.querySelector("#facts-query-graph-root-family");
const factsQueryGraphRootId = document.querySelector("#facts-query-graph-root-id");
const factsQueryGraphRelationships = document.querySelector("#facts-query-graph-relationships");
const factsQueryGraphDirection = document.querySelector("#facts-query-graph-direction");
const factsQueryGraphMaxDepth = document.querySelector("#facts-query-graph-max-depth");
const factsQueryGraphMaxNodes = document.querySelector("#facts-query-graph-max-nodes");
const factsQueryGraphMaxEdges = document.querySelector("#facts-query-graph-max-edges");
const factsQueryGraphCyclePolicy = document.querySelector("#facts-query-graph-cycle-policy");
const factsQueryMaxCandidates = document.querySelector("#facts-query-max-candidates");
const factsQueryMaxWorkUnits = document.querySelector("#facts-query-max-work-units");
const factsQueryMaxResults = document.querySelector("#facts-query-max-results");
const factsPreviewButton = document.querySelector("#facts-preview");
const factsQueryContinueButton = document.querySelector("#facts-query-continue");
const factsPreviewStatus = document.querySelector("#facts-preview-status");
const factsPreviewResults = document.querySelector("#facts-preview-results");

const EXPORTABLE_RECORD_CLASSES = [
  "HistorySpaceDefinition",
  "Entity",
  "EntityRetirement",
  "PerspectiveDefinitionRevision",
  "PerspectiveRetirement",
  "LayerDefinition",
  "LayerSchemaSnapshot",
  "EntityTypeDefinition",
  "PredicateDefinition",
  "EventKindDefinition",
  "TimelineDefinition",
  "TimeUnitDefinition",
  "Assertion",
  "AssertionValidityClosure",
  "AssertionRetraction",
  "Mask",
  "MaskValidityClosure",
  "MaskRetraction",
  "ReplacementBoundary",
  "ReplacementBoundaryValidityClosure",
  "ReplacementBoundaryRetraction",
  "ArchiveTransition",
  "Event",
  "EventMask",
  "EventSpanClosure",
  "EventRetraction",
  "EventMaskRetraction",
  "EventRelation",
  "EventRelationRetraction",
  "Source",
  "Evidence",
  "Provenance",
  "EvidenceRetraction",
  "ProvenanceRetraction",
  "TransferLineage",
];

const userMessages = {
  project_already_exists: "An diesem Ort gibt es bereits ein Projekt.",
  project_already_open: "Es ist bereits ein anderes Projekt geöffnet. Schließe es zuerst.",
  project_unavailable: "Dieses Konto hat keinen Zugriff auf das Projekt.",
  invalid_project: "Der ausgewählte Ordner enthält kein gültiges WorldDB-Projekt.",
  invalid_job_id: "Der Job konnte nicht eindeutig zugeordnet werden.",
  invalid_operation_id: "Die Vorgangskennung ist ungültig. Lade den Projektstatus neu.",
  recovery_required: "Das Projekt benötigt eine Prüfung oder Wiederherstellung und wurde nicht geöffnet.",
  recovery_inspection_unavailable: "Das Projekt konnte nicht read-only geprüft werden. Es wurde nicht geöffnet oder repariert.",
  journaled_recovery_rejected: "Die journalisierte Recovery wurde abgelehnt oder ist für diesen Befund nicht sicher.",
  salvage_rejected: "Salvage wurde abgelehnt. Das Quellprojekt blieb unverändert.",
  explicit_confirmation_required: "Diese Aktion benötigt eine eigene ausdrückliche Bestätigung.",
  backup_rejected: "Sicherung oder Wiederherstellung wurde abgelehnt. Prüfe Projekt, Eingabe und Berechtigung.",
  backup_unavailable: "Sicherungs- oder Wiederherstellungsdienste sind nicht verfügbar.",
  diagnostic_export_rejected: "Der Diagnoseexport wurde abgelehnt. Prüfe Berechtigung und gewählten Dateinamen.",
  diagnostic_export_unavailable: "Der Diagnoseexport konnte nicht gespeichert werden.",
  engine_unavailable: "Die WorldDB-Engine ist nicht verfügbar. Öffne das Projekt erneut.",
  export_import_rejected: "Export oder Import wurde abgelehnt. Prüfe Projekt, Eingabe und Berechtigung.",
  export_import_unavailable: "Export- und Importdienste sind nicht verfügbar.",
  ipc_diagnostic_unavailable: "Die interne Diagnose konnte nicht aufgezeichnet werden.",
  job_cancel_unavailable: "Der Job konnte nicht abgebrochen werden. Aktualisiere die Jobliste.",
  job_state_unavailable: "Der Jobstatus konnte nicht geladen werden.",
  migration_unavailable: "Die Migration kann gerade nicht geändert werden. Prüfe, ob ein anderes Fenster eine Auswahl oder Ausführung geöffnet hat.",
  migration_rejected: "Die Migration wurde abgelehnt. Prüfe den Plan und den aktuellen Projektstand.",
  host_unavailable: "Der lokale WorldDB-Host ist gerade nicht verfügbar.",
  selection_cancelled: "Die Auswahl wurde abgebrochen.",
  invalid_request: "Bitte prüfe die Eingabe.",
  unauthorized: "Diese Aktion ist für das aktuelle Konto oder Projekt nicht freigegeben.",
  unsupported_protocol: "Die App und die Engine verwenden unterschiedliche Protokollversionen.",
  purge_rejected: "Der Purge wurde abgelehnt. Prüfe Plan, Referenzen und Berechtigung.",
  purge_unavailable: "Der Purge-Dienst ist gerade nicht verfügbar.",
  "query cursor is invalidated; restart the search": "Der Suchcursor ist abgelaufen oder nicht mehr gültig. Starte die Suche erneut.",
  "query session state is unavailable": "Die Suchsitzung ist nicht verfügbar. Starte die Suche erneut.",
  "query cursor state is unavailable": "Die Suchsitzung ist nicht verfügbar. Starte die Suche erneut.",
  unknown_commit_outcome: "Der Speicherstatus ist unklar. Prüfe das Projekt, bevor du es erneut änderst.",
  schema_rejected: "Die Schema-Aktion wurde abgelehnt. Prüfe Eingaben, Berechtigung und aktuellen Projektstand.",
  entity_rejected: "Die Entitätsaktion wurde abgelehnt. Prüfe Eingaben, Berechtigung und aktuellen Projektstand.",
  branch_layer_rejected: "Die Branch- oder Layer-Aktion wurde abgelehnt. Prüfe Cutoff, Priorität, Berechtigung und aktuellen Projektstand.",
  history_space_transfer_rejected: "Die Übertragung wurde abgelehnt. Lade Quelle und Ziel neu und prüfe die Verweise sowie die Vorschau.",
  facts_rejected: "Der Fakt wurde abgelehnt. Prüfe Schema, Kontext, Weltzeit, Berechtigung und aktuellen Projektstand.",
  perspective_rejected: "Die Perspektivenaktion wurde abgelehnt. Prüfe Eingaben, Berechtigung und aktuellen Projektstand.",
  security_policy_rejected: "Die Rechteaktion wurde abgelehnt. Prüfe die erforderliche Berechtigung und lade den aktuellen Projektstand neu.",
};

let sessionId;
let projectOpen = false;
let projectRevision = null;
let currentDatabaseId = null;
let pendingOperations = [];
let operationReconciliationRunning = false;
let operationJournalUnavailable = false;
let projectCreationJournalUnavailable = false;
const pendingProjectCreationPrefix = "worlddb.pending_project_creations.v1:";
let projectBusy = false;
let jobsBusy = false;
let recoveryBusy = false;
let currentRecoveryReport = null;
let backupBusy = false;
let exportImportBusy = false;
let purgeBusy = false;
let currentPurgePlan = null;
let currentPurgeRequest = null;
let migrationBusy = false;
let currentMigrationState = null;
let schemaBusy = false;
let entityBusy = false;
let perspectiveBusy = false;
let securityPolicyBusy = false;
let securityPolicyUnavailable = false;
let branchLayerBusy = false;
let transferBusy = false;
let factBusy = false;
let diagnosticExportBusy = false;
let factsSmokeActive = false;
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
let transferPreviewBaseRevision = null;
let factCatalog = null;
let factCatalogRefreshPromise = null;
let factCatalogRefreshQueued = false;
let factSearchCursor = null;
let factSearchBaseSignature = null;
let projectRefreshPromise = null;
let projectRefreshQueued = false;
let jobsRefreshPromise = null;
let pendingFactActionPreviews = { assertion: false, event: false, lifecycle: false };
let pendingFactCorrectionCommands = { assertion: null, event: null };
let stagedEventRoles = [];
let stagedEventAttributes = [];
let stagedLifecycleChanges = [];

const localErrorCodes = new Set([
  "commit_conflict",
  "commit_confirmed",
  "not_committed",
  "unresolved_operation",
  "pending_storage_unavailable",
]);

const nextActionMessages = {
  "worlddb.error.action.resolve_operation": "Den Vorgangsstatus abgleichen, bevor du erneut schreibst.",
  "worlddb.error.action.refresh_and_review": "Aktualisiere die Daten und prüfe die Aktion erneut.",
  "worlddb.error.action.open_recovery": "Öffne die Recovery-Prüfung.",
  "worlddb.error.action.check_access": "Prüfe die Freigabe im aktuellen Projekt.",
  "worlddb.error.action.review_input": "Prüfe Eingabe und aktuellen Projektstand.",
  "worlddb.error.action.none": "Keine weiteren Schritte erforderlich.",
  "worlddb.error.action.contact_support": "Notiere Fehlercode und Vorgangskennung für den Support.",
};

function errorCode(error) {
  const candidate = typeof error === "string" ? error : error?.code ?? error?.message;
  if (typeof candidate !== "string") return "host_unavailable";
  if (Object.hasOwn(userMessages, candidate) || localErrorCodes.has(candidate)) return candidate;
  return "host_unavailable";
}

function safeOperationId(value) {
  return typeof value === "string"
    && /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(value)
    ? value
    : null;
}

function safeRevision(value) {
  const text = String(value ?? "");
  return /^(0|[1-9][0-9]{0,19})$/.test(text) ? text : "unbekannt";
}

function nextActionKey(code) {
  if (code === "unknown_commit_outcome") return "worlddb.error.action.resolve_operation";
  if (code === "commit_conflict") return "worlddb.error.action.refresh_and_review";
  if (code === "recovery_required" || code === "recovery_inspection_unavailable") {
    return "worlddb.error.action.open_recovery";
  }
  if (code === "unauthorized" || code === "project_unavailable") return "worlddb.error.action.check_access";
  if (code === "invalid_request" || code === "explicit_confirmation_required") {
    return "worlddb.error.action.review_input";
  }
  if (code === "selection_cancelled") return "worlddb.error.action.none";
  return "worlddb.error.action.contact_support";
}

function errorMetadataText(error, code) {
  const messageKey = `worlddb.error.${code}`;
  const actionKey = nextActionKey(code);
  const operationId = safeOperationId(error?.operation_id);
  const operation = operationId ? ` · OperationId ${operationId}` : "";
  return `\nFehlercode ${code} · Lokalisierung ${messageKey} · Nächster Schritt: ${nextActionMessages[actionKey]}${operation}`;
}

function showError(error) {
  const code = errorCode(error);
  let message;
  if (code === "commit_conflict") {
    const expected = safeRevision(error?.expected_base_revision);
    const current = safeRevision(error?.current_revision);
    message = `Konfliktbericht: Die Projektbasis ist von Revision ${expected} auf ${current} fortgeschritten. Es wurde nichts gespeichert.`;
  } else if (code === "commit_confirmed") {
    const operationId = safeOperationId(error?.operation_id) ?? "nicht verfügbar";
    const revision = safeRevision(error?.revision);
    message = `Commit bestätigt · Operation ${operationId} · Revision ${revision}. Lade die aktuellen Daten, um den gespeicherten Stand zu sehen.`;
  } else if (code === "not_committed") {
    message = "WAL-Status bestätigt: Es wurde nichts gespeichert. Prüfe die Eingabe und versuche es erneut.";
  } else if (code === "unresolved_operation") {
    message = "Eine vorherige Schreibaktion ist noch nicht geklärt. Prüfe zuerst ihren Status; neue Schreibaktionen bleiben bis dahin gesperrt.";
  } else if (code === "pending_storage_unavailable") {
    message = "Der Schreibstatus kann auf diesem Gerät nicht dauerhaft vorgemerkt werden. Es wurde nichts gesendet.";
  } else if ([
    "query cursor is invalidated; restart the search",
    "query session state is unavailable",
    "query cursor state is unavailable",
  ].includes(code)) {
    invalidateFactsSearch();
    message = "Der Suchcursor ist abgelaufen oder nicht mehr gültig. Starte die Suche erneut.";
  } else {
    message = userMessages[code] ?? "Die Aktion konnte nicht abgeschlossen werden.";
  }
  return `${message}${errorMetadataText(error, code)}`;
}

function backupErrorText(error) {
  return showError(error);
}

function operationStoragePrefix(databaseId = currentDatabaseId) {
  return databaseId ? `worlddb.pending_operations.v1:${databaseId}:` : null;
}

function operationStorageKey(operationId, databaseId = currentDatabaseId) {
  const prefix = operationStoragePrefix(databaseId);
  return prefix && operationId ? `${prefix}${operationId}` : null;
}

function pendingProjectCreationKey(operationId) {
  return operationId ? `${pendingProjectCreationPrefix}${operationId}` : null;
}

function loadPendingProjectCreations() {
  const operations = [];
  try {
    for (let index = 0; index < localStorage.length; index += 1) {
      const key = localStorage.key(index);
      if (!key?.startsWith(pendingProjectCreationPrefix)) continue;
      const operationId = key.slice(pendingProjectCreationPrefix.length);
      const operation = JSON.parse(localStorage.getItem(key));
      if (!/^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(operationId)
        || operation?.operation_id !== operationId
        || operation.command !== "create_project"
        || (operation.database_id !== null && !/^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(operation.database_id))
        || typeof operation.started_at !== "string") {
        projectCreationJournalUnavailable = true;
        return [];
      }
      operations.push(operation);
    }
  } catch {
    projectCreationJournalUnavailable = true;
    return [];
  }
  return operations.sort((left, right) => left.started_at.localeCompare(right.started_at));
}

function rememberPendingProjectCreation(operation) {
  if (projectCreationJournalUnavailable) throw new Error("pending_storage_unavailable");
  const key = pendingProjectCreationKey(operation.operation_id);
  if (!key) throw new Error("pending_storage_unavailable");
  try {
    localStorage.setItem(key, JSON.stringify(operation));
  } catch {
    throw new Error("pending_storage_unavailable");
  }
}

function forgetPendingProjectCreation(operationId) {
  const key = pendingProjectCreationKey(operationId);
  if (!key) throw new Error("pending_storage_unavailable");
  try {
    localStorage.removeItem(key);
  } catch {
    projectCreationJournalUnavailable = true;
    throw new Error("pending_storage_unavailable");
  }
}

function loadPendingOperations(databaseId) {
  const prefix = operationStoragePrefix(databaseId);
  if (!prefix) return [];
  try {
    const operations = [];
    for (let index = 0; index < localStorage.length; index += 1) {
      const key = localStorage.key(index);
      if (!key?.startsWith(prefix)) continue;
      const operationId = key.slice(prefix.length);
      const operation = JSON.parse(localStorage.getItem(key));
      if (!/^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(operationId)
        || operation?.operation_id !== operationId
        || typeof operation.command !== "string"
        || (operation.expected_base_revision !== null && !Number.isSafeInteger(operation.expected_base_revision))
        || typeof operation.started_at !== "string") {
        operationJournalUnavailable = true;
        return [];
      }
      operations.push(operation);
    }
    return operations.sort((left, right) => left.started_at.localeCompare(right.started_at));
  } catch {
    operationJournalUnavailable = true;
    return [];
  }
}

function savePendingOperation(operation) {
  const key = operationStorageKey(operation.operation_id);
  if (!key) throw new Error("pending_storage_unavailable");
  try {
    localStorage.setItem(key, JSON.stringify(operation));
  } catch {
    throw new Error("pending_storage_unavailable");
  }
}

function refreshPendingOperationsFromStorage() {
  if (!currentDatabaseId) return;
  pendingOperations = loadPendingOperations(currentDatabaseId);
  reconcileOperationsButton.hidden = !hasUnresolvedOperation();
  reconcileOperationsButton.disabled = operationReconciliationRunning || !projectOpen || operationJournalUnavailable;
  updateSchemaControls();
}

function hasUnresolvedOperation() {
  return operationJournalUnavailable || pendingOperations.length > 0;
}

function rememberPendingOperation(operation) {
  if (operationJournalUnavailable) throw new Error("pending_storage_unavailable");
  refreshPendingOperationsFromStorage();
  if (hasUnresolvedOperation()) throw new Error("unresolved_operation");
  savePendingOperation(operation);
  refreshPendingOperationsFromStorage();
}

function forgetPendingOperation(operationId) {
  const key = operationStorageKey(operationId);
  if (!key) throw new Error("pending_storage_unavailable");
  try {
    localStorage.removeItem(key);
  } catch {
    operationJournalUnavailable = true;
    refreshPendingOperationsFromStorage();
    throw new Error("pending_storage_unavailable");
  }
  refreshPendingOperationsFromStorage();
}

window.addEventListener("storage", (event) => {
  if (event.key === null) {
    operationJournalUnavailable = true;
    projectCreationJournalUnavailable = true;
    reconcileOperationsButton.hidden = false;
    reconcileOperationsButton.disabled = true;
    operationStatus.textContent = "Der lokale Schreibstatusspeicher wurde in einem anderen Fenster geleert. Schreibaktionen und neue Projektanlagen bleiben gesperrt.";
    updateSchemaControls();
    return;
  }
  if (event.key.startsWith(pendingProjectCreationPrefix)) {
    loadPendingProjectCreations();
    updateProjectControls();
    if (projectOpen) {
      void reconcilePendingProjectCreations().catch((error) => {
        operationStatus.textContent = showError(error);
      });
    }
    return;
  }
  const prefix = operationStoragePrefix();
  if (!prefix) return;
  if (!event.key.startsWith(prefix)) return;
  const hadPendingOperations = pendingOperations.length > 0;
  refreshPendingOperationsFromStorage();
  if (operationJournalUnavailable) {
    operationStatus.textContent = "Der vorgemerkte Schreibstatus kann nicht sicher gelesen werden. Schreibaktionen bleiben gesperrt.";
  } else if (hadPendingOperations && pendingOperations.length === 0) {
    operationStatus.textContent = "Der Status der ausstehenden Schreibaktion wurde in einem anderen Fenster geklärt.";
  }
});

function expectedBaseRevision(command) {
  return Number.isSafeInteger(command?.expected_base_revision) ? command.expected_base_revision : null;
}

function mutationCommand(ipcCommand, command) {
  if (!command || typeof command.command !== "string") return false;
  const writes = {
    manage_schema: new Set(["create", "set_lifecycle", "set_lifecycle_batch"]),
    manage_entities: new Set(["create", "retire"]),
    manage_branch_layers: new Set(["create_child", "create_layer", "revise_layer"]),
    manage_history_space_transfer: new Set(["commit"]),
    manage_perspectives: new Set(["create", "update", "retire"]),
    manage_security_policy: new Set([
      "set_principal_state", "register_role", "assign_role", "revoke_role_assignment",
      "add_capability_rule", "revoke_capability_rule",
    ]),
  };
  if (ipcCommand === "manage_facts") {
    return !["snapshot", "preview", "query", "commit_status"].includes(command.command);
  }
  return writes[ipcCommand]?.has(command.command) ?? false;
}

async function readOperationStatus(operation) {
  const response = await invoke("manage_facts", {
    request: {
      protocol_version: 1,
      session_id: sessionId,
      command: {
        command: "commit_status",
        operation_id: operation.operation_id,
        expected_base_revision: operation.expected_base_revision,
      },
    },
  });
  if (response.protocol_version !== 1 || response.result?.kind !== "operation_status") {
    throw new Error("unsupported_protocol");
  }
  if (response.result.operation_id !== operation.operation_id
    || !Number.isSafeInteger(response.result.current_revision)
    || (response.result.status === "committed" && !Number.isSafeInteger(response.result.revision))) {
    throw new Error("unsupported_protocol");
  }
  return response.result;
}

async function createProjectTracked(activeSessionId, projectName) {
  const operation = {
    operation_id: crypto.randomUUID(),
    command: "create_project",
    database_id: null,
    expected_base_revision: null,
    started_at: new Date().toISOString(),
  };
  rememberPendingProjectCreation(operation);
  try {
    const result = await invoke("create_project", {
      sessionId: activeSessionId,
      request: {
        protocol_version: 1,
        project_name: projectName,
        operation_id: operation.operation_id,
      },
    });
    forgetPendingProjectCreation(operation.operation_id);
    return result;
  } catch (error) {
    const code = errorCode(error);
    if (code === "unknown_commit_outcome") {
      if (error?.operation_id && error.operation_id !== operation.operation_id) {
        throw new Error("unsupported_protocol");
      }
      if (typeof error?.database_id === "string") {
        rememberPendingProjectCreation({ ...operation, database_id: error.database_id });
      }
    } else if ([
      "access_denied", "invalid_operation_id", "invalid_project", "invalid_request",
      "project_already_exists", "project_already_open", "project_unavailable",
      "selection_cancelled", "unsupported_protocol", "user_cancelled",
    ].includes(code)) {
      forgetPendingProjectCreation(operation.operation_id);
    }
    throw error;
  }
}

async function reconcilePendingProjectCreations() {
  if (!projectOpen || !currentDatabaseId || !sessionId) return;
  const operations = loadPendingProjectCreations();
  if (projectCreationJournalUnavailable) {
    operationStatus.textContent = "Der Status einer möglichen Projektanlage kann nicht sicher gelesen werden. Neue Projektanlagen bleiben gesperrt.";
    updateProjectControls();
    return;
  }
  for (const operation of operations) {
    if (operation.database_id && operation.database_id !== currentDatabaseId) continue;
    let status;
    try {
      status = await readOperationStatus(operation);
    } catch {
      operationStatus.textContent = `Der Bootstrap-Status für Operation ${operation.operation_id} konnte in diesem Projekt nicht gelesen werden; der Eintrag bleibt vorgemerkt.`;
      continue;
    }
    if (status.status === "committed") {
      forgetPendingProjectCreation(operation.operation_id);
      operationStatus.textContent = `Projektinitialisierung bestätigt · Operation ${operation.operation_id} · Revision ${status.revision}.`;
    } else if (status.status === "indeterminate") {
      savePendingOperation({
        ...operation,
        database_id: currentDatabaseId,
      });
      refreshPendingOperationsFromStorage();
      forgetPendingProjectCreation(operation.operation_id);
      operationStatus.textContent = `Bootstrap-Operation ${operation.operation_id} ist weiterhin ungeklärt. Schreibaktionen bleiben gesperrt.`;
    } else if (operation.database_id === currentDatabaseId) {
      forgetPendingProjectCreation(operation.operation_id);
      operationStatus.textContent = `Bootstrap-Status bestätigt: Operation ${operation.operation_id} wurde nicht committed.`;
    } else {
      operationStatus.textContent = `Operation ${operation.operation_id} gehört nicht zu diesem Projekt; der Abgleich bleibt beim Öffnen des betroffenen Projekts vorgemerkt.`;
    }
  }
  updateProjectControls();
}

function conflictError(status) {
  const error = new Error("commit_conflict");
  error.code = "commit_conflict";
  error.expected_base_revision = status.expected_base_revision;
  error.current_revision = status.current_revision;
  return error;
}

async function reconcilePendingOperations() {
  if (!projectOpen || !currentDatabaseId || !sessionId || operationReconciliationRunning) return;
  if (!pendingOperations.length) {
    reconcileOperationsButton.hidden = true;
    return;
  }
  operationReconciliationRunning = true;
  reconcileOperationsButton.disabled = true;
  try {
    for (const operation of [...pendingOperations]) {
      const status = await readOperationStatus(operation);
      if (status.status === "committed") {
        forgetPendingOperation(operation.operation_id);
        operationStatus.textContent = `Commit nach Projektöffnung bestätigt · Operation ${operation.operation_id} · Revision ${status.revision}.`;
      } else if (status.status === "not_committed") {
        forgetPendingOperation(operation.operation_id);
        operationStatus.textContent = status.conflict_report?.facts?.includes("base_revision_advanced")
          ? showError(conflictError(status))
          : `WAL-Status nach Projektöffnung: Operation ${operation.operation_id} wurde nicht committed. Eine neue Aktion ist jetzt möglich.`;
      } else {
        operationStatus.textContent = `Operation ${operation.operation_id} ist weiterhin ungeklärt. Schreibaktionen bleiben gesperrt.`;
      }
    }
  } catch (error) {
    operationStatus.textContent = `Der WAL-Status konnte nicht gelesen werden. Vorgemerkte Schreibaktionen bleiben gesperrt. ${showError(error)}`;
  } finally {
    operationReconciliationRunning = false;
    reconcileOperationsButton.hidden = pendingOperations.length === 0;
    reconcileOperationsButton.disabled = pendingOperations.length === 0 || !projectOpen;
    updateSchemaControls();
  }
}

async function invokeManagedCommand(ipcCommand, command, activeSessionId, validateResponse, operationIdOverride = command?.operation_id) {
  const write = mutationCommand(ipcCommand, command);
  const send = async (operationId) => {
    const response = await invoke(ipcCommand, {
      request: {
        protocol_version: 1,
        session_id: activeSessionId,
        ...(operationId ? { operation_id: operationId } : {}),
        command,
      },
    });
    if (!validateResponse(response)) throw new Error("unsupported_protocol");
    return response.result;
  };
  if (!write) return send(null);
  if (!projectOpen || !currentDatabaseId) throw new Error("project_unavailable");
  if (hasUnresolvedOperation()) throw new Error("unresolved_operation");
  const operationId = operationIdOverride ?? crypto.randomUUID();
  const operation = {
    operation_id: operationId,
    command: command.command,
    expected_base_revision: expectedBaseRevision(command)
      ?? (ipcCommand === "manage_history_space_transfer" && command.command === "commit"
        ? transferPreviewBaseRevision
        : null),
    started_at: new Date().toISOString(),
  };
  rememberPendingOperation(operation);
  try {
    const result = await send(operationId);
    const receiptOperationId = result?.operation_id;
    const noWriteConflict = ipcCommand === "manage_facts" && result?.kind === "event_graph_conflict";
    if (write && !noWriteConflict && receiptOperationId !== operationId) {
      throw new Error("unsupported_protocol");
    }
    if (receiptOperationId && receiptOperationId !== operationId) throw new Error("unsupported_protocol");
    forgetPendingOperation(operationId);
    return result;
  } catch (writeError) {
    let status;
    try {
      status = await readOperationStatus(operation);
    } catch {
      operationStatus.textContent = showError(Object.assign(new Error("unknown_commit_outcome"), { operation_id: operationId }));
      throw Object.assign(new Error("unknown_commit_outcome"), { operation_id: operationId });
    }
    if (status.status === "committed") {
      forgetPendingOperation(operationId);
      const confirmed = Object.assign(new Error("commit_confirmed"), {
        operation_id: operationId,
        revision: status.revision,
      });
      operationStatus.textContent = showError(confirmed);
      throw confirmed;
    }
    if (status.status === "not_committed") {
      forgetPendingOperation(operationId);
      if (status.conflict_report?.facts?.includes("base_revision_advanced")) {
        const conflict = conflictError(status);
        operationStatus.textContent = showError(conflict);
        throw conflict;
      }
      const rejected = Object.assign(new Error("not_committed"), {
        write_error_code: errorCode(writeError),
      });
      throw rejected;
    }
    operationStatus.textContent = showError(Object.assign(new Error("unknown_commit_outcome"), { operation_id: operationId }));
    throw Object.assign(new Error("unknown_commit_outcome"), { operation_id: operationId });
  }
}

function updateSchemaControls() {
  const canRead = projectOpen && !schemaBusy && !projectBusy && !entityBusy && !perspectiveBusy && !securityPolicyBusy && !branchLayerBusy && !transferBusy && !factBusy;
  const canMutate = canRead && schemaCurrentMode && !hasUnresolvedOperation();
  schemaRefreshButton.disabled = !canRead;
  schemaCreateButton.disabled = !canMutate;
  for (const control of schemaEditor.querySelectorAll("input, select, textarea, button")) {
    control.disabled = schemaBusy || projectBusy || perspectiveBusy || securityPolicyBusy || branchLayerBusy || transferBusy || factBusy || hasUnresolvedOperation();
  }
  schemaCreateButton.disabled = !canMutate;
  schemaLifecyclePublish.disabled = !canMutate || stagedLifecycleChanges.length === 0;
  for (const button of schemaDefinitions.querySelectorAll("button[data-lifecycle]")) {
    button.disabled = !canMutate;
  }
  for (const button of schemaLifecyclePending.querySelectorAll("button")) {
    button.disabled = schemaBusy || projectBusy || hasUnresolvedOperation();
  }
  updateEntityControls();
  updatePerspectiveControls();
  updateSecurityPolicyControls();
  updateBranchLayerControls();
  updateTransferControls();
  updateFactControls();
  updateProjectControls();
}

function updateEntityControls() {
  if (!entityPanel) return;
  const canRead = projectOpen && !entityBusy && !projectBusy && !schemaBusy && !perspectiveBusy && !securityPolicyBusy && !branchLayerBusy && !transferBusy && !factBusy;
  const canMutate = canRead && entityCurrentMode && !hasUnresolvedOperation();
  entityRefreshButton.disabled = !canRead;
  for (const control of entityEditor.querySelectorAll("input, select, button")) {
    control.disabled = entityBusy || projectBusy || schemaBusy || perspectiveBusy || securityPolicyBusy || branchLayerBusy || transferBusy || factBusy || hasUnresolvedOperation();
  }
  entityCreateButton.disabled = !canMutate || !entityTypeSelect.value
    || (selectedEntityType()?.lifecycle === "deprecated" && !entityAcceptDeprecated.checked);
  for (const button of entityList.querySelectorAll("button[data-entity-retire]")) {
    button.disabled = !canMutate;
  }
}

function updatePerspectiveControls() {
  if (!perspectivePanel) return;
  const blocked = perspectiveBusy || securityPolicyBusy || projectBusy || schemaBusy || entityBusy || branchLayerBusy || transferBusy || factBusy || hasUnresolvedOperation();
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
  const blocked = securityPolicyBusy || projectBusy || schemaBusy || entityBusy || perspectiveBusy || branchLayerBusy || transferBusy || factBusy;
  const canRead = projectOpen && !blocked && !securityPolicyUnavailable;
  const canWrite = canRead && !hasUnresolvedOperation();
  securityPolicyRefresh.disabled = !canRead;
  for (const editor of [
    securityPolicyPanel.querySelector("#security-policy-principal-editor"),
    securityPolicyPanel.querySelector("#security-policy-role-editor"),
    securityPolicyPanel.querySelector("#security-policy-rule-editor"),
  ]) {
    for (const control of editor.querySelectorAll("input, select, button")) control.disabled = !canWrite;
  }
  policyPrincipalSave.disabled = !canWrite || !policyPrincipalSelect.value;
  policyRoleAssign.disabled = !canWrite || !policyAssignmentPrincipal.value || !policyAssignmentRole.value;
  policyRoleCreate.disabled = !canWrite || !/^[a-z][a-z0-9_]*$/.test(policyNewRoleSymbol.value);
  policyRuleAdd.disabled = !canWrite || !policyRuleSubject.value || !policyRuleCapability.value;
  for (const button of securityPolicyPanel.querySelectorAll("button[data-policy-revoke]")) {
    button.disabled = !canWrite;
  }
}

function updateBranchLayerControls() {
  if (!branchLayerPanel) return;
  const canRead = projectOpen && !branchLayerBusy && !projectBusy && !schemaBusy && !entityBusy && !perspectiveBusy && !securityPolicyBusy && !transferBusy && !factBusy;
  const canMutate = canRead && branchLayerCurrentMode && !hasUnresolvedOperation();
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
  const blocked = transferBusy || projectBusy || schemaBusy || entityBusy || perspectiveBusy || securityPolicyBusy || branchLayerBusy || factBusy;
  const canRead = projectOpen && !blocked;
  transferLoadButton.disabled = !canRead || !transferSource.value || !transferTarget.value
    || transferSource.value === transferTarget.value;
  transferPicker.disabled = !canRead || !transferCatalog;
  transferPreviewButton.disabled = !canRead || !transferCatalog
    || transferContentList.querySelectorAll('input[type="checkbox"]:checked').length === 0;
  transferCommitButton.disabled = !canRead || hasUnresolvedOperation() || !transferPreviewTicket || !transferAcknowledge.checked;
}

function updateProjectControls() {
  const hasUnresolvedProjectCreation = projectCreationJournalUnavailable
    || loadPendingProjectCreations().length > 0;
  const migrationSelected = Boolean(currentMigrationState?.plan) || migrationBusy;
  createButton.disabled = projectBusy || backupBusy || exportImportBusy || purgeBusy || schemaBusy || entityBusy || perspectiveBusy || securityPolicyBusy || branchLayerBusy || transferBusy || factBusy || projectOpen || hasUnresolvedProjectCreation || migrationSelected;
  openButton.disabled = projectBusy || backupBusy || exportImportBusy || purgeBusy || schemaBusy || entityBusy || perspectiveBusy || securityPolicyBusy || branchLayerBusy || transferBusy || factBusy || projectOpen || migrationSelected;
  closeButton.disabled = projectBusy || backupBusy || exportImportBusy || purgeBusy || schemaBusy || entityBusy || perspectiveBusy || securityPolicyBusy || branchLayerBusy || transferBusy || factBusy || !projectOpen;
  jobsRefreshButton.disabled = jobsBusy || !projectOpen;
  jobsCloseProjectButton.disabled = closeButton.disabled;
  updateMigrationControls();
  updateRecoveryControls();
  updateBackupControls();
  updateExportImportControls();
  updatePurgeControls();
  updateDiagnosticControls();
}

function updateRecoveryControls() {
  const blocked = recoveryBusy || backupBusy || exportImportBusy || purgeBusy || migrationBusy || projectBusy || projectOpen;
  recoveryInspectButton.disabled = blocked;
  recoveryKeepReadOnlyButton.disabled = blocked || !currentRecoveryReport;
  recoveryRunButton.disabled = blocked || !currentRecoveryReport?.can_run_journaled_recovery;
  recoveryRestoreButton.disabled = blocked || !currentRecoveryReport?.can_restore_verified_backup;
  recoverySalvageButton.disabled = blocked || !currentRecoveryReport?.can_salvage;
  recoveryOpenCleanButton.disabled = blocked || currentRecoveryReport?.disposition !== "clean";
  updateExportImportControls();
  updatePurgeControls();
}

function updateBackupControls() {
  const blocked = backupBusy || exportImportBusy || purgeBusy || migrationBusy || recoveryBusy || projectBusy || projectOpen;
  backupProfile.disabled = blocked;
  backupCreateButton.disabled = blocked;
  backupVerifyButton.disabled = blocked;
  backupRestoreButton.disabled = blocked;
}

function updateDiagnosticControls() {
  const blocked = !sessionId || !projectOpen || diagnosticExportBusy || projectBusy || jobsBusy
    || recoveryBusy || backupBusy || exportImportBusy || purgeBusy || migrationBusy || schemaBusy
    || entityBusy || perspectiveBusy || securityPolicyBusy || branchLayerBusy || transferBusy || factBusy;
  diagnosticExportButton.disabled = blocked;
}

async function exportDiagnostics() {
  if (!sessionId || !projectOpen || diagnosticExportBusy) return;
  diagnosticExportBusy = true;
  diagnosticStatus.textContent = "Prüfe die aktuellen AuditRead- und AuditExport-Rechte …";
  updateProjectControls();
  try {
    const result = await invoke("export_diagnostics", {
      sessionId,
      request: { protocol_version: 1 },
    });
    if (result?.protocol_version !== 1 || typeof result.file_name !== "string"
      || !Number.isSafeInteger(result.record_count) || !Number.isSafeInteger(result.bytes)
      || !/^[0-9a-f]{64}$/.test(result.digest)) {
      throw new Error("unsupported_protocol");
    }
    diagnosticStatus.textContent = `Diagnoseexport gespeichert · ${result.file_name} · ${result.record_count} Einträge · ${result.bytes} Byte · BLAKE3 ${result.digest}`;
  } catch (error) {
    diagnosticStatus.textContent = showError(error);
  } finally {
    diagnosticExportBusy = false;
    updateProjectControls();
  }
}

function migrationEntry(container, label, value) {
  const entry = document.createElement("div");
  entry.className = "result-cell";
  const heading = document.createElement("strong");
  heading.textContent = label;
  const detail = document.createElement("p");
  detail.textContent = value;
  entry.append(heading, detail);
  container.append(entry);
}

function backupProfileLabel(profile) {
  return profile === "audit_complete" ? "AuditCompleteBackup" : "ExactDatabaseBackup";
}

function updateBackupProfileDetails() {
  if (backupProfile.value === "audit_complete") {
    backupProfileDetails.textContent = "Auditumfang: Included. Enthält den unterstützten RawRead-Audit-Prefix. Erfordert zusätzlich AuditRead und AuditExport; CLI und Storage prüfen die aktuelle Policy erneut.";
  } else {
    backupProfileDetails.textContent = "Auditumfang: Excluded. Enthält keine Audit-Historie. Erstellung und Restore erfordern die jeweiligen aktuellen ProjectRead-, BackupCreate- oder BackupRestore-Rechte.";
  }
}

function renderBackupResult(result) {
  backupResult.replaceChildren();
  backupResult.hidden = !result;
  if (!result) return;
  const auditText = result.audit_scope === "Included"
    ? "Included · unterstützter RawRead-Audit-Prefix"
    : "Excluded · Audit-Historie nicht enthalten";
  migrationEntry(backupResult, "Profil", result.profile);
  migrationEntry(backupResult, "Auditumfang", auditText);
  if (result.action === "restored") {
    migrationEntry(backupResult, "Restoremodus", "Neuer Klon; Quellprojekt nicht geändert");
    migrationEntry(backupResult, "Quelle", `${result.source_database_id} · Revision ${result.source_revision}`);
    migrationEntry(backupResult, "Neuer Klon", `${result.restored_database_id} · Revision ${result.restored_revision}`);
    migrationEntry(backupResult, "Audit-Wasserstand", result.audit_safe_sequence ?? (result.audit_scope === "Included" ? "Kein Audit-Wasserstand aufgezeichnet" : "Ausgeschlossen"));
    migrationEntry(backupResult, "Unabhängige Verify-Prüfung", `${result.clone_verify_disposition ?? "unbekannt"} · Ziel verifiziert: ${result.target_verified ? "Ja" : "Nein"}`);
  } else {
    migrationEntry(backupResult, "Datenbank", result.database_id ?? "–");
    migrationEntry(backupResult, "Revision und Inhalt", `Revision ${result.revision ?? "–"} · ${result.item_count} Elemente`);
    migrationEntry(backupResult, "Audit-Wasserstand", result.audit_safe_sequence ?? (result.audit_scope === "Included" ? "Kein Audit-Wasserstand aufgezeichnet" : "Ausgeschlossen"));
    migrationEntry(backupResult, "Backup-Verify", result.target_verified ? "Bestanden" : "Nicht bestätigt");
    migrationEntry(backupResult, "Authentizitätsstatus", result.authenticity);
  }
  migrationEntry(backupResult, "Zielordner", result.folder_name);
  migrationEntry(backupResult, "Quellprojekt verändert", result.source_modified ? "Ja" : "Nein");
}

async function runBackupAction(action) {
  if (!sessionId || projectOpen || backupBusy || exportImportBusy || migrationBusy || recoveryBusy) return;
  const profile = backupProfile.value;
  const profileName = backupProfileLabel(profile);
  if (action === "restore" && !window.confirm(
    `Backup als ${profileName} wiederherstellen?\n\nWorldDB verlangt ein sauberes Autorisierungsprojekt mit derselben Datenbank-ID, prüft das Backup vor dem Restore und erstellt ausschließlich einen neuen Klon in einem neuen Zielordner. Das Quellprojekt bleibt unverändert.`,
  )) return;

  backupBusy = true;
  backupResult.hidden = true;
  backupStatus.textContent = action === "create"
    ? `Wähle nacheinander das Quellprojekt und den Elternordner für ${profileName} …`
    : action === "verify"
      ? `Wähle ein vorhandenes ${profileName} zur Prüfung …`
      : `Wähle das saubere Autorisierungsprojekt, das ${profileName} und den Elternordner für den neuen Klon …`;
  updateProjectControls();
  try {
    const command = action === "create"
      ? "create_backup"
      : action === "verify"
        ? "verify_backup"
        : "restore_backup";
    const response = await invoke(command, {
      sessionId,
      request: { protocol_version: 1, profile },
    });
    const expectedAction = action === "create" ? "created" : action === "verify" ? "verified" : "restored";
    if (response.protocol_version !== 1 || response.result?.action !== expectedAction) {
      throw new Error("unsupported_protocol");
    }
    renderBackupResult(response.result);
    backupStatus.textContent = action === "create"
      ? `${profileName} wurde erstellt und unabhängig geprüft.`
      : action === "verify"
        ? `${profileName} wurde geprüft; Profil, Auditumfang und Zielinventar stimmen.`
        : `Restore als neuer Klon abgeschlossen und unabhängig geprüft. Die Quell-Datenbank blieb unverändert.`;
  } catch (error) {
    backupStatus.textContent = backupErrorText(error);
  } finally {
    backupBusy = false;
    updateProjectControls();
  }
}

function selectedRecordClasses() {
  return [...exportRecordClasses.querySelectorAll("input[data-export-class]:checked")]
    .map((input) => input.value);
}

function selectedHistorySpaces() {
  return exportHistorySpaces.value
    .split(/\r?\n/u)
    .map((value) => value.trim())
    .filter(Boolean);
}

function isCanonicalRevisionInput(value) {
  if (!/^(0|[1-9][0-9]{0,19})$/u.test(value)) return false;
  try {
    return BigInt(value) <= 18446744073709551615n;
  } catch {
    return false;
  }
}

function importMappingLines() {
  return importRemappings.value
    .split(/\r?\n/u)
    .map((value) => value.trim())
    .filter(Boolean);
}

function updateExportImportControls() {
  if (!exportImportPanel) return;
  const blocked = exportImportBusy || backupBusy || migrationBusy || recoveryBusy || projectBusy || projectOpen || !sessionId;
  const historySpaces = selectedHistorySpaces();
  const recordClasses = selectedRecordClasses();
  const canonicalUuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/u;
  let revisionsValid = isCanonicalRevisionInput(exportFromRevision.value)
    && isCanonicalRevisionInput(exportThroughRevision.value);
  if (revisionsValid) revisionsValid = BigInt(exportFromRevision.value) <= BigInt(exportThroughRevision.value);
  const historySpacesValid = historySpaces.length > 0
    && historySpaces.length <= 65536
    && historySpaces.every((value) => canonicalUuid.test(value))
    && new Set(historySpaces).size === historySpaces.length;
  const classesValid = recordClasses.length > 0
    && new Set(recordClasses).size === recordClasses.length
    && (exportKind.value !== "logical" || recordClasses.includes("HistorySpaceDefinition"));
  const mappings = importMappingLines();
  const mappingsValid = mappings.length <= 4096
    && mappings.every((mapping) => mapping.length <= 256
      && (mapping.match(/=/gu) ?? []).length === 1
      && !/[\u0000-\u001f]/u.test(mapping));
  exportRunButton.disabled = blocked || !revisionsValid || !historySpacesValid || !classesValid;
  importPlanButton.disabled = blocked || !mappingsValid;
  importPrepareButton.disabled = blocked;
  updatePurgeControls();
}

function purgeTargetLines() {
  return purgeTargets.value.split(/\r?\n/u).map((value) => value.trim()).filter(Boolean);
}

function purgeKnownCopyLines() {
  return purgeKnownCopies.value.split(/\r?\n/u).map((value) => value.trim()).filter(Boolean);
}

function purgeRequestFromInputs() {
  return {
    protocol_version: 1,
    targets: purgeTargetLines(),
    mode: purgeMode.value,
    external_inventory_complete: purgeExternalComplete.value === "complete",
    known_external_artifacts: purgeKnownCopyLines(),
  };
}

function purgeRequestIsValid(request) {
  const uuid = "[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}";
  const families = "history-space|layer|perspective|timeline|entity|entity-type|predicate|event-kind|event-role|event-attribute";
  const familyTargetPattern = new RegExp(`^(?:${families}):${uuid}$`, "u");
  const recordTargetPattern = new RegExp(`^record:(0|[1-9][0-9]{0,9}):${uuid}$`, "u");
  const copyPattern = /^(?:exact-backup|audit-complete-backup|logical-export|sharing-export):[0-9a-f]{64}$/u;
  const targetsValid = request.targets.every((value) => {
    if (familyTargetPattern.test(value)) return value.length <= 128;
    const match = recordTargetPattern.exec(value);
    return Boolean(match) && BigInt(match[1]) <= 4294967295n && value.length <= 128;
  });
  return request.targets.length > 0 && request.targets.length <= 4096
    && targetsValid
    && new Set(request.targets).size === request.targets.length
    && ["reject-if-referenced", "cascade"].includes(request.mode)
    && request.known_external_artifacts.length <= 4096
    && request.known_external_artifacts.every((value) => copyPattern.test(value))
    && new Set(request.known_external_artifacts).size === request.known_external_artifacts.length;
}

function purgeInputsMatchPlan() {
  if (!currentPurgeRequest) return false;
  return JSON.stringify(purgeRequestFromInputs()) === JSON.stringify(currentPurgeRequest);
}

function updatePurgeControls() {
  if (!purgePanel) return;
  const blocked = purgeBusy || backupBusy || exportImportBusy || migrationBusy || Boolean(currentMigrationState?.plan) || recoveryBusy || projectBusy || projectOpen || !sessionId;
  const requestValid = purgeRequestIsValid(purgeRequestFromInputs());
  const planMatchesInputs = Boolean(currentPurgePlan) && purgeInputsMatchPlan();
  purgePreviewButton.disabled = blocked || !requestValid;
  purgeExecuteButton.hidden = !currentPurgePlan;
  purgeExecuteButton.disabled = blocked || !currentPurgePlan?.approval_possible || !planMatchesInputs;
  purgeDiscardButton.hidden = !currentPurgePlan;
  purgeDiscardButton.disabled = blocked || !currentPurgePlan;
  if (currentPurgePlan && !planMatchesInputs && !purgeBusy) {
    purgeStatus.textContent = "Die Eingaben weichen vom angezeigten Plan ab. Erstelle den Plan erneut, bevor du ihn ausführen kannst.";
  }
}

function appendPurgeList(container, label, values, remaining = 0) {
  const column = document.createElement("div");
  column.className = "result-cell";
  const heading = document.createElement("strong");
  heading.textContent = label;
  const list = document.createElement("ul");
  if (values.length === 0) {
    const item = document.createElement("li");
    item.textContent = "Keine";
    list.append(item);
  } else {
    for (const value of values) {
      const item = document.createElement("li");
      item.textContent = value;
      list.append(item);
    }
  }
  column.append(heading, list);
  if (remaining > 0) {
    const note = document.createElement("p");
    note.className = "muted";
    note.textContent = `${remaining} weitere Einträge stehen im vollständigen Planbericht.`;
    column.append(note);
  }
  container.append(column);
}

function purgeErrorText(error) {
  return showError(error);
}

function renderPurgePlan(plan) {
  if (plan?.action !== "previewed" || plan.writes_database !== false || plan.source_modified !== false
    || plan.secure_erase_claimed !== false || plan.index_inventory_complete !== true
    || typeof plan.approval_possible !== "boolean" || !/^[0-9a-f]{64}$/u.test(plan.plan_fingerprint)
    || !/^[0-9a-f]{64}$/u.test(plan.report_digest)) {
    throw new Error("Der Purgeplan ist unvollständig oder meldet unerwartete Schreibvorgänge.");
  }
  purgeResult.replaceChildren();
  purgeResult.hidden = false;
  migrationEntry(purgeResult, "Quelle und Stand", `${plan.source_database_id} · Revision ${plan.source_revision}`);
  migrationEntry(purgeResult, "Umfang", `${plan.target_count} Ziele · ${plan.target_record_count} Zielrecords · ${plan.dependant_count} abhängige Records · ${plan.affected_record_count} betroffene Records`);
  migrationEntry(purgeResult, "Referenzbehandlung", plan.mode === "cascade" ? "Abhängige Records werden einbezogen" : "Plan bricht bei referenzierten Zielen ab");
  migrationEntry(purgeResult, "Ausführbarkeit", plan.approval_possible ? "Plan kann ausdrücklich bestätigt werden" : "Plan ist nicht ausführbar; siehe Inventar- und Referenzbefunde");
  migrationEntry(purgeResult, "Externe Kopien", `${plan.external_inventory_complete ? "Vollständig geprüft" : "Unvollständig"} · ${plan.known_external_artifacts.length} bekannte Kopien`);
  migrationEntry(purgeResult, "Indexbestand", `Vollständig geprüft · ${plan.index_generation_count} Generationen · Neu aufzubauen: ${plan.index_families_to_rebuild.join(", ") || "keine"}`);
  migrationEntry(purgeResult, "Neue Zieldatenbank", plan.destination_name);
  migrationEntry(purgeResult, "Vollständiger Bericht", `${plan.report_name} · BLAKE3 ${plan.report_digest}`);
  migrationEntry(purgeResult, "Plan-Fingerprint", plan.plan_fingerprint);
  migrationEntry(purgeResult, "Vorschau", "Die Quelldatenbank wurde nicht verändert. Sichere physische Löschung wird nicht zugesagt.");
  appendPurgeList(purgeResult, "Ziel-IDs", plan.target_identities, Number(plan.target_identities_remaining));
  appendPurgeList(purgeResult, "Zielrecords", plan.target_records.map((item) => `${item.identity} · ${item.content_digest}`), Number(plan.target_records_remaining));
  appendPurgeList(purgeResult, "Abhängige Records", plan.dependants.map((item) => `${item.identity} · ${item.content_digest}`), Number(plan.dependants_remaining));
  appendPurgeList(purgeResult, "Lokale Indexgenerationen", plan.index_generations.map((item) => `${item.family} · ${item.generation_id} · ${item.file_digest} · ${item.current ? "aktuell" : "historisch"}`), Number(plan.index_generations_remaining));
  appendPurgeList(purgeResult, "Bekannte externe Artefakte", plan.known_external_artifacts.slice(0, 128).map((item) => `${item.kind}:${item.digest}`), Math.max(0, plan.known_external_artifacts.length - 128));
}

function renderPurgeRun(result) {
  if (result?.action !== "completed" || !result.source_database_id || !result.destination_database_id
    || result.source_database_id === result.destination_database_id || !result.target_verified
    || !result.report_persisted || result.source_modified || result.secure_erase_claimed) {
    throw new Error("Das Purge-Ergebnis bestätigt die getrennte geprüfte Zieldatenbank nicht.");
  }
  currentPurgePlan = null;
  currentPurgeRequest = null;
  purgeResult.replaceChildren();
  purgeResult.hidden = false;
  migrationEntry(purgeResult, "Aktion", "Purge in eine neue, getrennte Datenbank abgeschlossen");
  migrationEntry(purgeResult, "Unveränderte Quelle", `${result.source_database_id} · Revision ${result.source_revision}`);
  migrationEntry(purgeResult, "Neue Zieldatenbank", `${result.destination_database_id} · Revision ${result.destination_revision} · ${result.destination_name}`);
  migrationEntry(purgeResult, "Ausführung", `${result.removed_record_count} entfernte Records · Modus ${result.mode}`);
  migrationEntry(purgeResult, "Prüfung und Bericht", `Ziel verifiziert: Ja · Bericht dauerhaft gespeichert: Ja · BLAKE3 ${result.report_digest}`);
  migrationEntry(purgeResult, "Auditbezug", `Operation ${result.operation_id} · Auditrecord ${result.audit_record_id}`);
  migrationEntry(purgeResult, "Externe Kopien", result.external_inventory_complete ? "Inventar als vollständig bestätigt" : "Inventar ausdrücklich unvollständig");
  migrationEntry(purgeResult, "Quelle und Löschgrenze", "Quelle unverändert · sichere physische Löschung wird nicht zugesagt.");
}

function initializeExportClassChoices() {
  exportRecordClasses.replaceChildren();
  for (const name of EXPORTABLE_RECORD_CLASSES) {
    const label = document.createElement("label");
    label.className = "inline";
    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    checkbox.value = name;
    checkbox.dataset.exportClass = "true";
    checkbox.checked = name === "HistorySpaceDefinition";
    const text = document.createElement("span");
    text.textContent = name;
    label.append(checkbox, text);
    exportRecordClasses.append(label);
  }
}

function exportImportErrorText(error) {
  return showError(error);
}

function renderExportImportResult(result) {
  exportImportResult.replaceChildren();
  exportImportResult.hidden = !result;
  if (!result) return;
  if (result.action === "completed") {
    if (result.format === "SharingExport") {
      if (result.omission_counts_disclosed !== false || result.omission_manifest || !result.source_audit_committed) {
        throw new Error("Sharing-Export-Ergebnis verletzt den geschlossenen Umfangsvertrag.");
      }
      migrationEntry(exportImportResult, "Exportformat", "Teilen-Export · keine exakte Sicherung");
      migrationEntry(exportImportResult, "Scope", `Revisionen ${result.from_revision}–${result.through_revision} · ${result.history_spaces.length} HistorySpaces · ${result.record_classes.length} angeforderte Recordklassen`);
      migrationEntry(exportImportResult, "Enthaltene Records", result.included_record_count ?? "unbekannt");
      migrationEntry(exportImportResult, "Ausgelassene Mengen", "Werden im Teilen-Export nicht offengelegt.");
      migrationEntry(exportImportResult, "Audit", "Autorisierung und Abschluss wurden im Quellprojekt dauerhaft protokolliert.");
      migrationEntry(exportImportResult, "Quellprojekt", result.source_modified ? "Durch die Auditnachweise geändert" : "Nicht geändert");
    } else {
      if (result.format !== "LogicalExport" || !result.omission_manifest?.complete || result.source_modified) {
        throw new Error("Logical-Export-Ergebnis ist unvollständig oder unerwartet.");
      }
      migrationEntry(exportImportResult, "Exportformat", "Logical Export · kein Exact Backup");
      migrationEntry(exportImportResult, "Quelle und Snapshot", `${result.database_id ?? "–"} · Revision ${result.snapshot_revision ?? "–"}`);
      migrationEntry(exportImportResult, "Scope", `Revisionen ${result.from_revision}–${result.through_revision} · ${result.history_spaces.length} HistorySpaces · ${result.record_classes.length} Recordklassen`);
      migrationEntry(exportImportResult, "Enthaltene Records", result.record_count ?? "unbekannt");
      migrationEntry(exportImportResult, "Vollständiges Auslassmanifest", `${result.omission_manifest.record_classes_omitted} nicht ausgewählte Recordklassen · ${result.omission_manifest.storage_classes_omitted} ausgelassene Storageklassen`);
      migrationEntry(exportImportResult, "Quellprojekt", "Nicht geändert");
    }
    migrationEntry(exportImportResult, "Artefakt", result.output_name);
    return;
  }
  if (result.action === "plan_created") {
    if (result.writes_database !== false) throw new Error("Die Planerstellung meldete unerwartete Datenbankschreibvorgänge.");
    migrationEntry(exportImportResult, "Aktion", "Kanonischer Importplan erstellt; Zieldatenbank nicht geändert.");
    migrationEntry(exportImportResult, "Quelle → Ziel", `${result.source_database_id} → ${result.destination_database_id}`);
    migrationEntry(exportImportResult, "ID-Remaps", result.mapping_count);
    migrationEntry(exportImportResult, "Plan-Digest", result.plan_digest);
    migrationEntry(exportImportResult, "Plan-Datei", result.plan_name);
    return;
  }
  if (result.action === "prepared") {
    if (result.writes_database !== false || !result.omission_manifest?.complete) {
      throw new Error("Prepare meldete einen unerwarteten Schreibvorgang oder ein unvollständiges Manifest.");
    }
    migrationEntry(exportImportResult, "Aktion", "Import vorbereitet und gegen Zielbestand geprüft; es wurden keine Records geschrieben.");
    migrationEntry(exportImportResult, "Quelle → Ziel", `${result.source_database_id} → ${result.destination_database_id}`);
    migrationEntry(exportImportResult, "Zu übertragende Records", result.record_count);
    migrationEntry(exportImportResult, "ID-Remaps", result.mapping_count);
    migrationEntry(exportImportResult, "Scope", `Revisionen ${result.from_revision}–${result.through_revision} · ${result.history_space_count} HistorySpaces · ${result.record_class_count} Recordklassen`);
    migrationEntry(exportImportResult, "Vollständiges Auslassmanifest", `${result.omission_manifest.record_classes_omitted} nicht ausgewählte Recordklassen · ${result.omission_manifest.storage_classes_omitted} ausgelassene Storageklassen`);
    migrationEntry(exportImportResult, "Stream-Fingerprint", result.stream_fingerprint);
    return;
  }
  throw new Error("Unbekanntes Export-/Import-Ergebnis.");
}

async function runExportAction() {
  if (!sessionId || projectOpen || exportImportBusy || backupBusy || migrationBusy || recoveryBusy) return;
  const request = {
    protocol_version: 1,
    kind: exportKind.value,
    from_revision: exportFromRevision.value.trim(),
    through_revision: exportThroughRevision.value.trim(),
    history_spaces: selectedHistorySpaces(),
    record_classes: selectedRecordClasses(),
  };
  exportImportBusy = true;
  exportImportResult.hidden = true;
  exportImportStatus.textContent = "Wähle im nativen Dialog das Quellprojekt und anschließend den neuen Speicherort für das Exportartefakt …";
  updateProjectControls();
  try {
    const response = await invoke("export_data", { sessionId, request });
    if (response.protocol_version !== 1 || response.result?.action !== "completed") {
      throw new Error("unsupported_protocol");
    }
    renderExportImportResult(response.result);
    exportImportStatus.textContent = response.result.format === "SharingExport"
      ? "Teilen-Export erstellt. Er ist keine Sicherung; das Quellprojekt enthält die erforderlichen Auditnachweise."
      : "Logical Export erstellt. Das vollständige Scope- und Auslassmanifest ist im Artefakt enthalten.";
  } catch (error) {
    exportImportStatus.textContent = exportImportErrorText(error);
  } finally {
    exportImportBusy = false;
    updateProjectControls();
  }
}

async function runImportPlanAction() {
  if (!sessionId || projectOpen || exportImportBusy || backupBusy || migrationBusy || recoveryBusy) return;
  const request = { protocol_version: 1, mappings: importMappingLines() };
  exportImportBusy = true;
  exportImportResult.hidden = true;
  exportImportStatus.textContent = "Wähle das Zielprojekt, das Logical-Export-Artefakt und den neuen Speicherort für den Importplan …";
  updateProjectControls();
  try {
    const response = await invoke("create_import_plan", { sessionId, request });
    if (response.protocol_version !== 1 || response.result?.action !== "plan_created") {
      throw new Error("unsupported_protocol");
    }
    renderExportImportResult(response.result);
    exportImportStatus.textContent = "Importplan erstellt. Die Zieldatenbank wurde nicht geändert; führe danach Prepare aus, um Zielbestand und Referenzen zu prüfen.";
  } catch (error) {
    exportImportStatus.textContent = exportImportErrorText(error);
  } finally {
    exportImportBusy = false;
    updateProjectControls();
  }
}

async function runImportPrepareAction() {
  if (!sessionId || projectOpen || exportImportBusy || backupBusy || migrationBusy || recoveryBusy) return;
  exportImportBusy = true;
  exportImportResult.hidden = true;
  exportImportStatus.textContent = "Wähle das Zielprojekt, dasselbe Logical-Export-Artefakt und den zugehörigen kanonischen Importplan …";
  updateProjectControls();
  try {
    const response = await invoke("prepare_import", { sessionId, request: { protocol_version: 1 } });
    if (response.protocol_version !== 1 || response.result?.action !== "prepared") {
      throw new Error("unsupported_protocol");
    }
    renderExportImportResult(response.result);
    exportImportStatus.textContent = "Prepare bestanden. Es wurden keine Records in die Zieldatenbank geschrieben.";
  } catch (error) {
    exportImportStatus.textContent = exportImportErrorText(error);
  } finally {
    exportImportBusy = false;
    updateProjectControls();
  }
}

async function runPurgePreview() {
  if (!sessionId || projectOpen || purgeBusy || backupBusy || exportImportBusy || migrationBusy || currentMigrationState?.plan || recoveryBusy) return;
  const request = purgeRequestFromInputs();
  if (!purgeRequestIsValid(request)) {
    purgeStatus.textContent = "Prüfe Ziel-IDs, Duplikate und bekannte Kopien. IDs brauchen ein unterstütztes Präfix und eine kleingeschriebene UUID.";
    return;
  }
  purgeBusy = true;
  currentPurgePlan = null;
  currentPurgeRequest = null;
  purgeResult.hidden = true;
  purgeStatus.textContent = "Wähle nacheinander das Quellprojekt, den Elternordner für die neue Datenbank und den Speicherort für den vollständigen Planbericht …";
  updateProjectControls();
  try {
    const response = await invoke("preview_purge", { sessionId, request });
    if (response.protocol_version !== 1 || response.result?.action !== "previewed") throw new Error("unsupported_protocol");
    renderPurgePlan(response.result);
    currentPurgeRequest = request;
    currentPurgePlan = response.result;
    purgeStatus.textContent = response.result.approval_possible
      ? "Plan und vollständiger Bericht geprüft. Die Ausführung erstellt erst nach Bestätigung eine neue Datenbank; die Quelle bleibt unverändert."
      : "Plan geprüft, aber nicht ausführbar. Korrigiere Zielumfang oder Inventar und erstelle einen neuen Plan.";
  } catch (error) {
    purgeStatus.textContent = purgeErrorText(error);
  } finally {
    purgeBusy = false;
    updateProjectControls();
  }
}

async function runPurgeExecution() {
  const plan = currentPurgePlan;
  if (!sessionId || projectOpen || purgeBusy || backupBusy || exportImportBusy || migrationBusy || currentMigrationState?.plan || recoveryBusy
    || !plan?.approval_possible || !purgeInputsMatchPlan()) return;
  const confirmation = `Den geprüften Purgeplan ausdrücklich ausführen?\n\n${plan.target_count} Ziele, ${plan.target_record_count} Zielrecords und ${plan.dependant_count} abhängige Records · Modus ${plan.mode}.\n\nWorldDB erstellt ${plan.destination_name} als neue Datenbank mit eigener DatabaseId. Die Quelle ${plan.source_database_id} bleibt unverändert. Sichere physische Löschung wird nicht zugesagt.\n\nPlan-Fingerprint: ${plan.plan_fingerprint}`;
  if (!window.confirm(confirmation)) return;
  purgeBusy = true;
  purgeStatus.textContent = "Der Host vergleicht den Quellstand und Plan-Fingerprint erneut, erstellt die getrennte Zieldatenbank und prüft sie unabhängig …";
  updateProjectControls();
  try {
    const response = await invoke("execute_purge", {
      sessionId,
      request: { protocol_version: 1, plan_fingerprint: plan.plan_fingerprint },
    });
    if (response.protocol_version !== 1 || response.result?.action !== "completed") throw new Error("unsupported_protocol");
    renderPurgeRun(response.result);
    purgeStatus.textContent = "Purge abgeschlossen. Die neue DatabaseId und die unveränderte Quelle sind oben getrennt aufgeführt.";
  } catch (error) {
    currentPurgePlan = null;
    currentPurgeRequest = null;
    purgeStatus.textContent = `${purgeErrorText(error)} Erstelle vor einem weiteren Versuch einen neuen Plan.`;
  } finally {
    purgeBusy = false;
    updateProjectControls();
  }
}

async function discardPurgePlan() {
  if (!sessionId || purgeBusy || migrationBusy || currentMigrationState?.plan || !currentPurgePlan) return;
  purgeBusy = true;
  updateProjectControls();
  try {
    const response = await invoke("discard_purge_plan", {
      sessionId,
      request: { protocol_version: 1 },
    });
    if (response.protocol_version !== 1) throw new Error("unsupported_protocol");
    currentPurgePlan = null;
    currentPurgeRequest = null;
    purgeResult.replaceChildren();
    purgeResult.hidden = true;
    purgeStatus.textContent = "Purgeplan und lokale Vorschau wurden verworfen. Es wurde keine Datenbank verändert.";
  } catch (error) {
    purgeStatus.textContent = purgeErrorText(error);
  } finally {
    purgeBusy = false;
    updateProjectControls();
  }
}

function migrationCategoryText(category) {
  return ({
    MetadataOnly: "Metadatenänderung",
    Additive: "Erweiterung",
    CompatibleConstraintChange: "Kompatible Constraintänderung",
    Restrictive: "Restriktive Migration",
    Breaking: "Breaking-Migration",
  })[category] ?? category ?? "–";
}

function migrationPhaseText(phase) {
  return ({
    preview_only_no_commit: "Vorschau; kein Commit erfolgt",
    committed: "Migration veröffentlicht",
  })[phase] ?? phase ?? "–";
}

function migrationRestorepointText(status) {
  return ({
    required_before_run_not_created: "Vor dem Start erforderlich; noch nicht erstellt",
    created_and_verified: "Exakte Sicherung und Restore-Klon erstellt und geprüft",
    not_required: "Für diese Kategorie nicht erforderlich",
  })[status] ?? status ?? "–";
}

function renderMigrationPlan(plan) {
  migrationPlanSummary.replaceChildren();
  migrationPlanSummary.hidden = !plan;
  if (!plan) return;
  migrationEntry(migrationPlanSummary, "Kategorie", migrationCategoryText(plan.category));
  migrationEntry(migrationPlanSummary, "Migration und Datenbank", `${plan.migration_id} · ${plan.database_id}`);
  migrationEntry(migrationPlanSummary, "Schema", `Revision ${plan.source_revision} → ${plan.target_revision}`);
  migrationEntry(migrationPlanSummary, "Schritte", plan.step_ids.join(", ") || "Keine");
  migrationEntry(migrationPlanSummary, "Transformer und Fingerprint", `v${plan.transformer_version} · ${plan.plan_fingerprint}`);
  migrationEntry(migrationPlanSummary, "Ressourcenlimit", `${plan.max_work_units} Arbeitseinheiten · ${plan.max_memory_bytes} Byte Speicher`);
  migrationEntry(migrationPlanSummary, "Adminaktion", plan.breaking_confirmation_required
    ? "Breaking erfordert eine getrennte Bestätigung."
    : "Keine Breaking-Bestätigung erforderlich.");
  migrationEntry(migrationPlanSummary, "Restorepoint", plan.restorepoint_required
    ? "Vor einer Breaking-Ausführung werden exakte Sicherung und geprüfter Restore-Klon verlangt."
    : "Für diese Kategorie nicht erforderlich.");
}

function renderMigrationPreview(preview) {
  migrationPreviewSummary.replaceChildren();
  migrationPreviewSummary.hidden = !preview;
  migrationRunButton.hidden = !preview || currentMigrationState?.run_attempted;
  if (!preview) return;
  const hasErrors = preview.error_count !== "0" || preview.omitted_error_count !== "0" || Boolean(preview.fatal_error);
  const hasUnlistedItems = preview.omitted_unresolved_count !== "0";
  const hasUnresolvedItems = preview.unresolved_items.length > 0;
  const resultText = hasErrors || hasUnlistedItems
    ? "Dry Run hat Fehler oder wegen des Diagnosebudgets nicht vollständig angezeigte Einträge; Ausführung bleibt gesperrt."
    : currentMigrationState?.can_execute
      ? hasUnresolvedItems
        ? "Alle ungelösten Einträge wurden ausdrücklich zum Auslassen markiert. Es wurde noch nichts veröffentlicht."
        : "Dry Run vollständig; Vorschau hat keine Daten veröffentlicht."
      : hasUnresolvedItems
        ? "Dry Run gefunden. Entscheide ausdrücklich, welche ungelösten Quelldatensätze ausgelassen werden sollen."
        : "Dry Run unvollständig; Ausführung bleibt gesperrt.";
  migrationEntry(migrationPreviewSummary, "Ergebnis", resultText);
  migrationEntry(migrationPreviewSummary, "Eingabe", `${preview.source_record_count} Records · ${preview.input_bytes} Byte`);
  migrationEntry(migrationPreviewSummary, "Platzschätzung", `${preview.estimated_output_records ?? "–"} Records · ${preview.estimated_output_bytes ?? "–"} Byte Ausgabe · ${preview.reserved_memory_bytes ?? "–"} Byte reservierter Speicher · ${preview.diagnostic_memory_bytes} Byte Diagnosespeicher`);
  migrationEntry(migrationPreviewSummary, "Befunde", `${preview.error_count} Fehler (${preview.omitted_error_count} weitere Details ausgelassen) · ${preview.unresolved_items.length + Number(preview.omitted_unresolved_count)} ungelöste Einträge`);
  migrationEntry(migrationPreviewSummary, "Vorschaufingerprints", `Eingabe ${preview.input_fingerprint ?? "nicht verfügbar"} · Ausgabe ${preview.output_fingerprint ?? "nicht verfügbar"}`);
  migrationEntry(migrationPreviewSummary, "Contractphase", migrationPhaseText(preview.contract_phase));
  migrationEntry(migrationPreviewSummary, "Restorepointstatus", migrationRestorepointText(preview.restorepoint_status));
  if (preview.fatal_error) migrationEntry(migrationPreviewSummary, "Vorabprüfung", `Abgebrochen: ${preview.fatal_error}`);
  for (const item of preview.errors) {
    migrationEntry(migrationPreviewSummary, `Fehler in Record ${item.record_index}`, item.cause);
  }
  for (const item of preview.unresolved_items) {
    const entry = document.createElement("div");
    entry.className = "result-cell";
    const heading = document.createElement("strong");
    heading.textContent = `Ungelöster Record ${item.record_index}`;
    const detail = document.createElement("p");
    detail.textContent = `${item.reason} · ${item.source_record_fingerprint}`;
    const choice = document.createElement("label");
    choice.className = "inline";
    const omit = document.createElement("input");
    omit.type = "checkbox";
    omit.dataset.migrationOmit = "true";
    omit.value = item.record_index;
    omit.checked = currentMigrationState?.omitted_record_indexes?.includes(item.record_index) ?? false;
    omit.disabled = migrationBusy || currentMigrationState?.run_attempted === true || hasUnlistedItems;
    omit.addEventListener("change", saveMigrationOmissions);
    const choiceText = document.createElement("span");
    choiceText.textContent = "Diesen Quelldatensatz ausdrücklich auslassen";
    choice.append(omit, choiceText);
    entry.append(heading, detail, choice);
    migrationPreviewSummary.append(entry);
  }
  if (preview.omitted_unresolved_count !== "0") {
    migrationEntry(migrationPreviewSummary, "Weitere ungelöste Einträge", preview.omitted_unresolved_count);
  }
  for (const warning of preview.warnings) {
    migrationEntry(migrationPreviewSummary, "Hinweis", warning);
  }
}

function renderMigrationState(state) {
  currentMigrationState = state;
  renderMigrationPlan(state?.plan ?? null);
  renderMigrationPreview(state?.dry_run ?? null);
  updateProjectControls();
}

function updateMigrationControls() {
  const state = currentMigrationState;
  const blocked = migrationBusy || backupBusy || exportImportBusy || purgeBusy || projectBusy || projectOpen;
  const otherBusy = projectBusy || backupBusy || exportImportBusy || purgeBusy || schemaBusy || entityBusy || perspectiveBusy
    || securityPolicyBusy || branchLayerBusy || transferBusy || factBusy;
  const hasUnresolvedProjectCreation = projectCreationJournalUnavailable
    || loadPendingProjectCreations().length > 0;
  const migrationSelected = migrationBusy || Boolean(state?.plan);
  createButton.disabled = otherBusy || projectOpen || hasUnresolvedProjectCreation || migrationSelected;
  openButton.disabled = otherBusy || projectOpen || migrationSelected;
  migrationSelectPlanButton.disabled = blocked || state?.run_attempted === true;
  migrationPreviewButton.disabled = blocked || !state?.plan || state.run_attempted === true;
  migrationCancelButton.hidden = !state;
  migrationCancelButton.disabled = blocked || state?.run_attempted === true;
  migrationRunButton.hidden = !state?.dry_run || state.run_attempted === true;
  migrationRunButton.disabled = blocked || state?.can_execute !== true;
  migrationResumeButton.hidden = state?.can_resume !== true;
  migrationResumeButton.disabled = blocked || state?.can_resume !== true;
  updateExportImportControls();
}

async function refreshMigrationState(activeSessionId = sessionId) {
  if (!activeSessionId) return;
  const response = await invoke("migration_status", {
    sessionId: activeSessionId,
    request: { protocol_version: 1 },
  });
  if (response.protocol_version !== 1) throw new Error("unsupported_protocol");
  renderMigrationState(response.state);
}

function migrationErrorText(error) {
  return showError(error);
}

async function saveMigrationOmissions() {
  if (!sessionId || migrationBusy || currentMigrationState?.run_attempted) return;
  const omittedRecordIndexes = [...migrationPreviewSummary.querySelectorAll("input[data-migration-omit]:checked")]
    .map((input) => input.value);
  migrationBusy = true;
  migrationStatus.textContent = "Die ausgewählten Adminentscheidungen werden an den unveränderten Dry Run gebunden …";
  updateMigrationControls();
  try {
    const response = await invoke("resolve_migration_items", {
      sessionId,
      request: { protocol_version: 1, omitted_record_indexes: omittedRecordIndexes },
    });
    if (response.protocol_version !== 1) throw new Error("unsupported_protocol");
    renderMigrationState(response.state);
    migrationStatus.textContent = currentMigrationState?.can_execute
      ? "Alle ungelösten Einträge sind ausdrücklich entschieden. Prüfe die Vorschau und starte die Migration bei Bedarf separat."
      : "Die Entscheidungen sind gespeichert; weitere ungelöste Einträge benötigen noch eine ausdrückliche Auswahl.";
  } catch (error) {
    migrationStatus.textContent = migrationErrorText(error);
    await refreshMigrationState(sessionId).catch(() => {});
  } finally {
    migrationBusy = false;
    updateMigrationControls();
  }
}

function setBusy(busy) {
  projectBusy = busy;
  updateSchemaControls();
  updateMigrationControls();
  updateBackupControls();
  updateExportImportControls();
}

function renderProject(project) {
  projectOpen = Boolean(project.project_open);
  const databaseId = projectOpen ? project.database_id ?? null : null;
  if (databaseId !== currentDatabaseId) {
    currentDatabaseId = databaseId;
    operationJournalUnavailable = false;
    pendingOperations = databaseId ? loadPendingOperations(databaseId) : [];
    if (operationJournalUnavailable) {
      operationStatus.textContent = "Der vorgemerkte Schreibstatus kann nicht sicher gelesen werden. Schreibaktionen bleiben gesperrt.";
    }
    reconcileOperationsButton.hidden = !hasUnresolvedOperation();
    reconcileOperationsButton.disabled = !databaseId || operationJournalUnavailable;
  }
  const pendingProjectCreations = loadPendingProjectCreations();
  if (projectCreationJournalUnavailable) {
    operationStatus.textContent = "Der Status einer möglichen Projektanlage kann nicht sicher gelesen werden. Neue Projektanlagen bleiben gesperrt.";
  } else if (!projectOpen && pendingProjectCreations.length > 0) {
    operationStatus.textContent = `Eine Projektanlage mit Operation ${pendingProjectCreations[0].operation_id} wartet auf den Abgleich. Öffne das betroffene Projekt, um seinen WAL-Status zu prüfen.`;
  }
  schemaPanel.hidden = !projectOpen;
  entityPanel.hidden = !projectOpen;
  securityPolicyPanel.hidden = !projectOpen;
  perspectivePanel.hidden = !projectOpen;
  branchLayerPanel.hidden = !projectOpen;
  transferPanel.hidden = !projectOpen;
  factsPanel.hidden = !projectOpen;
  jobsPanel.hidden = !projectOpen;
  diagnosticPanel.hidden = !projectOpen;
  recoveryPanel.hidden = projectOpen;
  migrationPanel.hidden = projectOpen;
  backupPanel.hidden = projectOpen;
  exportImportPanel.hidden = projectOpen;
  purgePanel.hidden = projectOpen;
  projectRevision = projectOpen ? project.revision ?? null : null;
  if (!projectOpen) {
    projectStatus.textContent = "Kein Projekt geöffnet";
    projectDetails.textContent = "Lege ein Projekt an oder öffne einen vorhandenen Projektordner.";
    jobsList.replaceChildren();
    jobsStatus.textContent = "Öffne ein Projekt, um den Jobjournalstatus zu lesen.";
    jobsShutdownStatus.textContent = "";
    updateSchemaControls();
    updateProjectControls();
    return;
  }
  projectStatus.textContent = project.project_name ?? "WorldDB-Projekt geöffnet";
  const storageFormatLabels = {
    current_v1: "CURRENT v1",
    current_v2: "CURRENT v2",
  };
  const storageFormat = storageFormatLabels[project.compatibility?.storage_format] ?? "unbekannt";
  projectDetails.textContent = `Rolle: ${project.role ?? "unbekannt"} · Stand: ${project.revision ?? "–"} · Format: ${storageFormat} · Format- und Schemakonvertierungen nur ausdrücklich`;
  updateSchemaControls();
  updateProjectControls();
}

async function getProject(activeSessionId) {
  return invoke("project_status", { sessionId: activeSessionId });
}

const recoveryDispositionText = {
  clean: "sauber geprüft",
  recovery_required: "journalisierte Recovery erforderlich",
  quarantined_read_only: "Quarantäne · read-only",
};
const recoveryDamageText = {
  bitflip: "Digest oder Prüfsumme stimmt nicht",
  truncation: "unvollständiges Dateiende",
  reorder: "ungültige Reihenfolge",
  duplicate_frame: "doppelter Frame",
  semantic_invalidity: "semantische Integritätsverletzung",
  other: "sonstiger Integritätsbefund",
};
const recoveryActionText = {
  preserve_original: "Original für Diagnose unverändert aufbewahren",
  keep_read_only: "Projekt read-only belassen",
  run_journaled_tail_recovery: "WAL-Endstück mit Recovery-Journal behandeln",
  run_journaled_recovery: "Journalisierte Recovery ausdrücklich ausführen",
  restore_verified_backup_to_new_destination: "Verifiziertes Backup in ein neues Ziel wiederherstellen",
  salvage_into_new_database: "Verifizierte Segmente als neues Salvage-Archiv kopieren",
};
const recoveryIssueText = {
  torn_tail: "Ein WAL-Endstück ist unvollständig und zählt nicht zur sicheren Revision.",
  uncommitted_tail: "Ein vollständiger Prepare-Eintrag besitzt keinen Commitmarker.",
  current_manifest_corrupt: "CURRENT oder das referenzierte Manifest ist nicht verifizierbar.",
  manifest_ahead_of_safe_prefix: "Das Manifest beansprucht eine Revision oberhalb des verifizierten WAL-Präfixes.",
  manifest_commit_hash_mismatch: "Der Manifest-Commitbezug stimmt nicht mit der WAL-Hashkette überein.",
  manifest_behind_committed_snapshot: "Das Manifest liegt hinter einem vollständig committed Snapshot.",
  manifest_snapshot_mismatch: "Manifest und committed Snapshot nennen unterschiedliche Segmentinventare.",
  committed_replay_payload_corrupt: "Ein committed Replay-Payload ist nicht vollständig verifizierbar.",
  required_audit_sequence_invalid: "Eine Required-Audit-Sequenz ist doppelt oder nicht fortlaufend.",
  referenced_segment_corrupt: "Ein vom committed Inventar referenziertes Segment ist beschädigt.",
  operation_index_mismatch: "Der OperationId-Index stimmt nicht mit dem vollständig geprüften WAL überein.",
  schema_history_invalid: "Die Schemahistorie konnte nicht vollständig verifiziert werden.",
  capability_history_invalid: "Die Berechtigungshistorie konnte nicht vollständig verifiziert werden.",
  segment_readback_failed: "Ein referenziertes Datensegment konnte beim zweiten Prüflauf nicht verifiziert werden.",
};

function renderRecoveryReport(report) {
  currentRecoveryReport = report;
  recoveryReportElement.replaceChildren();
  recoveryFindings.replaceChildren();
  recoveryReportElement.hidden = false;
  recoveryActions.hidden = !report.read_only;
  recoveryRunButton.hidden = !report.can_run_journaled_recovery;
  recoveryRestoreButton.hidden = !report.can_restore_verified_backup;
  recoverySalvageFields.hidden = !report.can_salvage;
  recoveryOpenCleanButton.hidden = report.disposition !== "clean";

  const summary = document.createElement("div");
  summary.className = "grid";
  const inventory = report.inventory;
  const metrics = [
    ["Sichere Revision", report.safe_revision],
    ["Zustand", recoveryDispositionText[report.disposition] ?? report.disposition],
    ["Schreibstatus", report.read_only ? "read-only" : "schreibbar"],
    ["WAL-Commitframes", inventory.wal_commit_frames],
    ["Manifestsegmente", inventory.manifest_segments ?? "nicht vollständig verifiziert"],
    ["Historysegmente", inventory.history_segments],
    ["Rechte-Segmente", inventory.security_policy_segments],
    ["Schema-Definitionen", inventory.schema_definitions],
    ["Rechteversionen", inventory.capability_versions],
    ["CURRENT/Manifest", report.current_manifest === "corrupt" ? "nicht verifizierbar" : "geprüft oder nicht anwendbar"],
  ];
  for (const [label, value] of metrics) {
    const cell = document.createElement("div");
    cell.className = "result-cell";
    const heading = document.createElement("strong");
    heading.textContent = label;
    const text = document.createElement("p");
    text.textContent = value;
    cell.append(heading, text);
    summary.append(cell);
  }
  recoveryReportElement.append(summary);

  if (report.findings.length === 0) {
    const clean = document.createElement("p");
    clean.className = "muted";
    clean.textContent = "Keine Schäden oder offenen Recovery-Befunde gefunden.";
    recoveryFindings.append(clean);
  } else {
    for (const finding of report.findings) {
      const item = document.createElement("article");
      item.className = "result-cell";
      const heading = document.createElement("h3");
      heading.textContent = finding.code.replaceAll("_", " ");
      const classification = document.createElement("p");
      classification.className = "muted";
      classification.textContent = recoveryDamageText[finding.damage_class] ?? finding.damage_class;
      const description = document.createElement("p");
      description.textContent = recoveryIssueText[finding.code] ?? finding.summary;
      item.append(heading, classification, description);
      recoveryFindings.append(item);
    }
  }

  const nextActions = report.next_actions.map((action) => recoveryActionText[action] ?? action).join(" · ");
  recoveryNextActions.textContent = nextActions || "Keine zusätzliche Recoveryaktion ist erforderlich.";
  recoveryStatus.textContent = `Read-only-Prüfung abgeschlossen · sichere Revision ${report.safe_revision} · ${recoveryDispositionText[report.disposition] ?? report.disposition}.`;
  updateRecoveryControls();
}

const jobText = {
  kind: { migration: "Migration", backup: "Sicherung", index_build: "Indexaufbau" },
  status: { queued: "wartet", running: "läuft", succeeded: "abgeschlossen", failed: "fehlgeschlagen", cancelled: "abgebrochen", needs_restart: "Neustart erforderlich", interrupted: "nach Neustart unterbrochen" },
  phase: { queued: "wartet in der Queue", starting: "wird gestartet", scanning: "liest Daten", preparing: "bereitet Änderung vor", committing: "Commit läuft", finalizing: "schließt ab", recovering: "prüft Wiederaufnahme", shutting_down: "wird beendet" },
  cancellation: { ready: "Abbruch kann angefordert werden", cancellation_requested: "Abbruch angefordert; sichere Grenze wird abgewartet", commitpoint_passed: "too_late: Commit wird sicher abgeschlossen", committed: "Commit abgeschlossen", not_committed: "nicht veröffentlicht", outcome_unknown: "Ausgang unklar; Abgleich erforderlich", interrupted_after_restart: "Commit-Ausgang nach Neustart unklar" },
};

function formatJobMemory(bytes) {
  if (!Number.isFinite(bytes) || bytes < 0) return "unbekannt";
  if (bytes < 1024) return `${bytes} Byte`;
  const unit = bytes < 1024 * 1024 ? "KiB" : "MiB";
  const divisor = unit === "KiB" ? 1024 : 1024 * 1024;
  return `${(bytes / divisor).toFixed(1)} ${unit}`;
}

function jobJournalStatusText(status) {
  switch (status) {
    case "available": return "Der dauerhafte Jobjournalstatus ist geprüft.";
    case "recovered_older_snapshot": return "Der neueste Journalstand war beschädigt; ein älterer geprüfter Stand wird angezeigt.";
    case "write_failed": return "Die letzte Statusänderung konnte nicht dauerhaft gesichert werden. Angezeigt wird der letzte bestätigte Stand.";
    case "unavailable": return "Das Jobjournal ist nicht lesbar. Es werden keine Hintergrundjobs angenommen.";
    default: return "Der Jobjournalstatus ist unbekannt.";
  }
}

function renderJobs(result, cancelDisposition = null) {
  jobsList.replaceChildren();
  jobsStatus.textContent = jobJournalStatusText(result?.status);
  if (cancelDisposition) {
    const cancelText = {
      signalled: "Abbruch wurde angefordert.",
      already_signalled: "Der Abbruch war bereits angefordert.",
      too_late: "too_late: Der Commitpunkt ist erreicht; die Veröffentlichung läuft zu Ende.",
      already_terminal: "Der Job war bereits abgeschlossen.",
      outcome_unknown: "Der Commit-Ausgang ist unklar; bitte den Status abgleichen.",
    }[cancelDisposition] ?? "Der Abbruchstatus ist unbekannt.";
    jobsStatus.textContent = `${jobsStatus.textContent} ${cancelText}`;
  }
  const jobs = Array.isArray(result?.jobs) ? result.jobs : [];
  if (jobs.length === 0) {
    appendText(jobsList, "p", "Keine Hintergrundjobs aufgezeichnet.", "muted compact");
    return;
  }
  for (const job of jobs) {
    const entry = document.createElement("article");
    entry.className = "job-entry";
    const kind = jobText.kind[job.kind] ?? "Job";
    const status = jobText.status[job.status] ?? "unbekannter Status";
    appendText(entry, "h3", `${kind} · ${status}`);
    appendText(entry, "p", `Phase: ${jobText.phase[job.phase] ?? "unbekannt"} · Fortschritt: ${job.progress?.kind === "determinate" && Number.isSafeInteger(job.progress.completed) && Number.isSafeInteger(job.progress.total) ? `${job.progress.completed.toLocaleString()} / ${job.progress.total.toLocaleString()}` : "unbestimmt"}`);
    appendText(entry, "p", `Budget: höchstens ${Number(job.max_work_units).toLocaleString()} Arbeitseinheiten und ${formatJobMemory(job.max_memory_bytes)} · reserviert: ${formatJobMemory(job.reserved_memory_bytes)}.`);
    appendText(entry, "p", `Abbruch: ${jobText.cancellation[job.cancellation_state] ?? "Status unbekannt"}.`);
    if (job.resume_metadata_version !== null && job.resume_metadata_version !== undefined) {
      appendText(entry, "p", `Wiederaufnahmepunkt belegt · Format ${job.resume_metadata_version} · ${job.resume_metadata_bytes} Byte.`);
    }
    if (job.recovered_after_restart) {
      appendText(entry, "p", "Nach Neustart wiederhergestellt: Dieser letzte dauerhaft gespeicherte Stand ist unterbrochen; Erfolg wird nicht unterstellt.", "schema-note muted");
    }
    if (job.failure) appendText(entry, "p", "Der Worker hat einen Fehler gemeldet.", "muted");
    appendText(entry, "p", `Zuletzt belegt: ${new Date(job.observed_at_unix_ms).toLocaleString()}.`, "muted");
    if (["queued", "running"].includes(job.status)) {
      const cancel = document.createElement("button");
      cancel.type = "button";
      cancel.textContent = "Abbruch anfordern";
      cancel.disabled = jobsBusy || result.status === "unavailable" || result.status === "write_failed";
      cancel.addEventListener("click", () => { void requestJobCancel(job.job_id); });
      entry.append(cancel);
    }
    jobsList.append(entry);
  }
}

async function refreshJobs(activeSessionId = sessionId) {
  if (!projectOpen || !activeSessionId) return;
  if (jobsRefreshPromise) return jobsRefreshPromise;
  if (jobsBusy) return;
  jobsBusy = true;
  updateProjectControls();
  jobsRefreshPromise = (async () => {
    try {
      const response = await invoke("list_jobs", {
        request: { protocol_version: 1, session_id: activeSessionId },
      });
      if (response.protocol_version !== 1 || !response.result) throw new Error("unsupported_protocol");
      renderJobs(response.result);
    } catch (error) {
      jobsStatus.textContent = showError(error);
    } finally {
      jobsBusy = false;
      jobsRefreshPromise = null;
      updateProjectControls();
    }
  })();
  return jobsRefreshPromise;
}

async function requestJobCancel(jobId) {
  if (!sessionId || jobsBusy) return;
  jobsBusy = true;
  updateProjectControls();
  try {
    const response = await invoke("cancel_job", {
      request: { protocol_version: 1, session_id: sessionId, job_id: jobId },
    });
    if (response.protocol_version !== 1 || !response.result) throw new Error("unsupported_protocol");
    renderJobs(response.result, response.cancel_disposition ?? null);
  } catch (error) {
    jobsStatus.textContent = showError(error);
  } finally {
    jobsBusy = false;
    updateProjectControls();
  }
}

async function refreshProject(activeSessionId) {
  if (projectRefreshPromise) {
    projectRefreshQueued = true;
    await projectRefreshPromise;
    return;
  }
  const refresh = (async () => {
    for (let attempt = 0; attempt < 5; attempt += 1) {
      projectRefreshQueued = false;
      const wasOpen = projectOpen;
      const previousRevision = projectRevision;
      const previousDatabaseId = currentDatabaseId;
      renderProject(await getProject(activeSessionId));
      if (projectOpen) await refreshJobs(activeSessionId);
      if (projectOpen && currentDatabaseId && currentDatabaseId !== previousDatabaseId) {
        await reconcilePendingOperations();
        await reconcilePendingProjectCreations();
      }
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
        await refreshFactsCatalog(activeSessionId).catch((error) => {
          factsContextNote.textContent = showError(error);
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
        factCatalog = null;
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
        factsHistorySpace.replaceChildren();
        factsLayer.replaceChildren();
        factsPerspective.replaceChildren();
        factsSubject.replaceChildren();
        factsPredicate.replaceChildren();
        factsValueEntity.replaceChildren();
        factsTimeValueTimeline.replaceChildren();
        factsTimeValueUnit.replaceChildren();
        factsValidityTimeline.replaceChildren();
        factsQueryTimeline.replaceChildren();
        factsPreviewResults.replaceChildren();
        factsWriteStatus.textContent = "";
        factsPreviewStatus.textContent = "";
        entityTypeSelect.replaceChildren();
        entityAcceptDeprecated.checked = false;
        updateEntityControls();
      }
      if (!projectRefreshQueued) return;
    }
    throw new Error("project_busy");
  })();
  projectRefreshPromise = refresh;
  try {
    await refresh;
  } finally {
    if (projectRefreshPromise === refresh) projectRefreshPromise = null;
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

function runRecoveryRendererSmoke() {
  renderRecoveryReport({
    safe_revision: "9223372036854775807",
    disposition: "quarantined_read_only",
    read_only: true,
    current_manifest: "corrupt",
    inventory: {
      wal_commit_frames: "12",
      manifest_segments: null,
      history_segments: "8",
      security_policy_segments: "2",
      schema_definitions: "5",
      capability_versions: "3",
    },
    findings: [{
      code: "referenced_segment_corrupt",
      damage_class: "bitflip",
      summary: "safe summary",
      next_actions: ["preserve_original", "keep_read_only"],
    }],
    next_actions: [
      "preserve_original",
      "keep_read_only",
      "restore_verified_backup_to_new_destination",
      "salvage_into_new_database",
    ],
    can_run_journaled_recovery: false,
    can_restore_verified_backup: true,
    can_salvage: true,
  });
  const damageText = `${recoveryReportElement.textContent} ${recoveryFindings.textContent} ${recoveryNextActions.textContent}`;
  if (!damageText.includes("9223372036854775807") || !damageText.includes("read-only")
    || !damageText.includes("beschädigt") || recoveryRestoreButton.hidden
    || recoverySalvageFields.hidden || !recoveryRunButton.hidden) {
    throw new Error("Recovery-Bericht, read-only-Zustand oder getrennte Folgeaktionen fehlen.");
  }

  renderRecoveryReport({
    safe_revision: "42",
    disposition: "clean",
    read_only: false,
    current_manifest: "verified_or_not_applicable",
    inventory: {
      wal_commit_frames: "42",
      manifest_segments: "6",
      history_segments: "4",
      security_policy_segments: "2",
      schema_definitions: "5",
      capability_versions: "3",
    },
    findings: [],
    next_actions: [],
    can_run_journaled_recovery: false,
    can_restore_verified_backup: false,
    can_salvage: false,
  });
  if (!recoveryFindings.textContent.includes("Keine Schäden")
    || recoveryOpenCleanButton.hidden || !recoveryRunButton.hidden
    || !recoveryRestoreButton.hidden || !recoverySalvageFields.hidden) {
    throw new Error("Der saubere Recovery-Bericht bietet unerwartete Änderungsaktionen an.");
  }
  currentRecoveryReport = null;
  recoveryReportElement.hidden = true;
  recoveryActions.hidden = true;
  recoveryOpenCleanButton.hidden = true;
  recoveryStatus.textContent = "";
  recoveryFindings.replaceChildren();
  updateRecoveryControls();
}

async function runRecoverySmoke(activeSessionId) {
  const rendererPathCanary = "C:/renderer/selected/operation-path";
  for (const [command, fields] of [
    ["create_backup", { source_path: rendererPathCanary, output_path: rendererPathCanary }],
    ["verify_backup", { backup_path: rendererPathCanary }],
    ["restore_backup", { backup_path: rendererPathCanary, authorization_path: rendererPathCanary, output_path: rendererPathCanary }],
    ["export_data", { kind: "logical", from_revision: "1", through_revision: "1", history_spaces: ["00000000-0000-0000-0000-000000000000"], record_classes: ["HistorySpaceDefinition"], source_path: rendererPathCanary, output_path: rendererPathCanary }],
    ["create_import_plan", { mappings: [], destination_path: rendererPathCanary, input_path: rendererPathCanary, output_path: rendererPathCanary }],
    ["prepare_import", { destination_path: rendererPathCanary, input_path: rendererPathCanary, plan_path: rendererPathCanary }],
  ]) {
    let rejected = false;
    try {
      await invoke(command, {
        sessionId: activeSessionId,
        request: { protocol_version: 1, profile: "exact", ...fields },
      });
    } catch {
      rejected = true;
    }
    if (!rejected) throw new Error("Ein Datei- oder Export-/Import-IPC-Befehl hat einen Rendererpfad angenommen.");
  }
  let purgeRendererPathsRejected = true;
  for (const [command, request] of [
    ["preview_purge", {
      protocol_version: 1,
      targets: ["entity:00000000-0000-0000-0000-000000000000"],
      mode: "cascade",
      external_inventory_complete: false,
      known_external_artifacts: [],
      source_path: rendererPathCanary,
      destination_path: rendererPathCanary,
      report_path: rendererPathCanary,
    }],
    ["execute_purge", {
      protocol_version: 1,
      plan_fingerprint: "a".repeat(64),
      destination_path: rendererPathCanary,
    }],
  ]) {
    try {
      await invoke(command, { sessionId: activeSessionId, request });
      purgeRendererPathsRejected = false;
    } catch {
      // Unknown renderer paths must be rejected before any native picker or run can start.
    }
  }
  if (!purgeRendererPathsRejected) throw new Error("Ein Purge-IPC-Befehl hat Rendererpfade angenommen.");
  await recordFactsSmokeStage("backup-renderer-paths:rejected");
  await recordFactsSmokeStage("export-import-renderer-paths:rejected");
  await recordFactsSmokeStage("purge-renderer-paths:rejected");

  const response = await invoke("inspect_recovery", {
    sessionId: activeSessionId,
    request: { protocol_version: 1 },
  });
  const report = response.result;
  if (response.protocol_version !== 1 || report.disposition !== "clean"
    || report.read_only || report.findings.length !== 0 || !report.safe_revision) {
    throw new Error("Die native read-only Recovery-Prüfung meldete keinen sauberen Projektstand.");
  }
  renderRecoveryReport(report);
  await recordFactsSmokeStage("recovery-smoke:pass");
}

async function runProjectSmoke(activeSessionId) {
  if (role === "primary") {
    await createProjectTracked(activeSessionId, "IPC-Smoke");
  } else {
    const deadline = Date.now() + 20000;
    let project;
    do {
      project = await getProject(activeSessionId);
      if (project.project_open) {
        await invoke("open_project", {
          sessionId: activeSessionId,
          request: { protocol_version: 1 },
        });
        break;
      }
      await new Promise((resolve) => setTimeout(resolve, 100));
    } while (Date.now() < deadline);
    if (!project?.project_open) throw new Error("Das primäre Fenster hat das Testprojekt nicht angelegt.");
  }
  renderProject(await getProject(activeSessionId));
  if (jobsPanel.hidden) throw new Error("Die Hintergrundjob-Ansicht blieb trotz geöffnetem Projekt verborgen.");
  const response = await invoke("list_jobs", {
    request: { protocol_version: 1, session_id: activeSessionId },
  });
  if (response.protocol_version !== 1 || response.result?.status !== "available"
    || !Array.isArray(response.result.jobs) || response.result.jobs.length !== 0) {
    throw new Error("Das Jobjournal lieferte für das frische Smoke-Projekt keinen leeren, geprüften Status.");
  }
  renderJobs(response.result);
  if (!jobsList.textContent.includes("Keine Hintergrundjobs aufgezeichnet.")) {
    throw new Error("Die leere Jobliste wird nicht sichtbar dargestellt.");
  }

  const fixture = {
    job_id: "00000000-0000-7000-8000-000000000020",
    kind: "migration",
    status: "running",
    phase: "committing",
    progress: { kind: "determinate", completed: 2, total: 10 },
    max_work_units: 100,
    max_memory_bytes: 4096,
    reserved_memory_bytes: 1024,
    pool: "blocking_io",
    cancellation_state: "commitpoint_passed",
    resume_metadata_version: 1,
    resume_metadata_bytes: 3,
    failure: null,
    observed_at_unix_ms: Date.now(),
    recovered_after_restart: false,
  };
  renderJobs({ status: "available", jobs: [fixture] }, "too_late");
  const committingText = `${jobsStatus.textContent} ${jobsList.textContent}`;
  if (!committingText.includes("2 / 10") || !committingText.includes("höchstens 100 Arbeitseinheiten")
    || !committingText.includes("Commit läuft") || !committingText.includes("too_late:")
    || !committingText.includes("Wiederaufnahmepunkt belegt")
    || !jobsList.querySelector("button")) {
    throw new Error("Fortschritt, Budget, Commitpoint, Cancelwirkung oder Resume-Hinweis fehlt in der Jobansicht.");
  }
  renderJobs({
    status: "available",
    jobs: [{
      ...fixture,
      status: "interrupted",
      phase: "scanning",
      progress: { kind: "indeterminate" },
      cancellation_state: "interrupted_after_restart",
      recovered_after_restart: true,
    }],
  });
  if (!jobsList.textContent.includes("unbestimmt")
    || !jobsList.textContent.includes("Nach Neustart wiederhergestellt")) {
    throw new Error("Unbestimmter Fortschritt oder belegter Neustartstatus fehlt in der Jobansicht.");
  }
  renderJobs(response.result);
  runRecoveryRendererSmoke();
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
  return invokeManagedCommand("manage_schema", command, activeSessionId,
    (response) => response.protocol_version === 1);
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
  return invokeManagedCommand("manage_entities", command, activeSessionId,
    (response) => response.protocol_version === 1);
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
  return invokeManagedCommand("manage_branch_layers", command, activeSessionId,
    (response) => response.protocol_version === 1);
}

async function manageHistorySpaceTransfer(command, activeSessionId = sessionId) {
  return invokeManagedCommand("manage_history_space_transfer", command, activeSessionId,
    (response) => response.protocol_version === 1);
}

async function manageFacts(command, activeSessionId = sessionId, operationIdOverride) {
  return invokeManagedCommand("manage_facts", command, activeSessionId,
    (response) => response.protocol_version === 1 && Boolean(response.result?.kind), operationIdOverride);
}

async function refreshFactsCatalog(activeSessionId = sessionId) {
  if (!projectOpen || !activeSessionId) return;
  if (factCatalogRefreshPromise) {
    factCatalogRefreshQueued = true;
    await factCatalogRefreshPromise;
    return;
  }

  const refresh = (async () => {
    for (let attempt = 0; attempt < 5; attempt += 1) {
      factCatalogRefreshQueued = false;
      factsContextNote.textContent = "Aktuelle Branches, Layer, Entitäten und Schemadefinitionen werden geladen …";
      const [schema, entities, branches, perspectives, records] = await Promise.all([
        manageSchema({ command: "snapshot", mode: { mode: "current" } }, activeSessionId),
        manageEntities({ command: "snapshot", mode: { mode: "current" } }, activeSessionId),
        manageBranchLayers({ command: "snapshot", mode: { mode: "current" } }, activeSessionId),
        invokePerspectiveSnapshot(activeSessionId, { mode: "current" }),
        manageFacts({ command: "snapshot" }, activeSessionId),
      ]);
      if ([schema, entities, branches, perspectives].some((snapshot) => snapshot.kind !== "snapshot")
        || records.kind !== "catalog") {
        throw new Error("unsupported_protocol");
      }
      const revisions = [schema.revision, entities.revision, branches.revision, perspectives.revision, records.revision];
      if (revisions.some((revision) => revision !== revisions[0])) {
        if (attempt < 4) continue;
        throw new Error("invalid_request");
      }
      factCatalog = { schema, entities, branches, perspectives, records: records.records,
        lifecycleVisible: records.lifecycle_visible, revision: revisions[0],
        queryRevision: records.revision_text,
        eventGraphGuidance: records.event_graph_guidance ?? [],
        sources: records.sources ?? [], evidence: records.evidence ?? [],
        provenance: records.provenance ?? [], endpointOptions: records.endpoint_options ?? [] };
      renderFactsChoices();
      renderFactRecordCatalog();
      updateFactControls();
      if (!factCatalogRefreshQueued) return;
    }
    throw new Error("project_busy");
  })();
  factCatalogRefreshPromise = refresh;
  try {
    await refresh;
  } finally {
    if (factCatalogRefreshPromise === refresh) factCatalogRefreshPromise = null;
  }
}

function replaceFactOptions(select, entries, previousValue, placeholder) {
  select.replaceChildren();
  for (const item of entries) {
    const option = document.createElement("option");
    option.value = item.value;
    option.textContent = item.label;
    select.append(option);
  }
  if (entries.some((item) => item.value === previousValue)) select.value = previousValue;
  else if (entries.length === 0) {
    const option = document.createElement("option");
    option.value = "";
    option.textContent = placeholder;
    select.append(option);
  }
}

function factsSchemaDefinitions(family, activeOnly = false) {
  const definitions = factCatalog?.schema.definitions ?? [];
  return definitions.filter((definition) => definition.family === family
    && (!activeOnly || definition.lifecycle === "active"));
}

function renderFactsChoices() {
  if (!factCatalog) return;
  if (factsQueryRecordedAsOf.dataset.auto !== "false") {
    factsQueryRecordedAsOf.value = factCatalog.queryRevision;
  }
  if (factsQuerySchemaRevision.dataset.auto !== "false") {
    factsQuerySchemaRevision.value = factCatalog.queryRevision;
  }
  const previous = {
    historySpace: factsHistorySpace.value,
    layer: factsLayer.value,
    perspective: factsPerspective.value,
    subject: factsSubject.value,
    predicate: factsPredicate.value,
    valueEntity: factsValueEntity.value,
    timeTimeline: factsTimeValueTimeline.value,
    validityTimeline: factsValidityTimeline.value,
    queryTimeline: factsQueryTimeline.value,
    timeUnit: factsTimeValueUnit.value,
    eventKind: factsEventKind.value,
    eventCloseTimeline: factsEventSpanCloseTimeline.value,
  };
  const branchNames = branchLabels(factCatalog.branches);
  replaceFactOptions(factsHistorySpace, factCatalog.branches.branches.map((branch) => ({
    value: branch.history_space_id,
    label: branchNames.get(branch.history_space_id) ?? "Branch",
  })), previous.historySpace, "Kein Branch vorhanden");
  replaceFactOptions(factsLayer, factCatalog.branches.layers
    .filter((layer) => layer.lifecycle !== "retired")
    .map((layer) => ({
      value: layer.layer_id,
      label: `${layer.symbol}${layer.is_base ? " · Basis" : ""}`,
    })), previous.layer, "Kein verwendbarer Layer vorhanden");
  replaceFactOptions(factsPerspective, (factCatalog.perspectives.perspectives ?? [])
    .filter((item) => item.retired_revision == null)
    .map((item) => ({ value: item.perspective_id, label: perspectiveNameFor(item) })),
  previous.perspective, "Keine Perspektive vorhanden");
  replaceFactOptions(factsSubject, factCatalog.entities.entities
    .filter((entity) => entity.retired_revision == null)
    .map((entity) => ({
      value: entity.entity_id,
      label: `${entity.entity_type_symbol} · Revision ${entity.created_revision}`,
    })), previous.subject, "Keine aktive Entität vorhanden");
  replaceFactOptions(factsValueEntity, factCatalog.entities.entities
    .filter((entity) => entity.retired_revision == null)
    .map((entity) => ({ value: entity.entity_id, label: entity.entity_type_symbol })),
  previous.valueEntity, "Keine aktive Entität vorhanden");
  replaceFactOptions(factsPredicate, factsSchemaDefinitions("predicate", true).map((definition) => ({
    value: definition.identity,
    label: definition.symbol,
  })), previous.predicate, "Kein aktives Prädikat vorhanden");
  const timelines = factsSchemaDefinitions("timeline", true).map((definition) => ({
    value: definition.identity,
    label: definition.symbol,
  }));
  for (const select of [factsTimeValueTimeline, factsValidityTimeline, factsQueryTimeline]) {
    replaceFactOptions(select, timelines, select === factsTimeValueTimeline
      ? previous.timeTimeline : select === factsValidityTimeline ? previous.validityTimeline : previous.queryTimeline,
    "Keine aktive Timeline vorhanden");
  }
  replaceFactOptions(factsTimeValueUnit, factsSchemaDefinitions("time_unit", true).map((definition) => ({
    value: definition.symbol,
    label: definition.symbol,
  })), previous.timeUnit, "Keine aktive Zeiteinheit vorhanden");
  replaceFactOptions(factsEventKind, factsSchemaDefinitions("event_kind", true).map((definition) => ({
    value: definition.identity,
    label: definition.symbol,
  })), previous.eventKind, "Kein aktives EventKind vorhanden");
  replaceFactOptions(factsEventSpanCloseTimeline, timelines, previous.eventCloseTimeline,
    "Keine aktive Timeline vorhanden");
  updateEventDraftTemplate();
  updateFactsValueFields();
  updateFactControls();
}

function renderFactRecordCatalog() {
  factsRecordCatalog.replaceChildren();
  const records = factCatalog?.records ?? [];
  if (!records.length) {
    appendText(factsRecordCatalog, "p", "Noch keine für dich sichtbaren Fakten, Events oder Eventrelationen.", "muted");
  } else {
    const list = document.createElement("ul");
    for (const record of records) {
      const status = [];
      if (record.retracted === true) status.push("zurückgenommen");
      else if (record.retracted === false) status.push("aktiv");
      if (record.archived === true) status.push("archiviert");
      else if (record.archived === false) status.push("nicht archiviert");
      const suffix = status.length ? ` · ${status.join(" · ")}` : "";
      const detail = record.family === "event_relation"
        ? ` · ${record.from_event_id} ${record.relation_kind} ${record.to_event_id}`
        : record.family === "event_mask" ? ` · Ziel-Event ${record.target_event_id}`
          : record.family === "event_span_closure" ? ` · Ende ${record.time_end_nanoseconds} ns auf ${record.timeline_id}` : "";
      appendText(list, "li", `${record.family} · ${record.record_id}${detail} · Revision ${record.created_revision}${suffix}`);
    }
    factsRecordCatalog.append(list);
  }

  const assertionRecords = records.filter((record) => record.family === "assertion" && record.retracted !== true);
  replaceFactOptions(factsCorrectionTarget, assertionRecords.map((record) => ({
    value: record.record_id,
    label: `${record.record_id} · Revision ${record.created_revision}`,
  })), factsCorrectionTarget.value, "Keine sichtbare aktive Assertion vorhanden");
  const eventRecords = records.filter((record) => record.family === "event" && record.retracted !== true);
  const unarchivedEvents = eventRecords.filter((record) => record.archived !== true);
  replaceFactOptions(factsEventCorrectionTarget, eventRecords.map((record) => ({
    value: record.record_id,
    label: `${record.record_id} · Revision ${record.created_revision}`,
  })), factsEventCorrectionTarget.value, "Kein sichtbarer aktiver Event vorhanden");
  replaceFactOptions(factsEventMaskTarget, unarchivedEvents.map((record) => ({
    value: record.record_id,
    label: `${record.event_kind_id ?? "Event"} · ${record.record_id}`,
  })), factsEventMaskTarget.value, "Kein sichtbares, nicht archiviertes Event vorhanden");
  const relationOptions = eventRecords.map((record) => ({
    value: record.record_id,
    label: `${record.event_kind_id ?? "Event"} · ${record.record_id}`,
  }));
  replaceFactOptions(factsEventRelationFrom, relationOptions, factsEventRelationFrom.value,
    "Kein sichtbares aktives Event vorhanden");
  replaceFactOptions(factsEventRelationTo, relationOptions, factsEventRelationTo.value,
    "Kein sichtbares aktives Event vorhanden");
  replaceFactOptions(factsEventSpanCloseTarget, eventRecords.map((record) => ({
    value: record.record_id,
    label: `${record.event_kind_id ?? "Event"} · ${record.record_id}`,
  })), factsEventSpanCloseTarget.value, "Kein sichtbares aktives Event vorhanden");
  const lifecycleFamilies = new Set(["assertion", "mask", "replacement_boundary", "event", "event_mask", "event_relation"]);
  const lifecycleRecords = records.filter((record) => lifecycleFamilies.has(record.family) && record.retracted !== true);
  replaceFactOptions(factsLifecycleTarget, lifecycleRecords.map((record) => ({
    value: `${record.family}:${record.record_id}`,
    label: `${record.family} · ${record.record_id}${record.archived === true ? " · archiviert" : ""}`,
  })), factsLifecycleTarget.value, "Keine Datensätze für Lebenszyklusaktionen vorhanden");
  const sourceOptions = (factCatalog?.sources ?? []).map((source) => ({
    value: source.source_id,
    label: `${source.source_kind} · ${source.source_id}`,
  }));
  replaceFactOptions(factsSourceSupersedeTarget, sourceOptions, factsSourceSupersedeTarget.value,
    "Keine sichtbare Source vorhanden");
  replaceFactOptions(factsEvidenceSource, sourceOptions, factsEvidenceSource.value,
    "Keine sichtbare Source vorhanden");
  const endpointOptions = factCatalog?.endpointOptions ?? [];
  const evidenceTargetOptions = endpointOptions
    .filter((endpoint) => endpoint.family !== "source" && endpoint.family !== "evidence")
    .map((endpoint) => ({
      value: `${endpoint.family}:${endpoint.record_id}`,
      label: `${endpoint.family} · ${endpoint.record_id}`,
    }));
  replaceFactOptions(factsEvidenceTarget, evidenceTargetOptions, factsEvidenceTarget.value,
    "Kein autorisierter Evidence-Zielrecord vorhanden");
  const provenanceEndpointOptions = endpointOptions.map((endpoint) => ({
    value: `${endpoint.family}:${endpoint.record_id}`,
    label: `${endpoint.family} · ${endpoint.record_id}`,
  }));
  replaceFactOptions(factsProvenanceFrom, provenanceEndpointOptions, factsProvenanceFrom.value,
    "Kein autorisierter Provenance-Endpunkt vorhanden");
  replaceFactOptions(factsProvenanceTo, provenanceEndpointOptions, factsProvenanceTo.value,
    "Kein autorisierter Provenance-Endpunkt vorhanden");
  const activeEvidence = (factCatalog?.evidence ?? []).filter((item) => item.retracted === false);
  replaceFactOptions(factsEvidenceRetractTarget, activeEvidence.map((item) => ({
    value: item.evidence_id,
    label: `${item.relation} · ${item.evidence_id}`,
  })), factsEvidenceRetractTarget.value, "Keine aktive, autorisierte Evidence vorhanden");
  const activeProvenance = (factCatalog?.provenance ?? []).filter((item) => item.retracted === false);
  replaceFactOptions(factsProvenanceRetractTarget, activeProvenance.map((item) => ({
    value: item.provenance_id,
    label: `${item.relation} · ${item.provenance_id}`,
  })), factsProvenanceRetractTarget.value, "Keine aktive, autorisierte Provenance vorhanden");
  renderFactMetaCatalog();
  updateEventCorrectionTemplate();
  factsEventGraphGuidance.replaceChildren();
  for (const line of factCatalog?.eventGraphGuidance ?? []) appendText(factsEventGraphGuidance, "p", line);
  clearFactActionPreviews();
}

function renderFactMetaCatalog() {
  factsMetaCatalog.replaceChildren();
  const groups = [
    ["Sources", (factCatalog?.sources ?? []).map((item) =>
      `${item.source_kind} · ${item.source_id} · Revision ${item.created_revision}${item.locator ? ` · ${item.locator}` : ""}`)],
    ["Evidence", (factCatalog?.evidence ?? []).map((item) =>
      `${item.relation} · ${item.source_id} → ${item.target_family}:${item.target_record_id} · Revision ${item.created_revision}${item.retracted === true ? " · zurückgenommen" : ""}`)],
    ["Provenance", (factCatalog?.provenance ?? []).map((item) =>
      `${item.relation} · ${item.from_family}:${item.from_record_id} → ${item.to_family}:${item.to_record_id} · Revision ${item.created_revision}${item.retracted === true ? " · zurückgenommen" : ""}`)],
  ];
  for (const [title, rows] of groups) {
    appendText(factsMetaCatalog, "h3", title);
    if (!rows.length) {
      appendText(factsMetaCatalog, "p", "Keine sichtbaren Einträge.", "muted");
      continue;
    }
    const list = document.createElement("ul");
    for (const row of rows) appendText(list, "li", row);
    factsMetaCatalog.append(list);
  }
}

function selectedFactRecord(select, composite = false) {
  const [family, id] = composite
    ? (select.value.includes(":") ? select.value.split(/:(.*)/s).slice(0, 2) : ["", ""])
    : [select === factsCorrectionTarget ? "assertion" : "event", select.value];
  return (factCatalog?.records ?? []).find((record) => record.family === family && record.record_id === id) ?? null;
}

function updateEventCorrectionTemplate() {
  const record = selectedFactRecord(factsEventCorrectionTarget);
  if (!record) return;
  if (factsEventCorrectionDraft.dataset.templateTarget === record.record_id
    && factsEventCorrectionDraft.value.trim()) return;
  const activeTimeline = factsSchemaDefinitions("timeline", true)[0]?.identity ?? "";
  factsEventCorrectionDraft.value = JSON.stringify({
    history_space_id: record.history_space_id ?? "",
    layer_id: record.layer_id ?? "",
    event_kind_id: record.event_kind_id ?? "",
    participants: [],
    attributes: [],
    event_time: { kind: "instant", timeline_id: activeTimeline, nanoseconds: "0" },
  }, null, 2);
  factsEventCorrectionDraft.dataset.templateTarget = record.record_id;
}

function updateEventDraftTemplate() {
  const eventKindId = factsEventKind.value;
  if (!eventKindId) return;
  if (factsEventDraft.dataset.templateKind === eventKindId && factsEventDraft.value.trim()) return;
  const activeTimeline = factsSchemaDefinitions("timeline", true)[0]?.identity ?? "";
  factsEventDraft.value = JSON.stringify({
    history_space_id: factsHistorySpace.value,
    layer_id: factsLayer.value,
    event_kind_id: eventKindId,
    participants: [],
    attributes: [],
    event_time: { kind: "instant", timeline_id: activeTimeline, nanoseconds: "0" },
  }, null, 2);
  factsEventDraft.dataset.templateKind = eventKindId;
}

function clearFactActionPreviews() {
  pendingFactActionPreviews = { assertion: false, event: false, lifecycle: false };
  pendingFactCorrectionCommands = { assertion: null, event: null };
  for (const button of [factsCorrectionCommitButton, factsEventCorrectionCommitButton, factsLifecycleCommitButton]) {
    button.disabled = true;
  }
  factsCorrectionPreviewResult.replaceChildren();
  factsEventCorrectionPreviewResult.replaceChildren();
  factsLifecyclePreviewResult.replaceChildren();
  updateFactControls();
}

function selectedFactPredicate() {
  return factsSchemaDefinitions("predicate", true)
    .find((definition) => definition.identity === factsPredicate.value) ?? null;
}

function factsContextInput() {
  const mode = factsEpistemicMode.value;
  const perspectiveId = mode === "world_state" ? null : factsPerspective.value;
  if (!factsHistorySpace.value || !factsLayer.value || (mode !== "world_state" && !perspectiveId)) {
    throw new Error("invalid_request");
  }
  return {
    history_space_id: factsHistorySpace.value,
    layer_id: factsLayer.value,
    perspective_id: perspectiveId,
    epistemic_mode: mode,
  };
}

function factsSourceFieldsInput() {
  const sourceKind = factsSourceKind.value.trim();
  const locator = factsSourceLocator.value.trim();
  const digest = factsSourceDigest.value.trim();
  const key = factsSourceMetadataKey.value.trim();
  const metadataValue = factsSourceMetadataValue.value;
  if (!/^[a-z][a-z0-9_]*$/.test(sourceKind)
    || (key && !/^[a-z][a-z0-9_]*$/.test(key))
    || (digest && (!/^(?:[0-9a-fA-F]{2})+$/.test(digest)))) {
    throw new Error("invalid_request");
  }
  if (!key && metadataValue) throw new Error("invalid_request");
  return {
    source_kind: sourceKind,
    locator: locator || null,
    content_digest_hex: digest || null,
    metadata: key ? [{ key, value: { kind: "string", data: metadataValue } }] : [],
  };
}

function factsEndpointInput(select) {
  const separator = select.value.indexOf(":");
  if (separator < 1) throw new Error("invalid_request");
  return {
    family: select.value.slice(0, separator),
    record_id: select.value.slice(separator + 1),
  };
}

function updateFactsValueFields() {
  const kind = selectedFactPredicate()?.details?.value_kind ?? "";
  factsValueKindLabel.textContent = kind ? `· ${kind}` : "";
  factsValueTextWrap.hidden = ["bool", "entity", "time"].includes(kind);
  factsValueBoolWrap.hidden = kind !== "bool";
  factsValueEntityWrap.hidden = kind !== "entity";
  factsTimeValueFields.hidden = kind !== "time";
  factsValueText.type = "text";
  factsValueText.inputMode = ["int", "uint", "decimal", "duration", "bytes"].includes(kind) ? "text" : "text";
  factsValueText.placeholder = ({
    int: "Exakte signed 128-bit Ganzzahl",
    uint: "Exakte unsigned 128-bit Ganzzahl",
    decimal: "Exakte Dezimalzahl",
    string: "Textwert",
    symbol: "ASCII-Symbol, z. B. true_name",
    duration: "Signed Nanosekunden",
    bytes: "Gerade Anzahl Hex-Zeichen",
  })[kind] ?? "";
  const resolution = selectedFactPredicate()?.details?.resolution_policy;
  factsBoundaryNote.textContent = resolution === "multi_value_replace"
    ? "Dieses Prädikat verwendet MultiValueReplace. Die Boundary definiert ab ihrer Gültigkeit die vollständige Wertemenge, auch wenn diese leer ist."
    : "Für dieses Prädikat ist keine Boundary zulässig; wähle ein Prädikat mit Auflösung MultiValueReplace.";
  updateFactControls();
}

function factsValidityInput() {
  const timelineId = factsValidityTimeline.value;
  if (!timelineId) throw new Error("invalid_request");
  if (!factsValidityEnabled.checked) {
    return { timeline_id: timelineId, start_nanoseconds: null, end_nanoseconds: null };
  }
  const start = factsValidityStart.value.trim() ? signed128Text(factsValidityStart.value) : "";
  const end = factsValidityEnd.value.trim() ? signed128Text(factsValidityEnd.value) : "";
  if (start && end && BigInt(start) > BigInt(end)) throw new Error("invalid_request");
  return {
    timeline_id: timelineId,
    start_nanoseconds: start || null,
    end_nanoseconds: end || null,
  };
}

function factsValueInput() {
  const kind = selectedFactPredicate()?.details?.value_kind;
  if (!kind) throw new Error("invalid_request");
  if (kind === "bool") return { kind, data: factsValueBool.value === "true" };
  if (kind === "entity") {
    if (!factsValueEntity.value) throw new Error("invalid_request");
    return { kind, data: factsValueEntity.value };
  }
  if (kind === "time") {
    if (!factsTimeValueTimeline.value || !factsTimeValueUnit.value) {
      throw new Error("invalid_request");
    }
    return {
      kind,
      data: {
        timeline_id: factsTimeValueTimeline.value,
        ticks: signed128Text(factsTimeValueTicks.value),
        unit_symbol: factsTimeValueUnit.value,
      },
    };
  }
  const raw = factsValueText.value;
  const data = kind === "int" || kind === "duration"
    ? signed128Text(raw)
    : kind === "uint" ? unsigned128Text(raw) : raw;
  if (kind === "symbol" && !/^[a-z][a-z0-9_]*$/.test(data)) throw new Error("invalid_request");
  if (kind === "bytes" && (data.length % 2 !== 0 || !/^[0-9a-f]*$/i.test(data))) throw new Error("invalid_request");
  return { kind: kind === "bytes" ? "bytes_hex" : kind, data };
}

function signed128Text(input) {
  const value = input.trim();
  if (!/^-?(0|[1-9][0-9]*)$/.test(value)) throw new Error("invalid_request");
  const integer = BigInt(value);
  if (integer < -(1n << 127n) || integer > (1n << 127n) - 1n) throw new Error("invalid_request");
  return value;
}

function unsigned128Text(input) {
  const value = input.trim();
  if (!/^(0|[1-9][0-9]*)$/.test(value)) throw new Error("invalid_request");
  const integer = BigInt(value);
  if (integer < 0n || integer > (1n << 128n) - 1n) throw new Error("invalid_request");
  return value;
}

function factsWorldTimeInput() {
  if (factsQueryTimeMode.value === "all_times") return { kind: "all_times" };
  if (!factsQueryTimeline.value) throw new Error("invalid_request");
  const value = signed128Text(factsQueryNanoseconds.value);
  return { kind: "at", timeline_id: factsQueryTimeline.value, nanoseconds: value };
}

function factsQueryRevisionInput(input) {
  const value = input.value.trim();
  if (!/^(0|[1-9][0-9]*)$/.test(value)) throw new Error("invalid_request");
  let revision;
  let latest;
  try {
    revision = BigInt(value);
    latest = BigInt(factCatalog.queryRevision);
  } catch { throw new Error("invalid_request"); }
  if (revision >= (1n << 64n) - 1n || revision > latest) throw new Error("invalid_request");
  return value;
}

function factsQueryUnsignedInput(input, maximum, allowZero = false) {
  const value = input.value.trim();
  if (!/^(0|[1-9][0-9]*)$/.test(value)) throw new Error("invalid_request");
  let parsed;
  try { parsed = BigInt(value); } catch { throw new Error("invalid_request"); }
  if ((!allowZero && parsed === 0n) || parsed > BigInt(maximum)) throw new Error("invalid_request");
  return Number(parsed);
}

function factsQueryCommand({ continuation = null } = {}) {
  if (!factCatalog || !factsSubject.value || !factsPredicate.value) throw new Error("invalid_request");
  const recordedAsOf = factsQueryRevisionInput(factsQueryRecordedAsOf);
  let schemaMode;
  if (factsQuerySchemaMode.value === "historical") {
    schemaMode = { mode: "historical", recorded_as_of: recordedAsOf };
  } else if (factsQuerySchemaMode.value === "current") {
    schemaMode = { mode: "current" };
  } else if (factsQuerySchemaMode.value === "explicit") {
    schemaMode = { mode: "explicit", revision: factsQueryRevisionInput(factsQuerySchemaRevision) };
  } else {
    throw new Error("invalid_request");
  }
  const worldTime = factsWorldTimeInput();
  if (factsQueryOperation.value === "explain" && worldTime.kind !== "at") {
    throw new Error("invalid_request");
  }
  const queryMode = factsQueryOperation.value;
  if (!["history", "resolved", "explain", "token_search", "graph", "count", "exists", "grouped_count"].includes(queryMode)) {
    throw new Error("invalid_request");
  }
  if (queryMode === "token_search" && selectedFactPredicate()?.details?.value_kind !== "string") {
    throw new Error("invalid_request");
  }
  const command = {
    command: "query",
    context: factsContextInput(),
    subject_id: factsSubject.value,
    predicate_id: factsPredicate.value,
    recorded_as_of: recordedAsOf,
    schema_mode: schemaMode,
    query_mode: queryMode,
    world_time: worldTime,
    max_candidates: factsQueryUnsignedInput(factsQueryMaxCandidates, Number.MAX_SAFE_INTEGER),
    max_work_units: factsQueryUnsignedInput(factsQueryMaxWorkUnits, Number.MAX_SAFE_INTEGER),
    max_results: factsQueryUnsignedInput(factsQueryMaxResults, Number.MAX_SAFE_INTEGER),
  };
  if (queryMode === "token_search") {
    const terms = factsQuerySearchTerms.value.trim();
    if (!terms) throw new Error("invalid_request");
    command.search_terms = terms;
    command.search_match = factsQuerySearchMatch.value;
    command.page_size = factsQueryUnsignedInput(factsQueryPageSize, 500);
    command.continuation = continuation;
  } else if (queryMode === "graph") {
    const rootRecordId = factsQueryGraphRootId.value.trim();
    const relationships = Array.from(factsQueryGraphRelationships.selectedOptions, (option) => option.value);
    if (!rootRecordId || relationships.length === 0) throw new Error("invalid_request");
    command.graph = {
      root_family: factsQueryGraphRootFamily.value,
      root_record_id: rootRecordId,
      relationships,
      direction: factsQueryGraphDirection.value,
      max_depth: factsQueryUnsignedInput(factsQueryGraphMaxDepth, 65535, true),
      max_nodes: factsQueryUnsignedInput(factsQueryGraphMaxNodes, Number.MAX_SAFE_INTEGER),
      max_edges: factsQueryUnsignedInput(factsQueryGraphMaxEdges, Number.MAX_SAFE_INTEGER),
      cycle_policy: factsQueryGraphCyclePolicy.value,
    };
  }
  return command;
}

function factsAssertionCorrectionCommand() {
  const target = selectedFactRecord(factsCorrectionTarget);
  if (!target?.history_space_id || !target.layer_id || !target.epistemic_mode
    || !target.subject_id || !target.predicate_id || !factsCorrectionReason.value.trim()
    || factsSubject.value !== target.subject_id || factsPredicate.value !== target.predicate_id) {
    throw new Error("invalid_request");
  }
  return {
    command: "correct_assertion",
    operation_id: crypto.randomUUID(),
    expected_base_revision: factCatalog.revision,
    target_assertion_id: target.record_id,
    expected_target_created_revision: target.created_revision,
    replacement_assertion_id: crypto.randomUUID(),
    retraction_id: crypto.randomUUID(),
    corrects_provenance_id: crypto.randomUUID(),
    replacement: {
      context: {
        history_space_id: target.history_space_id,
        layer_id: target.layer_id,
        perspective_id: target.perspective_id,
        epistemic_mode: target.epistemic_mode,
      },
      subject_id: target.subject_id,
      predicate_id: target.predicate_id,
      value: factsValueInput(),
      polarity: factsPolarity.value,
      validity: factsValidityInput(),
    },
    retraction_reason: factsCorrectionReason.value.trim(),
  };
}

function factsEventCorrectionCommand() {
  const target = selectedFactRecord(factsEventCorrectionTarget);
  if (!target?.event_kind_id) throw new Error("invalid_request");
  let replacement;
  try { replacement = JSON.parse(factsEventCorrectionDraft.value); } catch { throw new Error("invalid_request"); }
  if (!replacement || replacement.history_space_id !== target.history_space_id
    || replacement.layer_id !== target.layer_id || replacement.event_kind_id !== target.event_kind_id
    || !Array.isArray(replacement.participants) || !Array.isArray(replacement.attributes)
    || !replacement.event_time) throw new Error("invalid_request");
  return {
    command: "correct_event",
    operation_id: crypto.randomUUID(),
    expected_base_revision: factCatalog.revision,
    target_event_id: target.record_id,
    expected_target_created_revision: target.created_revision,
    replacement_event_id: crypto.randomUUID(),
    corrects_provenance_id: crypto.randomUUID(),
    replacement,
  };
}

function factsEventCreateCommand() {
  if (!factCatalog || !factsHistorySpace.value || !factsLayer.value || !factsEventKind.value) {
    throw new Error("invalid_request");
  }
  let draft;
  try { draft = JSON.parse(factsEventDraft.value); } catch { throw new Error("invalid_request"); }
  if (!draft || typeof draft.history_space_id !== "string" || !draft.history_space_id
    || typeof draft.layer_id !== "string" || !draft.layer_id
    || draft.event_kind_id !== factsEventKind.value
    || !Array.isArray(draft.participants) || !Array.isArray(draft.attributes)
    || !draft.event_time || !["instant", "span"].includes(draft.event_time.kind)) {
    throw new Error("invalid_request");
  }
  return {
    command: "create_event",
    expected_base_revision: factCatalog.revision,
    draft,
  };
}

function factsLifecycleCommand() {
  if (!factCatalog?.lifecycleVisible) throw new Error("invalid_request");
  const record = selectedFactRecord(factsLifecycleTarget, true);
  if (!record) throw new Error("invalid_request");
  const key = {
    assertion: "assertion_id",
    mask: "mask_id",
    replacement_boundary: "replacement_boundary_id",
    event: "event_id",
    event_mask: "event_mask_id",
    event_relation: "event_relation_id",
  }[record.family];
  if (!key) throw new Error("invalid_request");
  const target = { family: record.family, [key]: record.record_id };
  const action = factsLifecycleAction.value;
  if (action === "retract") {
    if (!factsLifecycleReason.value.trim()) throw new Error("invalid_request");
    return {
      command: "lifecycle",
      expected_base_revision: factCatalog.revision,
      target,
      action: { action, reason: factsLifecycleReason.value.trim() },
    };
  }
  return {
    command: "lifecycle",
    expected_base_revision: factCatalog.revision,
    target,
    action: { action },
  };
}

function previewAssertionCorrection() {
  try {
    const command = factsAssertionCorrectionCommand();
    pendingFactCorrectionCommands.assertion = command;
    const target = selectedFactRecord(factsCorrectionTarget);
    factsCorrectionPreviewResult.replaceChildren();
    appendText(factsCorrectionPreviewResult, "p", `Original: ${target.record_id} (angelegt in Revision ${target.created_revision}).`);
    appendText(factsCorrectionPreviewResult, "p", "Eine vollständige neue Assertion, die ausdrückliche Rücknahme des Originals und Corrects(neu, original) werden gemeinsam in genau einem Commit gespeichert.");
    const proposed = document.createElement("pre");
    proposed.textContent = JSON.stringify(command.replacement, null, 2);
    factsCorrectionPreviewResult.append(proposed);
    appendText(factsCorrectionPreviewResult, "p", `Rücknahmegrund: ${command.retraction_reason}. Die Vorschau ist unverbindlich; der Speicherpfad prüft den aktuellen Stand und alle Rechte erneut.` , "muted");
    pendingFactActionPreviews.assertion = true;
  } catch (error) {
    factsCorrectionPreviewResult.textContent = showError(error);
    pendingFactActionPreviews.assertion = false;
    pendingFactCorrectionCommands.assertion = null;
  }
  updateFactControls();
}

function previewEventCorrection() {
  try {
    const command = factsEventCorrectionCommand();
    pendingFactCorrectionCommands.event = command;
    factsEventCorrectionPreviewResult.replaceChildren();
    appendText(factsEventCorrectionPreviewResult, "p", `Original: ${command.target_event_id} (angelegt in Revision ${command.expected_target_created_revision}).`);
    appendText(factsEventCorrectionPreviewResult, "p", "Genau ein neuer Event und Corrects(neu, original) werden atomar gespeichert. Das Original bleibt aktiv; es wird keine EventRetraction erzeugt.");
    const proposed = document.createElement("pre");
    proposed.textContent = JSON.stringify(command.replacement, null, 2);
    factsEventCorrectionPreviewResult.append(proposed);
    appendText(factsEventCorrectionPreviewResult, "p", "Die Vorschau ist unverbindlich; Schema, Event-Zustand und Rechte werden beim Speichern erneut geprüft.", "muted");
    pendingFactActionPreviews.event = true;
  } catch (error) {
    factsEventCorrectionPreviewResult.textContent = showError(error);
    pendingFactActionPreviews.event = false;
    pendingFactCorrectionCommands.event = null;
  }
  updateFactControls();
}

function previewFactLifecycle() {
  try {
    const command = factsLifecycleCommand();
    const record = selectedFactRecord(factsLifecycleTarget, true);
    const action = command.action.action;
    factsLifecyclePreviewResult.textContent = action === "retract"
      ? `${record.family} ${record.record_id} wird mit einem neuen, begründeten Rücknahmedatensatz zurückgenommen.`
      : `${record.family} ${record.record_id} erhält einen neuen Archivübergang: ${action === "archive" ? "archiviert" : "aus dem Archiv geholt"}.`;
    appendText(factsLifecyclePreviewResult, "p", "Es wird kein bestehender Datensatz geändert. Die Änderung wird erst nach deiner Bestätigung ausgeführt und beim Commit erneut geprüft.", "muted");
    pendingFactActionPreviews.lifecycle = true;
  } catch (error) {
    factsLifecyclePreviewResult.textContent = showError(error);
    pendingFactActionPreviews.lifecycle = false;
  }
  updateFactControls();
}

function factsMaskSelectorInput() {
  if (factsMaskSelector.value === "exact_assertion") {
    const assertionId = factsMaskAssertionId.value.trim();
    if (!/^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(assertionId)) {
      throw new Error("invalid_request");
    }
    return { kind: "exact_assertion", assertion_id: assertionId };
  }
  if (!factsSubject.value || !factsPredicate.value) throw new Error("invalid_request");
  if (factsMaskSelector.value === "proposition") {
    return {
      kind: "proposition",
      subject_id: factsSubject.value,
      predicate_id: factsPredicate.value,
      value: factsValueInput(),
      polarity: factsPolarity.value,
    };
  }
  if (factsMaskSelector.value === "slot") {
    return { kind: "slot", subject_id: factsSubject.value, predicate_id: factsPredicate.value };
  }
  throw new Error("invalid_request");
}

function submitFact(buildCommand, label) {
  try {
    return publishFact(buildCommand(), label);
  } catch (error) {
    factsWriteStatus.textContent = showError(error);
    return Promise.resolve();
  }
}

function setFactsBusy(busy) {
  factBusy = busy;
  updateSchemaControls();
}

async function runFactsPreview(activeSessionId = sessionId, continuation = null) {
  if (!activeSessionId || !projectOpen) return;
  if (!continuation) invalidateFactsSearch();
  factsPreviewStatus.textContent = continuation
    ? "Weitere Suchtreffer werden geladen …"
    : "Abfrage wird ausgeführt …";
  factsPreviewButton.disabled = true;
  factsQueryContinueButton.disabled = true;
  factsQueryContinueButton.hidden = true;
  factsPreviewResults.replaceChildren();
  let response;
  try {
    const command = factsQueryCommand({ continuation });
    const signature = JSON.stringify({ ...command, continuation: null });
    if (continuation && signature !== factSearchBaseSignature) {
      throw new Error("query cursor is invalidated; restart the search");
    }
    response = await manageFacts(command, activeSessionId);
    if (response.kind !== "query") throw new Error("unsupported_protocol");
    renderFactsPreview(response);
    if (response.result.kind === "token_search") {
      factSearchCursor = response.result.next_cursor;
      factSearchBaseSignature = signature;
      factsQueryContinueButton.hidden = !factSearchCursor;
      factsQueryContinueButton.disabled = !factSearchCursor;
    } else {
      invalidateFactsSearch();
    }
  } catch (error) {
    if (continuation) invalidateFactsSearch();
    await invoke("facts_smoke_diagnostic", {
      details: JSON.stringify({ error: String(error?.stack ?? error), result: response?.result ?? null }),
    }).catch(() => {});
    factsPreviewStatus.textContent = showError(error);
  } finally {
    updateFactControls();
  }
}

function invalidateFactsSearch() {
  factSearchCursor = null;
  factSearchBaseSignature = null;
  if (factsQueryContinueButton) {
    factsQueryContinueButton.hidden = true;
    factsQueryContinueButton.disabled = true;
  }
}

async function publishFact(command, successLabel, expectedKind = "published") {
  if (!sessionId || !projectOpen || factBusy) return;
  setFactsBusy(true);
  factsWriteStatus.textContent = "Der Datensatz wird geprüft und gespeichert …";
  let publication;
  try {
    publication = await manageFacts(command);
    if (publication.kind === "event_graph_conflict") {
      const effects = publication.automatic_inference_applied
        ? " Der Graph hat eine automatische Relation ergänzt."
        : " Es wurde keine Relation aus Zeit oder Reihenfolge abgeleitet.";
      factsWriteStatus.textContent = `${publication.explanation} ${publication.relation_saved ? "Die Relation wurde gespeichert." : "Es wurde nichts gespeichert."}${effects}`;
      setFactsBusy(false);
      return;
    }
    if (publication.kind !== expectedKind) throw new Error("unsupported_protocol");
  } catch (error) {
    if (factsSmokeActive) {
      await invoke("facts_smoke_diagnostic", {
        details: JSON.stringify({ operation: command.command, error: String(error?.stack ?? error) }),
      }).catch(() => {});
    }
    factsWriteStatus.textContent = showError(error);
    setFactsBusy(false);
    return;
  }
  if (!publication) {
    factsWriteStatus.textContent = showError(new Error("unknown_commit_outcome"));
    setFactsBusy(false);
    return;
  }
  if (publication.kind === "assertion_corrected") {
    factsWriteStatus.textContent = `Assertion-Korrektur gemeinsam gespeichert · Revision ${publication.revision} · Original ${publication.target_assertion_id} zurückgenommen · Neue Assertion ${publication.replacement_assertion_id} · Corrects ${publication.corrects_provenance_id}`;
  } else if (publication.kind === "event_corrected") {
    factsWriteStatus.textContent = `Event-Korrektur gespeichert · Revision ${publication.revision} · Neues Event ${publication.replacement_event_id} · Corrects ${publication.corrects_provenance_id} · Original bleibt aktiv`;
  } else if (publication.kind === "lifecycle_changed") {
    factsWriteStatus.textContent = `${publication.target_family} ${publication.target_id}: ${publication.effect} · Revision ${publication.revision} · Lebenszyklusbeleg ${publication.record_id}`;
  } else {
    factsWriteStatus.textContent = `${successLabel} gespeichert · Revision ${publication.revision} · Beleg ${publication.record_id}`;
  }
  await refreshFactsCatalog(sessionId).catch((error) => {
    factsContextNote.textContent = showError(error);
  });
  factsPreviewStatus.textContent = "Der Schreibbeleg steht fest. Die Auflösungsvorschau wird separat neu berechnet …";
  await runFactsPreview(sessionId);
  setFactsBusy(false);
  clearFactActionPreviews();
}

function updateFactControls() {
  if (!factsPanel) return;
  const current = projectOpen && factCatalog;
  const canRead = current && !factBusy && !projectBusy;
  const canWrite = canRead && !hasUnresolvedOperation();
  const contextValid = Boolean(factsHistorySpace.value && factsLayer.value
    && (factsEpistemicMode.value === "world_state" || factsPerspective.value));
  const slotValid = Boolean(factsSubject.value && factsPredicate.value);
  const predicate = selectedFactPredicate();
  let valueValid = false;
  let validityValid = false;
  let queryTimeValid = false;
  let queryValid = false;
  let assertionCorrectionValid = false;
  let eventCorrectionValid = false;
  let eventCreateValid = false;
  let lifecycleValid = false;
  let sourceFieldsValid = false;
  try { factsValueInput(); valueValid = true; } catch {}
  try { factsValidityInput(); validityValid = true; } catch {}
  try { factsWorldTimeInput(); queryTimeValid = true; } catch {}
  try { factsQueryCommand(); queryValid = true; } catch {}
  try { factsAssertionCorrectionCommand(); assertionCorrectionValid = true; } catch {}
  try { factsEventCorrectionCommand(); eventCorrectionValid = true; } catch {}
  try { factsEventCreateCommand(); eventCreateValid = true; } catch {}
  try { factsLifecycleCommand(); lifecycleValid = true; } catch {}
  try { factsSourceFieldsInput(); sourceFieldsValid = true; } catch {}
  const eventMaskValid = Boolean(factsHistorySpace.value && factsLayer.value && factsEventMaskTarget.value);
  const eventRelationValid = Boolean(factsEventRelationFrom.value && factsEventRelationTo.value
    && factsEventRelationFrom.value !== factsEventRelationTo.value && factsEventRelationKind.value);
  const closeTime = factsEventSpanCloseNanoseconds.value.trim();
  let closeTimeValid = false;
  try {
    if (/^-?(0|[1-9][0-9]*)$/.test(closeTime)) {
      const value = BigInt(closeTime);
      closeTimeValid = value >= -(1n << 127n) && value <= (1n << 127n) - 1n;
    }
  } catch {}
  const eventSpanCloseValid = Boolean(factsEventSpanCloseTarget.value
    && factsEventSpanCloseTimeline.value && closeTimeValid);
  const maskSelectorValid = factsMaskSelector.value === "exact_assertion"
    ? /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(factsMaskAssertionId.value.trim())
    : factsMaskSelector.value === "proposition" ? valueValid : factsMaskSelector.value === "slot";
  factsPerspectiveWrap.hidden = factsEpistemicMode.value === "world_state";
  factsValidityRange.hidden = !factsValidityEnabled.checked;
  factsQueryPointWrap.hidden = factsQueryTimeMode.value !== "at";
  factsQueryNanosecondsWrap.hidden = factsQueryTimeMode.value !== "at";
  factsQuerySchemaRevisionWrap.hidden = factsQuerySchemaMode.value !== "explicit";
  factsQuerySearchControls.hidden = factsQueryOperation.value !== "token_search";
  factsQueryGraphControls.hidden = factsQueryOperation.value !== "graph";
  if (factsQueryOperation.value !== "token_search") invalidateFactsSearch();
  const explainOption = factsQueryOperation.querySelector('option[value="explain"]');
  if (explainOption) explainOption.disabled = factsQueryTimeMode.value !== "at";
  const tokenSearchOption = factsQueryOperation.querySelector('option[value="token_search"]');
  if (tokenSearchOption) tokenSearchOption.disabled = predicate?.details?.value_kind !== "string";
  factsMaskExactWrap.hidden = factsMaskSelector.value !== "exact_assertion";
  factsLifecycleReason.hidden = factsLifecycleAction.value !== "retract";
  factsMaskSelectorNote.textContent = factsMaskSelector.value === "slot"
    ? "Der Slot-Selector verwendet Subjekt, Prädikat und Perspektiv-/Epistemikpartition aus dem Kontext."
    : factsMaskSelector.value === "proposition"
      ? "Der Proposition-Selector verwendet Subjekt, Prädikat, Polarity und typisierten Wert aus den gemeinsamen Feldern."
      : "Die Assertion muss sichtbar sein und in einer niedrigeren Branch- oder Layer-Priorität liegen, damit die Mask sie übersteuern kann.";
  factsContextNote.textContent = current
    ? `Katalogstand Revision ${factCatalog.revision}. Die Auflösung fragt nur den ausgewählten Layer ab.`
    : "Aktueller Katalog wird geladen; bis dahin sind Schreibvorgänge gesperrt.";
  factsCreateAssertion.disabled = !canWrite || !contextValid || !slotValid || !validityValid || !valueValid;
  factsCreateMask.disabled = !canWrite || !contextValid || !slotValid || !maskSelectorValid
    || (factsValidityEnabled.checked && !validityValid);
  factsCreateBoundary.disabled = !canWrite || !contextValid || !slotValid
    || predicate?.details?.resolution_policy !== "multi_value_replace"
    || (factsValidityEnabled.checked && !validityValid);
  factsEventCreate.disabled = !canWrite || !eventCreateValid;
  factsEventMaskCreate.disabled = !canWrite || !eventMaskValid;
  factsEventRelationCreate.disabled = !canWrite || !eventRelationValid;
  factsEventSpanClose.disabled = !canWrite || !eventSpanCloseValid;
  const evidenceValid = Boolean(factsEvidenceSource.value && factsEvidenceTarget.value && factsEvidenceRelation.value);
  const provenanceValid = Boolean(factsProvenanceFrom.value && factsProvenanceTo.value
    && factsProvenanceFrom.value !== factsProvenanceTo.value && factsProvenanceRelation.value);
  factsSourceCreate.disabled = !canWrite || !sourceFieldsValid;
  factsSourceSupersede.disabled = !canWrite || !sourceFieldsValid || !factsSourceSupersedeTarget.value;
  factsEvidenceCreate.disabled = !canWrite || !evidenceValid;
  factsEvidenceRetract.disabled = !canWrite || !factsEvidenceRetractTarget.value || !factsEvidenceRetractReason.value.trim();
  factsProvenanceCreate.disabled = !canWrite || !provenanceValid;
  factsProvenanceRetract.disabled = !canWrite || !factsProvenanceRetractTarget.value || !factsProvenanceRetractReason.value.trim();
  factsPreviewButton.disabled = !canRead || !contextValid || !slotValid || !queryTimeValid || !queryValid;
  factsQueryContinueButton.disabled = !canRead || !slotValid
    || factsQueryOperation.value !== "token_search" || !factSearchCursor;
  factsCorrectionPreviewButton.disabled = !canRead || !assertionCorrectionValid || !slotValid || !validityValid || !valueValid;
  factsCorrectionCommitButton.disabled = !canWrite || !assertionCorrectionValid || !pendingFactActionPreviews.assertion;
  factsEventCorrectionPreviewButton.disabled = !canRead || !eventCorrectionValid;
  factsEventCorrectionCommitButton.disabled = !canWrite || !eventCorrectionValid || !pendingFactActionPreviews.event;
  const lifecycleRecord = selectedFactRecord(factsLifecycleTarget, true);
  const lifecycleAction = factsLifecycleAction.value;
  const lifecycleStateAllowsAction = Boolean(lifecycleRecord && factCatalog?.lifecycleVisible
    && ((lifecycleAction === "retract" && lifecycleRecord.retracted === false)
      || (lifecycleAction === "archive" && lifecycleRecord.archived === false)
      || (lifecycleAction === "unarchive" && lifecycleRecord.archived === true)));
  factsLifecyclePreviewButton.disabled = !canRead || !lifecycleValid || !lifecycleStateAllowsAction;
  factsLifecycleCommitButton.disabled = !canWrite || !lifecycleValid || !lifecycleStateAllowsAction
    || !pendingFactActionPreviews.lifecycle;
  factsWriteStatus.setAttribute("aria-busy", String(factBusy));
}

function renderFactsPreview(query) {
  const result = query.result;
  factsPreviewStatus.textContent = result.kind === "token_search" && !result.result_complete
    ? `Suchseite geladen · Revision ${query.snapshot_revision}`
    : `Abfrage abgeschlossen · Revision ${query.snapshot_revision}`;
  factsPreviewResults.replaceChildren();
  const binding = document.createElement("article");
  binding.className = "result-cell";
  appendText(binding, "h3", "Gebundener Abfragekontext");
  appendText(binding, "p", `Snapshot-Revision ${query.snapshot_revision} · RecordedAsOf ${query.recorded_as_of}`);
  appendText(binding, "p", `HistorySpace ${query.history_space_id} · Layer ${query.resolved_layer_ids.join(", ") || "keiner"}`);
  appendText(binding, "p", `Perspective ${query.perspective_id ?? "World"} · EpistemicMode ${query.epistemic_mode}`);
  const selectedWorldTime = query.world_time.kind === "all_times"
    ? "Alle Zeiten"
    : `${query.world_time.timeline_id} · ${query.world_time.nanoseconds} ns`;
  appendText(binding, "p", `WorldTime ${selectedWorldTime} · Abfrage ${query.query_mode}`);
  appendText(binding, "p", `Budgets · Kandidaten ${query.budget.max_candidates} · Arbeit ${query.budget.max_work_units} · Ergebnisse ${query.budget.max_results}`);
  const schemaMode = query.schema_mode.mode === "historical"
    ? `Historical (RecordedAsOf ${query.schema_mode.recorded_as_of})`
    : query.schema_mode.mode === "explicit"
      ? `Explicit (${query.schema_mode.revision})`
      : "Current";
  appendText(binding, "p", `SchemaMode ${schemaMode} · gebundene Schema-Revision ${query.schema_revision}`);
  factsPreviewResults.append(binding);

  if (result.kind === "history") {
    appendText(factsPreviewResults, "p", `Raw History · ${result.records.length} sichtbare Datensätze`);
    if (result.records.length === 0) appendText(factsPreviewResults, "p", "Keine passenden Rohdatensätze in dieser Historie.", "muted");
    for (const record of result.records) renderFactsHistoryRecord(record);
    return;
  }
  if (result.kind === "token_search") {
    appendText(factsPreviewResults, "p", `Wortsuche · ${result.hits.length} Treffer auf dieser Seite · Seitengröße ${result.page_size}`);
    appendText(factsPreviewResults, "p", result.result_complete
      ? "Alle Treffer wurden geladen."
      : "Diese Seite enthält noch nicht alle Treffer. Lade weitere Seiten, um die Suche abzuschließen.", result.result_complete ? "muted" : "schema-note");
    if (result.cursor_expires_at_ms) {
      appendText(factsPreviewResults, "p", "Der Fortsetzungscursor ist 60 Sekunden nach der ersten Seite gültig. Ist er abgelaufen, starte die Suche erneut.", "muted");
    }
    if (result.hits.length === 0) appendText(factsPreviewResults, "p", "Keine sichtbaren Treffer.", "muted");
    for (const hit of result.hits) {
      appendText(factsPreviewResults, "p", `${hit.family} ${hit.record_id} · Trefferfelder: ${hit.matched_fields.join(", ") || "keine"}`);
    }
    return;
  }
  if (result.kind === "graph") {
    appendText(factsPreviewResults, "p", `Vollständiger Graphdurchlauf · Tiefe ${result.max_depth_reached}/${result.max_depth_limit} · ${result.nodes.length} Knoten (Limit ${result.max_nodes_limit}) · ${result.edges.length} Beziehungen (Limit ${result.max_edges_limit})`);
    if (result.nodes.length === 0) appendText(factsPreviewResults, "p", "Kein sichtbarer Startknoten im gebundenen Abfragekontext.", "muted");
    for (const node of result.nodes) appendText(factsPreviewResults, "p", `Knoten · ${node.family} ${node.record_id}`);
    for (const edge of result.edges) {
      appendText(factsPreviewResults, "p", `Beziehung · ${edge.from.family} ${edge.from.record_id} —${edge.relationship}→ ${edge.to.family} ${edge.to.record_id} · ${edge.family} ${edge.record_id}`);
    }
    return;
  }
  if (result.kind === "aggregate") {
    if (result.result.kind === "count") {
      appendText(factsPreviewResults, "p", `COUNT · ${result.result.value} sichtbare aufgelöste Beiträge`);
    } else if (result.result.kind === "exists") {
      appendText(factsPreviewResults, "p", `EXISTS · ${result.result.value ? "Ja" : "Nein"}`);
    } else if (result.result.kind === "grouped_count") {
      appendText(factsPreviewResults, "p", "COUNT nach Polarity");
      if (result.result.groups.length === 0) appendText(factsPreviewResults, "p", "Keine Gruppen.", "muted");
      for (const group of result.result.groups) {
        appendText(factsPreviewResults, "p", `${group.polarity}: ${group.count}`);
      }
    }
    return;
  }
  if (result.kind === "resolved") {
    renderResolutionResult(factsPreviewResults, result.result);
    return;
  }
  if (result.kind === "explain") {
    appendText(factsPreviewResults, "p", `Explain · ${result.timeline_id} · ${result.nanoseconds} ns`);
    renderResolutionOutcome(factsPreviewResults, result.outcome);
    const labels = {
      candidate_scan: "Kandidatensuche",
      mask_projection: "Mask-Anwendung",
      replacement_boundary: "ReplacementBoundary-Anwendung",
      resolution: "Auflösung",
    };
    for (const stage of result.stages) {
      const card = document.createElement("article");
      card.className = "result-cell";
      appendText(card, "h3", labels[stage.kind] ?? stage.kind);
      appendText(card, "p", `Eingabe-Assertions: ${stage.input_assertions.join(", ") || "keine"}`);
      appendText(card, "p", `Ausgabe-Assertions: ${stage.output_assertions.join(", ") || "keine"}`);
      appendText(card, "p", `Angewandte Datensätze: ${stage.applied_records.map((item) => `${item.family} ${item.record_id}`).join(", ") || "keine"}`);
      factsPreviewResults.append(card);
    }
    return;
  }
  appendText(factsPreviewResults, "p", "Unbekannte Abfrageantwort.", "muted");
}

function renderFactsHistoryRecord(record) {
  const card = document.createElement("article");
  card.className = "result-cell";
  appendText(card, "h3", `${record.family} ${record.record_id} · Revision ${record.recorded_revision}`);
  appendText(card, "p", `Eigentümer-HistorySpace ${record.history_space_id}`);
  const detail = record.record;
  if (detail.kind === "assertion") {
    appendText(card, "p", `Kontext: ${formatFactsRecordContext(detail.context)}`);
    appendText(card, "p", `Assertion: Subjekt ${detail.subject_id} · Prädikat ${detail.predicate_id} · ${detail.polarity} · ${detail.value_kind} ${detail.value}`);
    appendText(card, "p", `Weltzeitgültigkeit: ${formatFactsValidity(detail.validity)}`);
  } else if (detail.kind === "assertion_retraction") {
    appendText(card, "p", `Nimmt Assertion ${detail.assertion_id} zurück · Grund: ${detail.reason}`);
  } else if (detail.kind === "mask") {
    appendText(card, "p", `Kontext: ${formatFactsRecordContext(detail.context)}`);
    appendText(card, "p", `Selector: ${formatFactsMaskSelector(detail.selector)}`);
    appendText(card, "p", `Weltzeitgültigkeit: ${detail.validity ? formatFactsValidity(detail.validity) : "alle Zeiten"}`);
  } else if (detail.kind === "mask_retraction") {
    appendText(card, "p", `Nimmt Mask ${detail.mask_id} zurück · Grund: ${detail.reason}`);
  } else if (detail.kind === "replacement_boundary") {
    appendText(card, "p", `Kontext: ${formatFactsRecordContext(detail.context)}`);
    appendText(card, "p", `Slot: Subjekt ${detail.subject_id} · Prädikat ${detail.predicate_id}`);
    appendText(card, "p", `Weltzeitgültigkeit: ${detail.validity ? formatFactsValidity(detail.validity) : "alle Zeiten"}`);
  } else if (detail.kind === "replacement_boundary_retraction") {
    appendText(card, "p", `Nimmt ReplacementBoundary ${detail.replacement_boundary_id} zurück · Grund: ${detail.reason}`);
  }
  factsPreviewResults.append(card);
}

function formatFactsRecordContext(context) {
  return `HistorySpace ${context.history_space_id} · Layer ${context.layer_id} · Perspective ${context.perspective_id ?? "World"} · ${context.epistemic_mode}`;
}

function formatFactsValidity(validity) {
  return `${validity.timeline_id} · [${validity.start_nanoseconds ?? "−∞"}, ${validity.end_nanoseconds ?? "+∞"}) ns`;
}

function formatFactsMaskSelector(selector) {
  if (selector.kind === "exact_assertion") return `konkrete Assertion ${selector.assertion_id}`;
  if (selector.kind === "proposition") {
    return `Proposition ${selector.subject_id} / ${selector.predicate_id} · ${selector.polarity} · ${selector.value_kind} ${selector.value}`;
  }
  return `Slot ${selector.subject_id} / ${selector.predicate_id} · Perspective ${selector.perspective_id ?? "World"} · ${selector.epistemic_mode}`;
}

function renderResolutionResult(parent, result) {
  if (result.kind === "complete_empty") {
    appendText(parent, "p", "CompleteEmpty: Die vollständige Historie enthält keine Assertion- oder Boundary-Zeitdomäne.");
    return;
  }
  if (result.kind === "point") {
    appendText(parent, "p", `Zeitpunkt ${result.timeline_id} · ${result.nanoseconds} ns`);
    renderResolutionOutcome(parent, result.outcome);
    return;
  }
  if (result.kind === "all_times") {
    if (result.slices.length === 0) {
      appendText(parent, "p", "Ungültige leere AllTimes-Antwort.", "muted");
      return;
    }
    for (const slice of result.slices) {
      const cell = document.createElement("article");
      cell.className = "result-cell";
      const start = slice.start_nanoseconds ?? "−∞";
      const end = slice.end_nanoseconds ?? "+∞";
      appendText(cell, "h3", `${slice.timeline_id} · [${start}, ${end}) ns`);
      renderResolutionOutcome(cell, slice.outcome);
      parent.append(cell);
    }
  }
}

function renderResolutionOutcome(parent, outcome) {
  const label = ({ known: "Known", unknown: "Unknown", conflict: "Conflict" })[outcome.kind] ?? "Unbekannt";
  appendText(parent, "p", `Ergebnis: ${label}`, outcome.kind === "conflict" ? "schema-note" : undefined);
  if (outcome.kind === "known" || outcome.kind === "conflict") {
    for (const entry of outcome.values) {
      appendText(parent, "p", `${entry.value} · ${entry.polarity} · Assertionen: ${entry.contributors.join(", ") || "keine"}`);
    }
    for (const conflict of outcome.conflicts ?? []) {
      appendText(parent, "p", `Widerspruch bei ${conflict.value}: positiv ${conflict.positive_contributors.join(", ")}; negativ ${conflict.negative_contributors.join(", ")}`, "schema-note");
    }
    if (outcome.kind === "conflict" && outcome.conflicts.length === 0) {
      appendText(parent, "p", `Gleichrangige Assertions widersprechen sich. Beteiligte Assertions: ${outcome.contributors.join(", ")}`, "schema-note");
    }
  }
}

function clearTransferPreview() {
  transferPreviewTicket = null;
  transferPreviewBaseRevision = null;
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
    transferPreviewBaseRevision = result.target_head;
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
    transferPreviewBaseRevision = null;
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
  return invokeManagedCommand("manage_security_policy", command, activeSessionId,
    (response) => response.protocol_version === 1 && Boolean(response.result?.kind));
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
  return invokeManagedCommand("manage_perspectives", command, activeSessionId,
    (response) => response.protocol_version === 1 && Boolean(response.result?.kind));
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

  schemaFamily.value = "timeline";
  schemaSymbol.value = "ipc_smoke_timeline";
  timelineCalendarProfile.value = "proleptic_gregorian_utc";
  timelineEpochUnixNanoseconds.value = "-170141183460469231731687303715884105728";
  updateSchemaFormVisibility();
  const timelineDraft = buildDefinitionDraft();
  if (timelineDraft.calendar_profile.epoch_unix_nanoseconds !== "-170141183460469231731687303715884105728") {
    throw new Error("timeline form did not preserve the exact signed epoch");
  }
  timelineEpochUnixNanoseconds.value = "170141183460469231731687303715884105728";
  let invalidEpochRejected = false;
  try {
    buildDefinitionDraft();
  } catch {
    invalidEpochRejected = true;
  }
  if (!invalidEpochRejected) throw new Error("timeline form accepted an epoch outside i128 range");
  timelineEpochUnixNanoseconds.value = "-170141183460469231731687303715884105728";
  await publishDefinition();
  const timeline = selectedSchema?.definitions.find((item) => item.symbol === "ipc_smoke_timeline");
  if (timeline?.family !== "timeline" || timeline.details.epoch_unix_nanoseconds !== "-170141183460469231731687303715884105728") {
    throw new Error("timeline calendar profile and epoch were not published exactly");
  }

  schemaFamily.value = "time_unit";
  schemaSymbol.value = "ipc_smoke_max_scale";
  timeUnitNanosecondsPerTick.value = "18446744073709551615";
  updateSchemaFormVisibility();
  const timeUnitDraft = buildDefinitionDraft();
  if (timeUnitDraft.nanoseconds_per_tick !== "18446744073709551615") {
    throw new Error("time-unit form did not preserve the exact u64 scale");
  }
  timeUnitNanosecondsPerTick.value = "0";
  let zeroScaleRejected = false;
  try {
    buildDefinitionDraft();
  } catch {
    zeroScaleRejected = true;
  }
  if (!zeroScaleRejected) throw new Error("time-unit form accepted a zero scale");
  timeUnitNanosecondsPerTick.value = "18446744073709551615";
  await publishDefinition();
  const timeUnit = selectedSchema?.definitions.find((item) => item.symbol === "ipc_smoke_max_scale");
  if (timeUnit?.family !== "time_unit" || timeUnit.details.nanoseconds_per_tick !== "18446744073709551615") {
    throw new Error("time-unit scale was not published exactly");
  }

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
  if (historical.definitions.some((item) => item.identity === timeline.identity || item.identity === timeUnit.identity)) {
    throw new Error("historical schema view included a later timeline or time unit");
  }
  if (!explicit.definitions.some((item) => item.identity === definition.identity)) {
    throw new Error("explicit schema view omitted the published definition");
  }
  if (!explicit.definitions.some((item) => item.identity === timeline.identity)
    || !explicit.definitions.some((item) => item.identity === timeUnit.identity)) {
    throw new Error("explicit schema view omitted a timeline or time unit");
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

  for (const timeDefinition of [timeline, timeUnit]) {
    stageDefinitionLifecycle(timeDefinition, "deprecated");
    await publishLifecycleBatch();
    const deprecated = selectedSchema?.definitions.find((item) => item.identity === timeDefinition.identity);
    if (deprecated?.lifecycle !== "deprecated" || !renderedDefinitionText(deprecated).includes(lifecycleLabel("deprecated"))) {
      throw new Error(`${timeDefinition.family} deprecation was not visible after publication`);
    }
    stageDefinitionLifecycle(deprecated, "retired");
    await publishLifecycleBatch();
    const retired = selectedSchema?.definitions.find((item) => item.identity === timeDefinition.identity);
    if (retired?.lifecycle !== "retired" || !renderedDefinitionText(retired).includes(lifecycleLabel("retired"))) {
      throw new Error(`${timeDefinition.family} retirement was not visible after publication`);
    }
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

async function waitForFactWrite(label) {
  const expectedPrefix = `${label} gespeichert · Revision `;
  const deadline = Date.now() + 30000;
  while (Date.now() < deadline) {
    const status = factsWriteStatus.textContent;
    if (status.startsWith(expectedPrefix)) {
      const receipt = status.match(/Revision ([0-9]+) · Beleg ([0-9a-f-]{36})$/i);
      if (!receipt) throw new Error(`${label} write did not expose its revision-bound record receipt`);
      return { revision: Number(receipt[1]), recordId: receipt[2] };
    }
    if (status && !status.startsWith("Der Datensatz wird geprüft und gespeichert …")) {
      throw new Error(`${label} write was rejected: ${status}`);
    }
    await new Promise((resolve) => window.setTimeout(resolve, 50));
  }
  throw new Error(`Timed out waiting for the ${label} write receipt`);
}

async function waitForFactPreview(expectedRevision = null) {
  const deadline = Date.now() + 30000;
  while (Date.now() < deadline) {
    const completed = factsPreviewStatus.textContent.match(/^(?:Abfrage abgeschlossen|Suchseite geladen) · Revision ([0-9]+)$/);
    if (completed) {
      if (expectedRevision === null || Number(completed[1]) >= expectedRevision) return;
      await new Promise((resolve) => window.setTimeout(resolve, 50));
      continue;
    }
    if (factsPreviewStatus.textContent
      && !factsPreviewStatus.textContent.startsWith("Der Schreibbeleg steht fest.")
      && !factsPreviewStatus.textContent.startsWith("Abfrage wird ausgeführt …")
      && !factsPreviewStatus.textContent.startsWith("Weitere Suchtreffer werden geladen …")) {
      throw new Error(`Query was rejected: ${factsPreviewStatus.textContent}`);
    }
    await new Promise((resolve) => window.setTimeout(resolve, 50));
  }
  throw new Error("Timed out waiting for the resolution preview result");
}

async function clickFactWrite(button, label) {
  if (button.disabled) throw new Error(`${label} form remained disabled with a complete valid fixture`);
  factsPreviewStatus.textContent = "";
  button.click();
  const receipt = await waitForFactWrite(label);
  await waitForFactPreview(receipt.revision);
  return receipt;
}

async function clickFactPreview() {
  if (factsPreviewButton.disabled) {
    const diagnostics = {
      projectOpen,
      projectBusy,
      factBusy,
      projectRevision,
      catalogRevision: factCatalog?.revision ?? null,
      context: factsContextInput(),
      subject: factsSubject.value,
      predicate: factsPredicate.value,
      queryTimeMode: factsQueryTimeMode.value,
      queryTimeline: factsQueryTimeline.value,
      queryNanoseconds: factsQueryNanoseconds.value,
    };
    throw new Error(`resolution preview form remained disabled: ${JSON.stringify(diagnostics)}`);
  }
  factsPreviewStatus.textContent = "";
  factsPreviewButton.click();
  await waitForFactPreview();
  return factsPreviewResults.textContent;
}

async function waitForFactAction(prefix) {
  const deadline = Date.now() + 30000;
  while (Date.now() < deadline) {
    const status = factsWriteStatus.textContent;
    if (status.startsWith(prefix)) return status;
    if (status && !status.startsWith("Der Datensatz wird geprüft und gespeichert …")) {
      throw new Error(`factual lifecycle action was rejected: ${status}`);
    }
    await new Promise((resolve) => window.setTimeout(resolve, 50));
  }
  throw new Error(`timed out waiting for factual lifecycle action: ${prefix}`);
}

async function commitPreviewedFactAction(previewButton, commitButton, statusPrefix) {
  if (previewButton.disabled) throw new Error("factual action preview remained disabled");
  previewButton.click();
  if (commitButton.disabled) throw new Error("factual action confirmation did not follow the preview");
  commitButton.click();
  const status = await waitForFactAction(statusPrefix);
  const revision = status.match(/Revision ([0-9]+)/);
  if (!revision) throw new Error("factual action receipt omitted its shared revision");
  await waitForFactPreview(Number(revision[1]));
  return status;
}

async function recordFactsSmokeStage(stage) {
  await invoke("facts_smoke_diagnostic", { details: `facts-smoke:${stage}` }).catch(() => {});
}

async function createFactsMaskBranch(activeSessionId, parentHistorySpaceId, cutoffRevision, stage = "mask-branch") {
  await recordFactsSmokeStage(`${stage}:before-snapshot`);
  const before = await manageBranchLayers({ command: "snapshot", mode: { mode: "current" } }, activeSessionId);
  await recordFactsSmokeStage(`${stage}:before-create`);
  const existing = new Set(before.branches.map((branch) => branch.history_space_id));
  const created = await manageBranchLayers({
    command: "create_child",
    expected_base_revision: before.revision,
    parent_history_space_id: parentHistorySpaceId,
    base_revision: cutoffRevision,
  }, activeSessionId);
  await recordFactsSmokeStage(`${stage}:after-create`);
  if (created.kind !== "published") throw new Error("the facts smoke overlay Branch was not published");
  const after = await manageBranchLayers({ command: "snapshot", mode: { mode: "current" } }, activeSessionId);
  await recordFactsSmokeStage(`${stage}:after-verify-snapshot`);
  const branch = after.branches.find((item) => !existing.has(item.history_space_id)
    && item.parent_history_space_id === parentHistorySpaceId
    && item.base_revision === cutoffRevision);
  if (!branch) throw new Error("the facts smoke overlay Branch did not preserve its parent cutoff");
  await refreshFactsCatalog(activeSessionId);
  await recordFactsSmokeStage(`${stage}:after-catalog`);
  return branch;
}

async function runFactsSmoke(activeSessionId) {
  const privateCanary = "WDB_INTERNAL_CAUSE_CANARY_93D1";
  const publicError = await invoke("diagnostic_smoke_canary", { sessionId: activeSessionId });
  const serializedError = JSON.stringify(publicError);
  const displayedError = showError({
    ...publicError,
    detail: privateCanary,
    technical_detail: privateCanary,
    message: privateCanary,
    message_key: privateCanary,
    next_action_key: privateCanary,
  });
  diagnosticStatus.textContent = displayedError;
  if (publicError?.code !== "migration_rejected"
    || publicError?.message_key !== "worlddb.error.migration_rejected"
    || serializedError.includes(privateCanary)
    || diagnosticStatus.textContent.includes(privateCanary)) {
    throw new Error("Die öffentliche Diagnose hat interne Fehlerdetails offengelegt.");
  }
  await recordFactsSmokeStage("diagnostic-canary:rejected");

  let rendererDiagnosticPathRejected = false;
  try {
    await invoke("export_diagnostics", {
      sessionId: activeSessionId,
      request: { protocol_version: 1, path: "C:/renderer/selected/diagnostics.json" },
    });
  } catch {
    rendererDiagnosticPathRejected = true;
  }
  if (!rendererDiagnosticPathRejected) {
    throw new Error("Der Diagnoseexport hat einen Rendererpfad angenommen.");
  }
  await recordFactsSmokeStage("diagnostic-renderer-paths:rejected");
  diagnosticStatus.textContent = "";

  factsQueryRecordedAsOf.dataset.auto = "true";
  factsQuerySchemaRevision.dataset.auto = "true";
  factsQueryOperation.value = "resolved";
  factsQuerySchemaMode.value = "historical";
  schemaViewMode.value = "current";
  schemaCurrentMode = true;
  schemaFamily.value = "predicate";
  schemaSymbol.value = "ipc_smoke_facts";
  schemaSubjectType.value = "any";
  schemaValueKind.value = "string";
  schemaCardinality.value = "multi";
  schemaResolution.value = "multi_value_replace";
  updateSchemaFormVisibility();
  await publishDefinition();
  const predicate = selectedSchema?.definitions.find((item) => item.family === "predicate" && item.symbol === "ipc_smoke_facts");
  if (predicate?.lifecycle !== "active" || predicate.details.resolution_policy !== "multi_value_replace") {
    throw new Error("the smoke predicate was not published with MultiValueReplace resolution");
  }

  schemaFamily.value = "timeline";
  schemaSymbol.value = "ipc_smoke_facts_timeline";
  timelineCalendarProfile.value = "none";
  updateSchemaFormVisibility();
  await publishDefinition();
  const timeline = selectedSchema?.definitions.find((item) => item.family === "timeline" && item.symbol === "ipc_smoke_facts_timeline");
  if (timeline?.lifecycle !== "active") throw new Error("the smoke Timeline was not active after publication");

  entityViewMode.value = "current";
  entityCurrentMode = true;
  await refreshEntities(activeSessionId);
  const subjectType = selectedEntities.entity_types.find((item) => item.symbol === "ipc_smoke_entity_available" && item.lifecycle === "active");
  if (!subjectType) throw new Error("the smoke EntityType was not active");
  entityTypeSelect.value = subjectType.entity_type_id;
  entityAcceptDeprecated.checked = false;
  updateEntityTypeSelectionState();
  const createdEntity = await createEntity({ propagateErrors: true });
  if (createdEntity?.kind !== "published" || !createdEntity.entity_id) throw new Error("the smoke subject Entity was not persisted");

  schemaFamily.value = "event_kind";
  schemaSymbol.value = "ipc_smoke_event";
  stagedEventRoles = [{
    symbol: "actor",
    entity_constraint: { kind: "exact", entity_type_id: subjectType.entity_type_id },
    min_participants: 1,
    max_participants: 1,
  }];
  stagedEventAttributes = [{
    symbol: "summary",
    value_kind: "string",
    object_constraint: null,
    constraints: [],
    decimal_metadata: null,
    required: true,
  }];
  eventTimeForm.value = "open_span_allowed";
  updateSchemaFormVisibility();
  await publishDefinition();
  const eventKind = selectedSchema?.definitions.find((item) => item.family === "event_kind" && item.symbol === "ipc_smoke_event");
  const eventRole = eventKind?.details.roles?.[0];
  const eventAttribute = eventKind?.details.attributes?.[0];
  if (!eventKind || !eventRole || !eventAttribute || eventKind.details.event_time.form !== "open_span_allowed") {
    throw new Error("the smoke EventKind did not publish its required role, typed attribute, and open-span form");
  }

  const branchSnapshot = await manageBranchLayers({ command: "snapshot", mode: { mode: "current" } }, activeSessionId);
  const rootBranch = branchSnapshot.branches.find((branch) => branch.parent_history_space_id == null);
  const activeBaseLayer = branchSnapshot.layers.find((layer) => layer.is_base && layer.lifecycle === "active");
  if (!rootBranch || !activeBaseLayer) throw new Error("facts smoke did not resolve an active query branch and base Layer");
  const initialPreview = await manageFacts({
    command: "preview",
    context: {
      history_space_id: rootBranch.history_space_id,
      layer_id: activeBaseLayer.layer_id,
      perspective_id: null,
      epistemic_mode: "world_state",
    },
    subject_id: createdEntity.entity_id,
    predicate_id: predicate.identity,
    world_time: { kind: "all_times" },
  }, activeSessionId);
  if (initialPreview.kind !== "preview" || initialPreview.result.kind !== "complete_empty") {
    throw new Error("the empty facts slot did not return an explicit CompleteEmpty preview");
  }
  await refreshFactsCatalog(activeSessionId);
  const root = rootBranch;
  const baseLayer = activeBaseLayer;
  if (!root || !baseLayer) throw new Error("facts form did not load the root branch and active base Layer");
  const catalogPreview = await manageFacts({
    command: "preview",
    context: {
      history_space_id: root.history_space_id,
      layer_id: baseLayer.layer_id,
      perspective_id: null,
      epistemic_mode: "world_state",
    },
    subject_id: createdEntity.entity_id,
    predicate_id: predicate.identity,
    world_time: { kind: "all_times" },
  }, activeSessionId);
  if (catalogPreview.kind !== "preview" || catalogPreview.result.kind !== "complete_empty") {
    throw new Error("the facts form catalog did not bind the new slot to the CompleteEmpty preview");
  }
  factsHistorySpace.value = root.history_space_id;
  factsLayer.value = baseLayer.layer_id;
  factsEpistemicMode.value = "world_state";
  factsPerspective.value = "";
  factsSubject.value = createdEntity.entity_id;
  factsPredicate.value = predicate.identity;
  factsValidityTimeline.value = timeline.identity;
  factsValidityEnabled.checked = false;
  factsQueryTimeMode.value = "all_times";
  factsQueryOperation.value = "resolved";
  factsQuerySchemaMode.value = "historical";
  factsPolarity.value = "positive";
  updateFactsValueFields();
  const emptyHistoryPreview = await manageFacts(factsQueryCommand(), activeSessionId);
  if (emptyHistoryPreview.kind !== "query" || emptyHistoryPreview.query_mode !== "resolved"
    || emptyHistoryPreview.result.kind !== "resolved"
    || emptyHistoryPreview.result.result.kind !== "complete_empty") {
    throw new Error("the empty facts slot did not return an explicitly bound CompleteEmpty query");
  }

  factsValueText.value = "Exact-Mask-Target";
  const staleCommitBaseRevision = factCatalog.revision;
  const assertion = await clickFactWrite(factsCreateAssertion, "Assertion");
  await waitForFactPreview();
  if (!factsPreviewResults.textContent.includes("Ergebnis: Known")
    || !factsPreviewResults.textContent.includes("Exact-Mask-Target")) {
    throw new Error("the all-times form preview did not show the persisted Assertion as Known");
  }

  const staleCommit = await manageFacts({
    command: "create_assertion",
    expected_base_revision: staleCommitBaseRevision,
    context: factsContextInput(),
    subject_id: factsSubject.value,
    predicate_id: factsPredicate.value,
    value: factsValueInput(),
    polarity: factsPolarity.value,
    validity: factsValidityInput(),
  }, activeSessionId).then(() => null, (error) => error);
  if (errorCode(staleCommit) !== "commit_conflict"
    || !operationStatus.textContent.includes(`Revision ${staleCommitBaseRevision}`)
    || !operationStatus.textContent.includes("Es wurde nichts gespeichert.")) {
    throw new Error("the stale write did not return the safe ConflictReport through OperationId status");
  }
  await recordFactsSmokeStage("commit-conflict:confirmed");

  const unknownCommitOperationId = "00000000-0000-7000-8000-000000000041";
  await refreshFactsCatalog(activeSessionId);
  const unknownCommitBaseRevision = factCatalog.revision;
  factsValueText.value = "Unknown-Commit-Response";
  const unknownCommit = await manageFacts({
    command: "create_assertion",
    expected_base_revision: unknownCommitBaseRevision,
    context: factsContextInput(),
    subject_id: factsSubject.value,
    predicate_id: factsPredicate.value,
    value: factsValueInput(),
    polarity: factsPolarity.value,
    validity: factsValidityInput(),
  }, activeSessionId, unknownCommitOperationId).then(() => null, (error) => error);
  if (errorCode(unknownCommit) !== "commit_confirmed"
    || unknownCommit.operation_id !== unknownCommitOperationId
    || !operationStatus.textContent.includes(unknownCommitOperationId)
    || !operationStatus.textContent.includes(`Revision ${unknownCommit.revision}`)) {
    throw new Error(`the lost commit reply was not reconciled as committed under its original OperationId (code=${errorCode(unknownCommit)}, writeError=${unknownCommit?.write_error_code ?? "none"}, id=${safeOperationId(unknownCommit?.operation_id) ?? "missing"}, expectedBase=${safeRevision(unknownCommitBaseRevision)}, current=${safeRevision(projectRevision)}, pending=${pendingOperations.length}, status=${operationStatus.textContent})`);
  }
  await refreshFactsCatalog(activeSessionId);
  if (factCatalog.revision !== unknownCommit.revision) {
    throw new Error("the reconciled commit receipt revision did not match the refreshed database head");
  }
  factsValueText.value = "Exact-Mask-Target";
  await recordFactsSmokeStage("unknown-commit:resolved");

  factsQueryTimeMode.value = "at";
  factsQueryTimeline.value = timeline.identity;
  factsQueryNanoseconds.value = "0";
  updateFactControls();
  const pointPreview = await clickFactPreview();
  if (!pointPreview.includes("Ergebnis: Known") || !pointPreview.includes("Exact-Mask-Target")) {
    throw new Error("the point-selector form preview did not resolve the persisted Assertion");
  }
  factsQueryTimeMode.value = "all_times";
  updateFactControls();

  const exactMaskBranch = await createFactsMaskBranch(activeSessionId, root.history_space_id, assertion.revision);
  factsHistorySpace.value = exactMaskBranch.history_space_id;
  updateFactControls();
  factsMaskSelector.value = "exact_assertion";
  factsMaskAssertionId.value = assertion.recordId;
  updateFactControls();
  const exactMask = await clickFactWrite(factsCreateMask, "Mask");
  await waitForFactPreview();
  if (!factsPreviewResults.textContent.includes("Ergebnis: Unknown")) {
    throw new Error("the exact-Assertion Mask did not make the preview Unknown");
  }

  factsLifecycleTarget.value = `mask:${exactMask.recordId}`;
  factsLifecycleAction.value = "retract";
  factsLifecycleReason.value = "IPC smoke: separate explicit Mask retraction";
  updateFactControls();
  await commitPreviewedFactAction(
    factsLifecyclePreviewButton,
    factsLifecycleCommitButton,
    `mask ${exactMask.recordId}: retracted · Revision `,
  );
  await waitForFactPreview();
  if (!factsPreviewResults.textContent.includes("Ergebnis: Known")) {
    throw new Error("explicit Mask retraction did not restore the unmasked Assertion");
  }

  factsLifecycleTarget.value = `assertion:${assertion.recordId}`;
  factsLifecycleAction.value = "archive";
  updateFactControls();
  await commitPreviewedFactAction(
    factsLifecyclePreviewButton,
    factsLifecycleCommitButton,
    `assertion ${assertion.recordId}: archived · Revision `,
  );
  factsLifecycleTarget.value = `assertion:${assertion.recordId}`;
  factsLifecycleAction.value = "unarchive";
  updateFactControls();
  await commitPreviewedFactAction(
    factsLifecyclePreviewButton,
    factsLifecycleCommitButton,
    `assertion ${assertion.recordId}: unarchived · Revision `,
  );

  factsHistorySpace.value = root.history_space_id;
  updateFactControls();
  factsValueText.value = "Proposition-Mask-Target";
  const propositionAssertion = await clickFactWrite(factsCreateAssertion, "Assertion");
  await waitForFactPreview();
  const propositionMaskBranch = await createFactsMaskBranch(
    activeSessionId,
    root.history_space_id,
    propositionAssertion.revision,
  );
  factsHistorySpace.value = propositionMaskBranch.history_space_id;
  updateFactControls();
  factsMaskSelector.value = "proposition";
  updateFactControls();
  await clickFactWrite(factsCreateMask, "Mask");
  await waitForFactPreview();

  factsHistorySpace.value = root.history_space_id;
  updateFactControls();
  factsValueText.value = "Slot-Mask-Target";
  const slotAssertion = await clickFactWrite(factsCreateAssertion, "Assertion");
  await waitForFactPreview();
  const slotMaskBranch = await createFactsMaskBranch(
    activeSessionId,
    root.history_space_id,
    slotAssertion.revision,
  );
  factsHistorySpace.value = slotMaskBranch.history_space_id;
  updateFactControls();
  factsMaskSelector.value = "slot";
  updateFactControls();
  const slotMask = await clickFactWrite(factsCreateMask, "Mask");
  await waitForFactPreview();
  if (!factsPreviewResults.textContent.includes("Ergebnis: Unknown")) {
    throw new Error("the complete Slot Mask did not make the preview Unknown");
  }

  const slotBoundary = await clickFactWrite(factsCreateBoundary, "ReplacementBoundary");
  await waitForFactPreview();
  if (!factsPreviewResults.textContent.includes("Ergebnis: Known")) {
    throw new Error("the MultiValueReplace boundary did not expose its known empty-set resolution");
  }

  const resolvedQuery = await manageFacts(factsQueryCommand(), activeSessionId);
  if (resolvedQuery.kind !== "query" || resolvedQuery.result.kind !== "resolved"
    || resolvedQuery.result.result.kind !== "all_times") {
    throw new Error("Resolved View did not return a complete all-times query result");
  }
  const resolvedText = await clickFactPreview();
  if (!resolvedText.includes("Ergebnis: Known")) throw new Error("Resolved View was not rendered in the desktop query panel");

  factsQueryOperation.value = "history";
  updateFactControls();
  const rawHistoryQuery = await manageFacts(factsQueryCommand(), activeSessionId);
  if (rawHistoryQuery.kind !== "query" || rawHistoryQuery.result.kind !== "history"
    || !rawHistoryQuery.result.records.some((item) => item.record_id === slotAssertion.recordId
      && item.record.kind === "assertion" && item.record.value === "Slot-Mask-Target")
    || !rawHistoryQuery.result.records.some((item) => item.record_id === slotMask.recordId
      && item.record.kind === "mask")
    || !rawHistoryQuery.result.records.some((item) => item.record_id === slotBoundary.recordId
      && item.record.kind === "replacement_boundary")) {
    throw new Error("Raw History did not expose the selected Assertion, Mask, and Boundary payloads");
  }
  const historyText = await clickFactPreview();
  if (!historyText.includes("Raw History") || !historyText.includes(slotMask.recordId)
    || !historyText.includes("Slot-Mask-Target")) {
    throw new Error("Raw History was not rendered with its assertion value and Mask record");
  }

  factsHistorySpace.value = root.history_space_id;
  factsQueryOperation.value = "resolved";
  factsQueryTimeMode.value = "at";
  factsQueryTimeline.value = timeline.identity;
  factsQueryNanoseconds.value = "0";
  updateFactControls();
  const currentHead = factCatalog.queryRevision;
  const historicalQuery = await manageFacts({
    ...factsQueryCommand(),
    recorded_as_of: String(slotAssertion.revision),
    schema_mode: { mode: "historical", recorded_as_of: String(slotAssertion.revision) },
  }, activeSessionId);
  if (historicalQuery.kind !== "query" || historicalQuery.snapshot_revision !== currentHead
    || historicalQuery.recorded_as_of !== String(slotAssertion.revision)
    || historicalQuery.result.kind !== "resolved"
    || historicalQuery.result.result.kind !== "point"
    || historicalQuery.result.result.outcome.kind !== "known") {
    throw new Error("the Historical RecordedAsOf query did not preserve its separate live Snapshot binding");
  }

  factsHistorySpace.value = slotMaskBranch.history_space_id;
  factsQueryOperation.value = "explain";
  factsQuerySchemaMode.value = "current";
  updateFactControls();
  const explainQuery = await manageFacts(factsQueryCommand(), activeSessionId);
  if (explainQuery.kind !== "query" || explainQuery.result.kind !== "explain"
    || !explainQuery.result.stages.some((stage) => stage.applied_records.some((item) =>
      item.family === "mask" && item.record_id === slotMask.recordId))
    || !explainQuery.result.stages.some((stage) => stage.applied_records.some((item) =>
      item.family === "replacement_boundary" && item.record_id === slotBoundary.recordId))) {
    throw new Error("Explain did not show the applied Mask and ReplacementBoundary records");
  }
  const explainText = await clickFactPreview();
  if (!explainText.includes("Explain") || !explainText.includes(slotMask.recordId)
    || !explainText.includes(slotBoundary.recordId)) {
    throw new Error("Explain was not rendered with its applied Mask and ReplacementBoundary IDs");
  }

  factsQueryOperation.value = "resolved";
  factsQuerySchemaMode.value = "explicit";
  factsQuerySchemaRevision.value = factCatalog.queryRevision;
  updateFactControls();
  const explicitSchemaQuery = await manageFacts(factsQueryCommand(), activeSessionId);
  if (explicitSchemaQuery.kind !== "query"
    || explicitSchemaQuery.schema_mode.mode !== "explicit"
    || explicitSchemaQuery.schema_revision !== factCatalog.queryRevision) {
    throw new Error("Explicit SchemaMode did not retain its selected schema revision in the query result");
  }

  factsHistorySpace.value = root.history_space_id;
  factsLayer.value = baseLayer.layer_id;
  factsQueryTimeMode.value = "all_times";
  factsQuerySchemaMode.value = "current";
  factsQueryRecordedAsOf.dataset.auto = "true";
  factsQuerySchemaRevision.dataset.auto = "true";
  factsValueText.value = "Slot-Mask-Target";
  factsPolarity.value = "positive";
  updateFactsValueFields();
  updateFactControls();
  await clickFactWrite(factsCreateAssertion, "Assertion");

  factsQueryOperation.value = "token_search";
  factsQuerySearchTerms.value = "Slot-Mask-Target";
  factsQuerySearchMatch.value = "all_terms";
  factsQueryPageSize.value = "1";
  updateFactControls();
  const firstSearchPage = await manageFacts(factsQueryCommand(), activeSessionId);
  if (firstSearchPage.kind !== "query" || firstSearchPage.result.kind !== "token_search"
    || firstSearchPage.result.hits.length !== 1 || !firstSearchPage.result.next_cursor
    || firstSearchPage.result.result_complete) {
    throw new Error("TokenSearch did not return an explicitly incomplete first page and cursor");
  }
  const firstSearchText = await clickFactPreview();
  if (!firstSearchText.includes("Wortsuche") || firstSearchText.includes("Slot-Mask-Target")
    || !firstSearchText.includes("60 Sekunden")
    || factsQueryContinueButton.hidden || factsQueryContinueButton.disabled) {
    throw new Error("TokenSearch did not show a safe, paginated result with its continuation action");
  }
  factsQueryContinueButton.click();
  await waitForFactPreview();
  if (!factsPreviewResults.textContent.includes("Alle Treffer wurden geladen")) {
    throw new Error("TokenSearch continuation did not visibly complete the result pages");
  }

  factsQueryOperation.value = "graph";
  factsQueryGraphRootFamily.value = "assertion";
  factsQueryGraphRootId.value = slotAssertion.recordId;
  for (const option of factsQueryGraphRelationships.options) {
    option.selected = option.value === "event_causes";
  }
  factsQueryGraphDirection.value = "both";
  factsQueryGraphMaxDepth.value = "0";
  factsQueryGraphMaxNodes.value = "20";
  factsQueryGraphMaxEdges.value = "20";
  updateFactControls();
  const graphQuery = await manageFacts(factsQueryCommand(), activeSessionId);
  if (graphQuery.kind !== "query" || graphQuery.result.kind !== "graph"
    || !graphQuery.result.nodes.some((node) => node.family === "assertion" && node.record_id === slotAssertion.recordId)) {
    throw new Error("Graph traversal did not return its visible zero-depth root");
  }
  const graphText = await clickFactPreview();
  if (!graphText.includes("Vollständiger Graphdurchlauf") || !graphText.includes(slotAssertion.recordId)) {
    throw new Error("Graph result and traversal limits were not rendered in the desktop panel");
  }

  factsQueryOperation.value = "count";
  updateFactControls();
  const countQuery = await manageFacts(factsQueryCommand(), activeSessionId);
  if (countQuery.kind !== "query" || countQuery.result.kind !== "aggregate"
    || countQuery.result.result.kind !== "count" || BigInt(countQuery.result.result.value) < 3n) {
    throw new Error("COUNT did not report complete visible resolved contributors");
  }
  if (!(await clickFactPreview()).includes("COUNT ·")) throw new Error("COUNT was not rendered in the query panel");

  factsQueryOperation.value = "exists";
  updateFactControls();
  const existsQuery = await manageFacts(factsQueryCommand(), activeSessionId);
  if (existsQuery.kind !== "query" || existsQuery.result.kind !== "aggregate"
    || existsQuery.result.result.kind !== "exists" || existsQuery.result.result.value !== true) {
    throw new Error("EXISTS did not report the visible resolved result");
  }
  if (!(await clickFactPreview()).includes("EXISTS · Ja")) throw new Error("EXISTS was not rendered in the query panel");

  factsQueryOperation.value = "grouped_count";
  updateFactControls();
  const groupedQuery = await manageFacts(factsQueryCommand(), activeSessionId);
  if (groupedQuery.kind !== "query" || groupedQuery.result.kind !== "aggregate"
    || groupedQuery.result.result.kind !== "grouped_count"
    || !groupedQuery.result.result.groups.some((group) => group.polarity === "positive")) {
    throw new Error("GroupedCount did not return complete visible polarity groups");
  }
  if (!(await clickFactPreview()).includes("COUNT nach Polarity")) {
    throw new Error("GroupedCount was not rendered in the query panel");
  }

  factsQuerySchemaMode.value = "historical";
  factsQueryTimeMode.value = "all_times";
  factsQueryOperation.value = "resolved";
  factsQueryRecordedAsOf.dataset.auto = "true";
  factsQuerySchemaRevision.dataset.auto = "true";
  updateFactControls();
  await recordFactsSmokeStage("query-modes:complete");

  factsCorrectionTarget.value = assertion.recordId;
  factsCorrectionReason.value = "IPC smoke: explicit assertion correction";
  updateFactControls();
  const correctionStatus = await commitPreviewedFactAction(
    factsCorrectionPreviewButton,
    factsCorrectionCommitButton,
    `Assertion-Korrektur gemeinsam gespeichert · Revision `,
  );
  if (!correctionStatus.includes(`Original ${assertion.recordId} zurückgenommen`)
    || !correctionStatus.includes("Corrects")) {
    throw new Error("Assertion correction UI did not report its explicit Retraction and Corrects edge");
  }
  const corrected = factCatalog.records.find((item) => item.family === "assertion"
    && item.record_id === assertion.recordId);
  if (corrected?.retracted !== true) {
    throw new Error("Assertion correction did not expose the original's explicit lifecycle state");
  }

  const committedStatus = factsWriteStatus.textContent;
  const savedQueryTimeline = factsQueryTimeline.value;
  factsQueryTimeMode.value = "at";
  factsQueryTimeline.value = "";
  updateFactControls();
  factsPreviewButton.disabled = false;
  await runFactsPreview(activeSessionId);
  if (factsWriteStatus.textContent !== committedStatus
    || !factsPreviewStatus.textContent
    || factsPreviewStatus.textContent.startsWith("Abfrage abgeschlossen · Revision ")) {
    throw new Error("a failed separate preview changed the successful write receipt");
  }
  factsQueryTimeMode.value = "all_times";
  factsQueryTimeline.value = savedQueryTimeline;
  updateFactControls();

  await refreshFactsCatalog(activeSessionId);
  factsHistorySpace.value = root.history_space_id;
  factsLayer.value = baseLayer.layer_id;
  factsEventKind.value = eventKind.identity;
  const setSmokeEventDraft = (eventTime, summary) => {
    factsEventDraft.value = JSON.stringify({
      history_space_id: root.history_space_id,
      layer_id: baseLayer.layer_id,
      event_kind_id: eventKind.identity,
      participants: [{ role_id: eventRole.identity, entity_id: createdEntity.entity_id }],
      attributes: [{ attribute_id: eventAttribute.identity, value: { kind: "string", data: summary } }],
      event_time: eventTime,
    }, null, 2);
    factsEventDraft.dataset.templateKind = eventKind.identity;
    updateFactControls();
  };
  setSmokeEventDraft({ kind: "instant", timeline_id: timeline.identity, nanoseconds: "10" }, "erster Smoke-Event");
  const eventA = await clickFactWrite(factsEventCreate, "Event");
  setSmokeEventDraft({ kind: "instant", timeline_id: timeline.identity, nanoseconds: "20" }, "zweiter Smoke-Event");
  const eventB = await clickFactWrite(factsEventCreate, "Event");
  setSmokeEventDraft({ kind: "span", timeline_id: timeline.identity, start_nanoseconds: "30", end_nanoseconds: null }, "offener Smoke-Span");
  const openSpan = await clickFactWrite(factsEventCreate, "Event");

  factsEventSpanCloseTarget.value = openSpan.recordId;
  factsEventSpanCloseTimeline.value = timeline.identity;
  factsEventSpanCloseNanoseconds.value = "50";
  updateFactControls();
  const spanClosure = await clickFactWrite(factsEventSpanClose, "Event-Spanabschluss");
  if (!factCatalog.records.some((item) => item.family === "event_span_closure" && item.record_id === spanClosure.recordId
    && item.time_end_nanoseconds === "50")) {
    throw new Error("the UI did not expose the explicit open-span closure record");
  }

  factsEventRelationFrom.value = eventA.recordId;
  factsEventRelationTo.value = eventB.recordId;
  factsEventRelationKind.value = "before";
  updateFactControls();
  const beforeRelation = await clickFactWrite(factsEventRelationCreate, "Eventrelation");
  const waitForEventGraphConflict = async (expectedText) => {
    const deadline = Date.now() + 30000;
    while (factBusy && Date.now() < deadline) await new Promise((resolve) => window.setTimeout(resolve, 50));
    const status = factsWriteStatus.textContent;
    if (!status.includes(expectedText) || !status.includes("Es wurde nichts gespeichert.")) {
      throw new Error(`the Event graph conflict was not explained safely: ${status}`);
    }
  };
  const revisionBeforeConflict = factCatalog.revision;
  factsEventRelationFrom.value = eventB.recordId;
  factsEventRelationTo.value = eventA.recordId;
  factsEventRelationKind.value = "after";
  updateFactControls();
  if (factsEventRelationCreate.disabled) throw new Error("the inverse After conflict fixture was disabled");
  factsEventRelationCreate.click();
  await waitForEventGraphConflict("bereits aktiv");
  if (factCatalog.revision !== revisionBeforeConflict) throw new Error("a rejected inverse After relation changed the data revision");

  factsEventRelationFrom.value = eventA.recordId;
  factsEventRelationTo.value = eventB.recordId;
  factsEventRelationKind.value = "causes";
  updateFactControls();
  await clickFactWrite(factsEventRelationCreate, "Eventrelation");
  const revisionBeforeCauseCycle = factCatalog.revision;
  factsEventRelationFrom.value = eventB.recordId;
  factsEventRelationTo.value = eventA.recordId;
  factsEventRelationKind.value = "causes";
  updateFactControls();
  factsEventRelationCreate.click();
  await waitForEventGraphConflict("Zyklus im Kausalgraphen");
  if (factCatalog.revision !== revisionBeforeCauseCycle) throw new Error("a rejected Causes cycle changed the data revision");

  factsEventRelationFrom.value = eventA.recordId;
  factsEventRelationTo.value = eventB.recordId;
  factsEventRelationKind.value = "same_time";
  updateFactControls();
  factsEventRelationCreate.click();
  await waitForEventGraphConflict("SameTime-Gruppe");
  if (factCatalog.revision !== revisionBeforeCauseCycle) throw new Error("a rejected SameTime conflict changed the data revision");
  if (!factCatalog.eventGraphGuidance.some((line) => line.includes("niemals automatisch Before, SameTime oder Causes"))) {
    throw new Error("the Event UI omitted its explicit no-inference guidance");
  }
  if (!factCatalog.records.some((item) => item.family === "event_relation" && item.record_id === beforeRelation.recordId
    && item.from_event_id === eventA.recordId && item.to_event_id === eventB.recordId && item.relation_kind === "before")) {
    throw new Error("the Event relation catalog did not expose the canonical Before relation");
  }

  const eventMaskBranch = await createFactsMaskBranch(
    activeSessionId,
    root.history_space_id,
    factCatalog.revision,
    "event-mask-branch",
  );
  factsHistorySpace.value = eventMaskBranch.history_space_id;
  factsLayer.value = baseLayer.layer_id;
  factsEventMaskTarget.value = eventA.recordId;
  updateFactControls();
  const eventMask = await clickFactWrite(factsEventMaskCreate, "EventMask");
  const visibleMaskedEvent = factCatalog.records.find((item) => item.family === "event" && item.record_id === eventA.recordId);
  if (visibleMaskedEvent?.retracted !== false) throw new Error("EventMask incorrectly changed the target Event's retraction state");
  factsLifecycleTarget.value = `event_mask:${eventMask.recordId}`;
  factsLifecycleAction.value = "retract";
  factsLifecycleReason.value = "IPC smoke: EventMask separately retracted";
  updateFactControls();
  const eventMaskStatus = await commitPreviewedFactAction(
    factsLifecyclePreviewButton,
    factsLifecycleCommitButton,
    `event_mask ${eventMask.recordId}: retracted · Revision `,
  );
  if (!eventMaskStatus.includes("Lebenszyklusbeleg")) throw new Error("EventMask retraction did not return its lifecycle receipt");
  const retractedEventMask = factCatalog.records.find((item) => item.family === "event_mask" && item.record_id === eventMask.recordId);
  if (retractedEventMask?.retracted !== true) throw new Error("the UI did not expose the EventMask's explicit retraction");
  const stillActiveEvent = factCatalog.records.find((item) => item.family === "event" && item.record_id === eventA.recordId);
  if (stillActiveEvent?.retracted !== false) throw new Error("retracting an EventMask also retracted its target Event");

  factsLifecycleTarget.value = `event:${eventB.recordId}`;
  factsLifecycleAction.value = "retract";
  factsLifecycleReason.value = "IPC smoke: explicit Event retraction";
  updateFactControls();
  await commitPreviewedFactAction(
    factsLifecyclePreviewButton,
    factsLifecycleCommitButton,
    `event ${eventB.recordId}: retracted · Revision `,
  );
  if (factCatalog.records.find((item) => item.family === "event" && item.record_id === eventB.recordId)?.retracted !== true) {
    throw new Error("the explicit Event retraction did not appear in the record catalog");
  }

  factsSourceKind.value = "book";
  factsSourceLocator.value = "https://example.invalid/source";
  factsSourceDigest.value = "1234abcd";
  factsSourceMetadataKey.value = "edition";
  factsSourceMetadataValue.value = "first";
  updateFactControls();
  const source = await clickFactWrite(factsSourceCreate, "Source");
  if (!factCatalog.sources.some((item) => item.source_id === source.recordId
    && item.source_kind === "book" && item.content_digest_hex === "1234abcd")) {
    throw new Error("the Source fields were not persisted and returned in the authorized catalog");
  }

  factsEvidenceSource.value = source.recordId;
  factsEvidenceTarget.value = `assertion:${assertion.recordId}`;
  factsEvidenceRelation.value = "supports";
  updateFactControls();
  const evidence = await clickFactWrite(factsEvidenceCreate, "Evidence");
  if (!factCatalog.evidence.some((item) => item.evidence_id === evidence.recordId
    && item.source_id === source.recordId && item.target_record_id === assertion.recordId)) {
    throw new Error("Evidence endpoints were not preserved in the authorized catalog");
  }

  factsProvenanceFrom.value = `source:${source.recordId}`;
  factsProvenanceTo.value = `assertion:${assertion.recordId}`;
  factsProvenanceRelation.value = "derived_from";
  updateFactControls();
  const provenance = await clickFactWrite(factsProvenanceCreate, "Provenance");
  if (!factCatalog.provenance.some((item) => item.provenance_id === provenance.recordId
    && item.from_record_id === source.recordId && item.to_record_id === assertion.recordId)) {
    throw new Error("the Provenance edge was not returned with its closed endpoints");
  }

  factsSourceSupersedeTarget.value = source.recordId;
  factsSourceKind.value = "book_revision";
  factsSourceLocator.value = "https://example.invalid/source/revised";
  factsSourceDigest.value = "5678abcd";
  factsSourceMetadataValue.value = "second";
  updateFactControls();
  const replacementSource = await clickFactWrite(factsSourceSupersede, "Source mit Lineage");
  if (!factCatalog.provenance.some((item) => item.from_family === "source"
    && item.from_record_id === source.recordId && item.to_family === "source"
    && item.to_record_id === replacementSource.recordId && item.relation === "derived_from")) {
    throw new Error("Source replacement did not store its explicit DerivedFrom lineage");
  }

  factsEvidenceRetractTarget.value = evidence.recordId;
  factsEvidenceRetractReason.value = "IPC smoke: source changed";
  updateFactControls();
  await clickFactWrite(factsEvidenceRetract, "Evidence-Rücknahme");
  if (factCatalog.evidence.find((item) => item.evidence_id === evidence.recordId)?.retracted !== true) {
    throw new Error("the Evidence retraction was not shown as a separate lifecycle record");
  }

  factsProvenanceRetractTarget.value = provenance.recordId;
  factsProvenanceRetractReason.value = "IPC smoke: explicit Provenance retraction";
  updateFactControls();
  await clickFactWrite(factsProvenanceRetract, "Provenance-Rücknahme");
  if (factCatalog.provenance.find((item) => item.provenance_id === provenance.recordId)?.retracted !== true) {
    throw new Error("the Provenance retraction was not shown in the catalog");
  }
  await recordFactsSmokeStage("meta-history:complete");
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
  return ({ entity_type: "EntityType", predicate: "Prädikat", event_kind: "Ereignistyp", timeline: "Timeline", time_unit: "Zeiteinheit", layer: "Layer", layer_snapshot: "Layer-Stand" })[value] ?? value;
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
    const lifecycleManagedFamily = ["entity_type", "predicate", "event_kind", "timeline", "time_unit"].includes(definition.family);
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
  if (definition.family === "timeline") {
    const profile = details.calendar_profile === "proleptic_gregorian_utc"
      ? "Proleptischer gregorianischer Kalender (UTC)"
      : "Kein ziviler Kalender";
    const parts = [`Kalenderprofil: ${profile}`];
    if (details.epoch_unix_nanoseconds != null) {
      parts.push(`Epoch der Timeline-Null: ${details.epoch_unix_nanoseconds} ns seit Unix-Epoch`);
    }
    return parts;
  }
  if (definition.family === "time_unit") {
    return [`Skala: ${details.nanoseconds_per_tick} Nanosekunden pro Tick`];
  }
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

function renderedDefinitionText(definition) {
  const heading = `${familyLabel(definition.family)} · ${definition.symbol}`;
  return [...schemaDefinitions.querySelectorAll("article.definition")]
    .find((card) => card.querySelector("h3")?.textContent === heading)?.textContent ?? "";
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
  schemaTimelineFields.hidden = family !== "timeline";
  schemaTimeUnitFields.hidden = family !== "time_unit";
  timelineEpochWrap.hidden = timelineCalendarProfile.value !== "proleptic_gregorian_utc";
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

function validateI128Decimal(value) {
  const text = value.trim();
  if (!/^-?(0|[1-9]\d*)$/.test(text)) throw new Error("invalid_request");
  const parsed = BigInt(text);
  if (parsed < -(1n << 127n) || parsed > (1n << 127n) - 1n) throw new Error("invalid_request");
  return text;
}

function validatePositiveU64Decimal(value) {
  const text = value.trim();
  if (!/^[1-9]\d*$/.test(text)) throw new Error("invalid_request");
  const parsed = BigInt(text);
  if (parsed > (1n << 64n) - 1n) throw new Error("invalid_request");
  return text;
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
  if (schemaFamily.value === "timeline") {
    const calendarProfile = timelineCalendarProfile.value;
    const profile = calendarProfile === "none"
      ? { profile: "none" }
      : {
        profile: "proleptic_gregorian_utc",
        epoch_unix_nanoseconds: validateI128Decimal(timelineEpochUnixNanoseconds.value),
      };
    return { family: "timeline", symbol, calendar_profile: profile };
  }
  if (schemaFamily.value === "time_unit") {
    return {
      family: "time_unit",
      symbol,
      nanoseconds_per_tick: validatePositiveU64Decimal(timeUnitNanosecondsPerTick.value),
    };
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
  timelineCalendarProfile.value = "none";
  timelineEpochUnixNanoseconds.value = "";
  timeUnitNanosecondsPerTick.value = "";
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
        await listen("project-state-changed", async () => {
          if (factBusy) return;
          try {
            await refreshProject(sessionId);
          } catch {
            return;
          }
          if (!projectOpen) return;
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
          if (!factBusy) refreshFactsCatalog(sessionId).catch((error) => {
            factsContextNote.textContent = showError(error);
          });
          if (!transferBusy) refreshTransferCatalog(sessionId).catch((error) => {
            transferStatus.textContent = showError(error);
          });
        });
        await listen("migration-state-changed", async () => {
          try {
            await refreshMigrationState(sessionId);
          } catch (error) {
            migrationStatus.textContent = migrationErrorText(error);
          }
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
      await refreshMigrationState(sessionId);

      if (projectMode.enabled && projectMode.startup_smoke_enabled) {
        operationStatus.textContent = "Zwei-Fenster-Projektprüfung läuft …";
        await runProjectSmoke(sessionId);
        if (role === "primary") {
          operationStatus.textContent = "Schema-, Entitäts-, Perspektiven-, Rechte-, Branch- und Layerprüfung läuft …";
          await runSchemaSmoke(sessionId);
          await runEntitySmoke(sessionId);
          await runBranchLayerSmoke(sessionId);
          await runPerspectiveSmoke(sessionId);
          await runSecurityPolicySmoke(sessionId);
          factsSmokeActive = true;
          try {
            await runFactsSmoke(sessionId);
          } catch (error) {
            await invoke("facts_smoke_diagnostic", { details: String(error?.message ?? error) }).catch(() => {});
            throw error;
          } finally {
            factsSmokeActive = false;
          }
        }
        operationStatus.textContent = "Projektprüfung abgeschlossen.";
        await refreshProject(sessionId);
        if (role === "primary") {
          const closeResponse = await invoke("close_project", {
            sessionId,
            request: { protocol_version: 1 },
          });
          if (!closeResponse.closed || closeResponse.project?.project_open
            || !closeResponse.shutdown?.drained
            || closeResponse.shutdown.unfinished_job_ids.length !== 0
            || closeResponse.shutdown.unfinished_workers !== 0) {
            throw new Error("Der geordnete Job-Shutdown hat den Projekt-Drain nicht vollständig bestätigt.");
          }
          renderProject(closeResponse.project);
          jobsShutdownStatus.textContent = "Shutdown: vollständig; keine offenen Jobs oder Worker.";
          await recordFactsSmokeStage("project-complete");
          await runRecoverySmoke(sessionId);
          await recordFactsSmokeStage("recovery-smoke:complete");
        }
      }

      window.setInterval(() => {
        if (!factBusy && !projectBusy) refreshProject(sessionId).catch(() => {});
        if (!jobsBusy && projectOpen) refreshJobs(sessionId).catch(() => {});
      }, 1200);
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
      await createProjectTracked(sessionId, projectName.value);
      operationStatus.textContent = "Projekt wurde angelegt.";
      await refreshProject(sessionId);
    } catch (error) {
      operationStatus.textContent = showError(error);
    } finally {
      setBusy(false);
      await refreshProject(sessionId).catch(() => {});
    }
  });

  reconcileOperationsButton.addEventListener("click", () => {
    void reconcilePendingOperations();
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

  recoveryInspectButton.addEventListener("click", async () => {
    if (!sessionId || projectOpen || recoveryBusy) return;
    recoveryBusy = true;
    recoveryStatus.textContent = "Projektordner wird read-only geprüft …";
    currentRecoveryReport = null;
    updateRecoveryControls();
    try {
      const response = await invoke("inspect_recovery", {
        sessionId,
        request: { protocol_version: 1 },
      });
      renderRecoveryReport(response.result);
    } catch (error) {
      recoveryReportElement.hidden = true;
      recoveryActions.hidden = true;
      recoveryStatus.textContent = showError(error);
    } finally {
      recoveryBusy = false;
      updateRecoveryControls();
    }
  });

  recoveryOpenCleanButton.addEventListener("click", () => openButton.click());

  recoveryKeepReadOnlyButton.addEventListener("click", () => {
    if (!currentRecoveryReport) return;
    recoveryStatus.textContent = `Die Quelle bleibt unverändert und read-only; sichere Revision ${currentRecoveryReport.safe_revision}.`;
  });

  recoveryRunButton.addEventListener("click", async () => {
    if (!sessionId || !currentRecoveryReport?.can_run_journaled_recovery || recoveryBusy) return;
    const confirmed = window.confirm(
      "Journalisierte Recovery darf WAL-Endstücke quarantänisieren und ein verifiziertes Manifest veröffentlichen. Das ist eine ausdrückliche Änderung am Projekt. Fortfahren?",
    );
    if (!confirmed) return;
    recoveryBusy = true;
    recoveryStatus.textContent = "Journalisierte Recovery läuft …";
    updateRecoveryControls();
    try {
      const response = await invoke("run_journaled_recovery", {
        sessionId,
        request: { protocol_version: 1, confirmed: true },
      });
      renderRecoveryReport(response.result.report);
      recoveryStatus.textContent = `Journalisierte Recovery abgeschlossen · ${response.result.quarantined_tails} WAL-Endstücke quarantänisiert · ${response.result.replayed_snapshots} Snapshots wiederholt · Zustand: ${recoveryDispositionText[response.result.report.disposition] ?? response.result.report.disposition}.`;
    } catch (error) {
      recoveryStatus.textContent = showError(error);
    } finally {
      recoveryBusy = false;
      updateRecoveryControls();
    }
  });

  recoverySalvageButton.addEventListener("click", async () => {
    if (!sessionId || !currentRecoveryReport?.can_salvage || recoveryBusy) return;
    const archiveName = recoveryArchiveName.value.trim();
    if (!archiveName) {
      recoveryStatus.textContent = "Gib einen Namen für das neue Salvage-Archiv ein.";
      recoveryArchiveName.focus();
      return;
    }
    recoveryBusy = true;
    recoveryStatus.textContent = "Wähle im nativen Dialog einen vorhandenen Elternordner für das neue Archiv …";
    updateRecoveryControls();
    try {
      const response = await invoke("salvage_recovery", {
        sessionId,
        request: { protocol_version: 1, archive_name: archiveName },
      });
      const result = response.result;
      recoveryStatus.textContent = `Salvage-Archiv ${result.archive_name} erstellt · neue Datenbank-ID ${result.new_database_id} · sichere Revision ${result.safe_revision} · ${result.copied_segments} Segmente kopiert · ${result.omitted_segments} ausgelassen · ${result.uncertainty_count} Unsicherheiten im markierten Archivbericht.`;
    } catch (error) {
      recoveryStatus.textContent = showError(error);
    } finally {
      recoveryBusy = false;
      updateRecoveryControls();
    }
  });

  migrationSelectPlanButton.addEventListener("click", async () => {
    if (!sessionId || projectOpen || migrationBusy || currentMigrationState?.run_attempted) return;
    migrationBusy = true;
    migrationStatus.textContent = "Wähle im nativen Dialog zuerst das geschlossene Quellprojekt und danach den kanonischen Migrationsplan …";
    updateMigrationControls();
    try {
      const response = await invoke("select_migration_plan", {
        sessionId,
        request: { protocol_version: 1 },
      });
      if (response.protocol_version !== 1) throw new Error("unsupported_protocol");
      renderMigrationState({
        plan: response.plan,
        dry_run: null,
        run_attempted: false,
        attempted_run_id: null,
        omitted_record_indexes: [],
        can_execute: false,
        can_resume: false,
      });
      migrationStatus.textContent = "Plan geprüft. Wähle jetzt für jeden Schritt die zugehörigen Quelldatensatzdateien und führe den Dry Run aus.";
    } catch (error) {
      migrationStatus.textContent = migrationErrorText(error);
      await refreshMigrationState(sessionId).catch(() => {});
    } finally {
      migrationBusy = false;
      updateMigrationControls();
    }
  });

  migrationPreviewButton.addEventListener("click", async () => {
    if (!sessionId || projectOpen || migrationBusy || !currentMigrationState?.plan) return;
    migrationBusy = true;
    migrationStatus.textContent = "Wähle für jeden Migrationsschritt die Quelldatensätze aus; Abbrechen im Dateidialog bedeutet, dass der Schritt keine Eingabedatensätze hat …";
    updateMigrationControls();
    try {
      const response = await invoke("preview_migration", {
        sessionId,
        request: { protocol_version: 1 },
      });
      if (response.protocol_version !== 1) throw new Error("unsupported_protocol");
      await refreshMigrationState(sessionId);
      migrationStatus.textContent = response.result.preflight_complete
        ? "Dry Run abgeschlossen. Die angezeigte Vorschau hat keine Daten geschrieben."
        : "Dry Run abgeschlossen. Befunde oder Budgetgrenzen sperren die Ausführung.";
    } catch (error) {
      migrationStatus.textContent = migrationErrorText(error);
      await refreshMigrationState(sessionId).catch(() => {});
    } finally {
      migrationBusy = false;
      updateMigrationControls();
    }
  });

  migrationRunButton.addEventListener("click", async () => {
    const state = currentMigrationState;
    if (!sessionId || projectOpen || migrationBusy || !state?.can_execute) return;
    const breaking = state.plan.category === "Breaking";
    if (breaking && !window.confirm(
      `Breaking-Migration ${state.plan.migration_id} ausdrücklich starten?\n\nWorldDB erstellt zuerst eine exakte Sicherung und einen unabhängig geprüften Restore-Klon in den anschließend ausgewählten Ordnern. Danach beginnt die irreversible Contract-Phase.`,
    )) return;
    migrationBusy = true;
    migrationStatus.textContent = breaking
      ? "Wähle nacheinander die Elternordner für exakte Sicherung und geprüften Restore-Klon …"
      : "Migration wird nach erneuter Prüfung des Quellstands ausgeführt …";
    updateMigrationControls();
    try {
      const response = await invoke("run_migration", {
        sessionId,
        request: { protocol_version: 1, confirmed_breaking: breaking },
      });
      if (response.protocol_version !== 1 || response.result.status !== "completed") {
        throw new Error("unsupported_protocol");
      }
      renderMigrationState(null);
      migrationStatus.textContent = `Migration veröffentlicht · ${response.result.completed_step_count} Schritte · neue Revision ${response.result.final_revision} · ${migrationRestorepointText(response.result.restorepoint_status)}.`;
    } catch (error) {
      migrationStatus.textContent = migrationErrorText(error);
      await refreshMigrationState(sessionId).catch(() => {});
    } finally {
      migrationBusy = false;
      updateMigrationControls();
    }
  });

  migrationResumeButton.addEventListener("click", async () => {
    const state = currentMigrationState;
    if (!sessionId || projectOpen || migrationBusy || !state?.can_resume) return;
    const breaking = state.plan.category === "Breaking";
    if (breaking && !window.confirm(
      `Den bereits begonnenen Breaking-Lauf ${state.attempted_run_id ?? "(Lauf-ID unbekannt)"} fortsetzen?\n\nWorldDB prüft Journal und Sicherung erneut und verlangt für Breaking einen neuen Restore-Klon.`,
    )) return;
    migrationBusy = true;
    migrationStatus.textContent = breaking
      ? "Wähle den Elternordner für den neuen Restore-Klon …"
      : "Der bestehende Migrationslauf wird anhand seines Journals fortgesetzt …";
    updateMigrationControls();
    try {
      const response = await invoke("resume_migration", {
        sessionId,
        request: { protocol_version: 1, confirmed_breaking: breaking },
      });
      if (response.protocol_version !== 1 || response.result.status !== "resumed") {
        throw new Error("unsupported_protocol");
      }
      renderMigrationState(null);
      migrationStatus.textContent = `Migration fortgesetzt und abgeschlossen · ${response.result.completed_step_count} Schritte · neue Revision ${response.result.final_revision} · ${migrationRestorepointText(response.result.restorepoint_status)}.`;
    } catch (error) {
      migrationStatus.textContent = migrationErrorText(error);
      await refreshMigrationState(sessionId).catch(() => {});
    } finally {
      migrationBusy = false;
      updateMigrationControls();
    }
  });

  migrationCancelButton.addEventListener("click", async () => {
    if (!sessionId || migrationBusy || currentMigrationState?.run_attempted) return;
    if (!window.confirm("Den ausgewählten Migrationsplan und seine Dry-Run-Eingaben verwerfen? Es wurde noch nichts migriert.")) return;
    migrationBusy = true;
    updateMigrationControls();
    try {
      await invoke("cancel_migration", {
        sessionId,
        request: { protocol_version: 1 },
      });
      renderMigrationState(null);
      migrationStatus.textContent = "Migrationsplan und Vorschau wurden verworfen; das Projekt blieb unverändert.";
    } catch (error) {
      migrationStatus.textContent = migrationErrorText(error);
      await refreshMigrationState(sessionId).catch(() => {});
    } finally {
      migrationBusy = false;
      updateMigrationControls();
    }
  });

  recoveryRestoreButton.addEventListener("click", () => {
    backupPanel.scrollIntoView({ behavior: "smooth", block: "center" });
    backupProfile.focus();
    backupStatus.textContent = "Wähle dasselbe Profil und den Auditumfang wie beim Backup. Restore braucht ein sauberes Autorisierungsprojekt mit derselben Datenbank-ID und erstellt einen getrennten neuen Klon.";
  });

  backupProfile.addEventListener("change", updateBackupProfileDetails);
  backupCreateButton.addEventListener("click", () => { void runBackupAction("create"); });
  backupVerifyButton.addEventListener("click", () => { void runBackupAction("verify"); });
  backupRestoreButton.addEventListener("click", () => { void runBackupAction("restore"); });
  initializeExportClassChoices();
  exportKind.addEventListener("change", updateExportImportControls);
  exportFromRevision.addEventListener("input", updateExportImportControls);
  exportThroughRevision.addEventListener("input", updateExportImportControls);
  exportHistorySpaces.addEventListener("input", updateExportImportControls);
  exportRecordClasses.addEventListener("change", updateExportImportControls);
  importRemappings.addEventListener("input", updateExportImportControls);
  exportRunButton.addEventListener("click", () => { void runExportAction(); });
  importPlanButton.addEventListener("click", () => { void runImportPlanAction(); });
  importPrepareButton.addEventListener("click", () => { void runImportPrepareAction(); });
  updateExportImportControls();
  for (const input of [purgeTargets, purgeKnownCopies]) input.addEventListener("input", updatePurgeControls);
  for (const input of [purgeMode, purgeExternalComplete]) input.addEventListener("change", updatePurgeControls);
  purgePreviewButton.addEventListener("click", () => { void runPurgePreview(); });
  purgeExecuteButton.addEventListener("click", () => { void runPurgeExecution(); });
  purgeDiscardButton.addEventListener("click", () => { void discardPurgePlan(); });
  diagnosticExportButton.addEventListener("click", () => { void exportDiagnostics(); });
  updatePurgeControls();
  updateDiagnosticControls();

  closeButton.addEventListener("click", async () => {
    if (!sessionId) return;
    setBusy(true);
    operationStatus.textContent = "Projekt wird geschlossen …";
    try {
      const result = await invoke("close_project", { sessionId, request: { protocol_version: 1 } });
      const shutdown = result.shutdown;
      const summary = shutdown
        ? ` Job-Drain: ${shutdown.drained ? "vollständig" : "unvollständig"} · ${shutdown.unfinished_job_ids.length} offene Jobs · ${shutdown.unfinished_workers} offene Worker · ${shutdown.worker_panics} Worker-Fehler.`
        : "";
      if (result.closed) {
        operationStatus.textContent = `Projekt wurde geordnet geschlossen.${summary}`;
        jobsShutdownStatus.textContent = `Shutdown: ${shutdown?.drained ? "vollständig" : "kein aktiver Job-Drain erforderlich"}.`;
      } else {
        operationStatus.textContent = `Projekt bleibt geöffnet, bis alle Jobs einen sicheren Endzustand erreicht haben.${summary}`;
        jobsShutdownStatus.textContent = operationStatus.textContent;
      }
    } catch (error) {
      operationStatus.textContent = showError(error);
    } finally {
      setBusy(false);
      await refreshProject(sessionId).catch(() => {});
    }
  });

  jobsRefreshButton.addEventListener("click", () => refreshJobs().catch(() => {}));
  jobsCloseProjectButton.addEventListener("click", () => closeButton.click());

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
  factsEpistemicMode.addEventListener("change", () => {
    if (factsEpistemicMode.value === "world_state") factsPerspective.value = "";
    clearFactActionPreviews();
  });
  for (const select of [factsHistorySpace, factsLayer, factsPerspective, factsSubject, factsPolarity,
    factsValueBool, factsValueEntity, factsTimeValueTimeline, factsTimeValueUnit, factsValidityTimeline,
    factsMaskSelector, factsQueryOperation, factsQuerySchemaMode, factsQueryTimeMode, factsQueryTimeline, factsCorrectionTarget,
    factsLifecycleTarget, factsLifecycleAction, factsEventKind, factsEventMaskTarget,
    factsEventRelationFrom, factsEventRelationTo, factsEventRelationKind,
    factsEventSpanCloseTarget, factsEventSpanCloseTimeline]) {
    select.addEventListener("change", clearFactActionPreviews);
  }
  factsEventKind.addEventListener("change", () => {
    factsEventDraft.dataset.templateKind = "";
    updateEventDraftTemplate();
    clearFactActionPreviews();
  });
  factsPredicate.addEventListener("change", () => { clearFactActionPreviews(); updateFactsValueFields(); });
  factsValidityEnabled.addEventListener("change", clearFactActionPreviews);
  factsEventCorrectionTarget.addEventListener("change", () => {
    factsEventCorrectionDraft.dataset.templateTarget = "";
    updateEventCorrectionTemplate();
    clearFactActionPreviews();
  });
  factsCorrectionTarget.addEventListener("change", () => {
    const target = selectedFactRecord(factsCorrectionTarget);
    if (!target?.history_space_id || !target.layer_id || !target.epistemic_mode
      || !target.subject_id || !target.predicate_id) {
      updateFactControls();
      return;
    }
    factsHistorySpace.value = target.history_space_id;
    factsLayer.value = target.layer_id;
    factsPerspective.value = target.perspective_id ?? "";
    factsEpistemicMode.value = target.epistemic_mode;
    factsSubject.value = target.subject_id;
    factsPredicate.value = target.predicate_id;
    updateFactsValueFields();
    updateFactControls();
  });
  for (const input of [factsValueText, factsTimeValueTicks, factsValidityStart, factsValidityEnd,
    factsMaskAssertionId, factsQueryNanoseconds, factsCorrectionReason, factsEventCorrectionDraft,
  factsEventDraft, factsLifecycleReason, factsEventSpanCloseNanoseconds]) {
    input.addEventListener("input", clearFactActionPreviews);
  }
  factsQueryRecordedAsOf.addEventListener("input", () => {
    factsQueryRecordedAsOf.dataset.auto = "false";
    invalidateFactsSearch();
    updateFactControls();
  });
  factsQuerySchemaRevision.addEventListener("input", () => {
    factsQuerySchemaRevision.dataset.auto = "false";
    invalidateFactsSearch();
    updateFactControls();
  });
  for (const control of [factsHistorySpace, factsLayer, factsPerspective, factsEpistemicMode,
    factsSubject, factsPredicate, factsQueryOperation, factsQuerySchemaMode, factsQueryTimeMode,
    factsQueryTimeline, factsQueryNanoseconds, factsQuerySchemaRevision,
    factsQuerySearchTerms, factsQuerySearchMatch, factsQueryPageSize,
    factsQueryGraphRootFamily, factsQueryGraphRootId, factsQueryGraphRelationships,
    factsQueryGraphDirection, factsQueryGraphMaxDepth, factsQueryGraphMaxNodes,
    factsQueryGraphMaxEdges, factsQueryGraphCyclePolicy, factsQueryMaxCandidates,
    factsQueryMaxWorkUnits, factsQueryMaxResults]) {
    control.addEventListener("change", () => {
      invalidateFactsSearch();
      updateFactControls();
    });
    control.addEventListener("input", () => {
      invalidateFactsSearch();
      updateFactControls();
    });
  }
  for (const input of [factsSourceKind, factsSourceLocator, factsSourceDigest,
    factsSourceMetadataKey, factsSourceMetadataValue, factsEvidenceRetractReason,
    factsProvenanceRetractReason]) {
    input.addEventListener("input", updateFactControls);
  }
  for (const select of [factsSourceSupersedeTarget, factsEvidenceSource, factsEvidenceTarget,
    factsEvidenceRelation, factsEvidenceRetractTarget, factsProvenanceFrom, factsProvenanceTo,
    factsProvenanceRelation, factsProvenanceRetractTarget]) {
    select.addEventListener("change", updateFactControls);
  }
  factsCreateAssertion.addEventListener("click", () => submitFact(() => ({
    command: "create_assertion",
    expected_base_revision: factCatalog.revision,
    context: factsContextInput(),
    subject_id: factsSubject.value,
    predicate_id: factsPredicate.value,
    value: factsValueInput(),
    polarity: factsPolarity.value,
    validity: factsValidityInput(),
  }), "Assertion"));
  factsSourceCreate.addEventListener("click", () => submitFact(() => ({
    command: "create_source",
    expected_base_revision: factCatalog.revision,
    ...factsSourceFieldsInput(),
  }), "Source"));
  factsSourceSupersede.addEventListener("click", () => submitFact(() => ({
    command: "supersede_source",
    expected_base_revision: factCatalog.revision,
    superseded_source_id: factsSourceSupersedeTarget.value,
    ...factsSourceFieldsInput(),
  }), "Source mit Lineage"));
  factsEvidenceCreate.addEventListener("click", () => submitFact(() => ({
    command: "create_evidence",
    expected_base_revision: factCatalog.revision,
    source_id: factsEvidenceSource.value,
    target: factsEndpointInput(factsEvidenceTarget),
    relation: factsEvidenceRelation.value,
  }), "Evidence"));
  factsEvidenceRetract.addEventListener("click", () => submitFact(() => ({
    command: "retract_evidence",
    expected_base_revision: factCatalog.revision,
    evidence_id: factsEvidenceRetractTarget.value,
    reason: factsEvidenceRetractReason.value.trim(),
  }), "Evidence-Rücknahme"));
  factsProvenanceCreate.addEventListener("click", () => submitFact(() => ({
    command: "create_provenance",
    expected_base_revision: factCatalog.revision,
    from: factsEndpointInput(factsProvenanceFrom),
    to: factsEndpointInput(factsProvenanceTo),
    relation: factsProvenanceRelation.value,
  }), "Provenance"));
  factsProvenanceRetract.addEventListener("click", () => submitFact(() => ({
    command: "retract_provenance",
    expected_base_revision: factCatalog.revision,
    provenance_id: factsProvenanceRetractTarget.value,
    reason: factsProvenanceRetractReason.value.trim(),
  }), "Provenance-Rücknahme"));
  factsCreateMask.addEventListener("click", () => submitFact(() => ({
    command: "create_mask",
    expected_base_revision: factCatalog.revision,
    context: factsContextInput(),
    selector: factsMaskSelectorInput(),
    validity: factsValidityEnabled.checked ? factsValidityInput() : null,
  }), "Mask"));
  factsCreateBoundary.addEventListener("click", () => submitFact(() => ({
    command: "create_replacement_boundary",
    expected_base_revision: factCatalog.revision,
    context: factsContextInput(),
    subject_id: factsSubject.value,
    predicate_id: factsPredicate.value,
    validity: factsValidityEnabled.checked ? factsValidityInput() : null,
  }), "ReplacementBoundary"));
  factsEventCreate.addEventListener("click", () => submitFact(factsEventCreateCommand, "Event"));
  factsEventMaskCreate.addEventListener("click", () => submitFact(() => ({
    command: "create_event_mask",
    expected_base_revision: factCatalog.revision,
    history_space_id: factsHistorySpace.value,
    layer_id: factsLayer.value,
    target_event_id: factsEventMaskTarget.value,
  }), "EventMask"));
  factsEventRelationCreate.addEventListener("click", () => submitFact(() => ({
    command: "create_event_relation",
    expected_base_revision: factCatalog.revision,
    from_event_id: factsEventRelationFrom.value,
    to_event_id: factsEventRelationTo.value,
    relation_kind: factsEventRelationKind.value,
  }), "Eventrelation"));
  factsEventSpanClose.addEventListener("click", () => submitFact(() => ({
    command: "close_event_span",
    expected_base_revision: factCatalog.revision,
    event_id: factsEventSpanCloseTarget.value,
    timeline_id: factsEventSpanCloseTimeline.value,
    close_nanoseconds: factsEventSpanCloseNanoseconds.value.trim(),
  }), "Event-Spanabschluss"));
  factsPreviewButton.addEventListener("click", () => runFactsPreview());
  factsQueryContinueButton.addEventListener("click", () => {
    if (factSearchCursor) runFactsPreview(sessionId, factSearchCursor);
  });
  factsCorrectionPreviewButton.addEventListener("click", previewAssertionCorrection);
  factsCorrectionCommitButton.addEventListener("click", () => {
    try {
      const command = pendingFactCorrectionCommands.assertion;
      if (!command) throw new Error("invalid_request");
      publishFact(command, "Assertion-Korrektur", "assertion_corrected");
    }
    catch (error) { factsWriteStatus.textContent = showError(error); clearFactActionPreviews(); }
  });
  factsEventCorrectionPreviewButton.addEventListener("click", previewEventCorrection);
  factsEventCorrectionCommitButton.addEventListener("click", () => {
    try {
      const command = pendingFactCorrectionCommands.event;
      if (!command) throw new Error("invalid_request");
      publishFact(command, "Event-Korrektur", "event_corrected");
    }
    catch (error) { factsWriteStatus.textContent = showError(error); clearFactActionPreviews(); }
  });
  factsLifecyclePreviewButton.addEventListener("click", previewFactLifecycle);
  factsLifecycleCommitButton.addEventListener("click", () => {
    try { publishFact(factsLifecycleCommand(), "Lebenszyklusaktion", "lifecycle_changed"); }
    catch (error) { factsWriteStatus.textContent = showError(error); clearFactActionPreviews(); }
  });
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
  timelineCalendarProfile.addEventListener("change", updateSchemaFormVisibility);
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
  updateBackupProfileDetails();
  renderStagedEventItems();
}
