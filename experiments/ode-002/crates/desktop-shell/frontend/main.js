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
};

let sessionId;
let projectOpen = false;
let projectRevision = null;
let projectBusy = false;
let schemaBusy = false;
let schemaCurrentMode = true;
let selectedSchema = null;
let currentSchema = null;
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
  const canRead = projectOpen && !schemaBusy && !projectBusy;
  const canMutate = canRead && schemaCurrentMode;
  schemaRefreshButton.disabled = !canRead;
  schemaCreateButton.disabled = !canMutate;
  for (const control of schemaEditor.querySelectorAll("input, select, textarea, button")) {
    control.disabled = schemaBusy || projectBusy;
  }
  schemaCreateButton.disabled = !canMutate;
  schemaLifecyclePublish.disabled = !canMutate || stagedLifecycleChanges.length === 0;
  for (const button of schemaDefinitions.querySelectorAll("button[data-lifecycle]")) {
    button.disabled = !canMutate;
  }
  for (const button of schemaLifecyclePending.querySelectorAll("button")) {
    button.disabled = schemaBusy || projectBusy;
  }
  updateProjectControls();
}

function updateProjectControls() {
  createButton.disabled = projectBusy || schemaBusy || projectOpen;
  openButton.disabled = projectBusy || schemaBusy || projectOpen;
  closeButton.disabled = projectBusy || schemaBusy || !projectOpen;
}

function setBusy(busy) {
  projectBusy = busy;
  updateSchemaControls();
}

function renderProject(project) {
  projectOpen = Boolean(project.project_open);
  schemaPanel.hidden = !projectOpen;
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
  }
  if (!projectOpen) {
    selectedSchema = null;
    currentSchema = null;
    schemaDefinitions.replaceChildren();
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

async function manageSchema(command) {
  const response = await invoke("manage_schema", {
    request: { protocol_version: 1, session_id: sessionId, command },
  });
  if (response.protocol_version !== 1) throw new Error("unsupported_protocol");
  return response.result;
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
          operationStatus.textContent = "Schema- und Historienprüfung läuft …";
          await runSchemaSmoke(sessionId);
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
