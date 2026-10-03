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

const userMessages = {
  project_already_exists: "An diesem Ort gibt es bereits ein Projekt.",
  project_already_open: "Es ist bereits ein anderes Projekt geöffnet. Schließe es zuerst.",
  project_unavailable: "Dieses Konto hat keinen Zugriff auf das Projekt.",
  invalid_project: "Der ausgewählte Ordner enthält kein gültiges WorldDB-Projekt.",
  recovery_required: "Das Projekt benötigt eine Prüfung oder Wiederherstellung und wurde nicht geöffnet.",
  host_unavailable: "Der lokale WorldDB-Host ist gerade nicht verfügbar.",
  selection_cancelled: "Die Auswahl wurde abgebrochen.",
  invalid_request: "Bitte prüfe den Projektnamen.",
  unknown_commit_outcome: "Der Speicherstatus ist unklar. Prüfe das Projekt, bevor du es erneut anlegst.",
};

function errorCode(error) {
  if (typeof error === "string") return error;
  return error?.code ?? error?.message ?? "host_unavailable";
}

function showError(error) {
  const code = errorCode(error);
  return userMessages[code] ?? userMessages[code.split(":").at(-1)] ?? "Die Aktion konnte nicht abgeschlossen werden.";
}

function setBusy(busy) {
  for (const button of [createButton, openButton, closeButton]) {
    button.disabled = busy;
  }
}

function renderProject(project) {
  if (!project.project_open) {
    projectStatus.textContent = "Kein Projekt geöffnet";
    projectDetails.textContent = "Lege ein Projekt an oder öffne einen vorhandenen Projektordner.";
    createButton.disabled = false;
    openButton.disabled = false;
    closeButton.disabled = true;
    return;
  }
  projectStatus.textContent = project.project_name ?? "WorldDB-Projekt geöffnet";
  projectDetails.textContent = `Rolle: ${project.role ?? "unbekannt"} · Stand: ${project.revision ?? "–"}`;
  createButton.disabled = true;
  openButton.disabled = true;
  closeButton.disabled = false;
}

async function getProject(sessionId) {
  return invoke("project_status", { sessionId });
}

async function refreshProject(sessionId) {
  renderProject(await getProject(sessionId));
}

async function runSecurityProbes(sessionId) {
  const rejects = (promise) => promise.then(() => false, () => true);
  const invalidSessionRejected = await rejects(invoke("health", {
    request: { protocol_version: 1, session_id: "00".repeat(16) },
  }));
  const rendererPathRejected = await rejects(invoke("begin_transfer", {
    sessionId,
    request: {
      protocol_version: 1,
      total_bytes: 4,
      chunk_bytes: 4,
      project_path: "C:/renderer/chosen/database",
    },
  }));
  const rendererIdentityRejected = await rejects(invoke("close_project", {
    sessionId,
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

async function runProjectSmoke(sessionId) {
  if (role === "primary") {
    await invoke("create_project", {
      sessionId,
      request: { protocol_version: 1, project_name: "IPC-Smoke" },
    });
    return;
  }
  const deadline = Date.now() + 20000;
  let project;
  do {
    project = await getProject(sessionId);
    if (project.project_open) {
      await invoke("open_project", {
        sessionId,
        request: { protocol_version: 1 },
      });
      return;
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  } while (Date.now() < deadline);
  throw new Error("Das primäre Fenster hat das Testprojekt nicht angelegt.");
}

if (!invoke || !["primary", "secondary"].includes(role)) {
  windowStatus.textContent = "Dieses Fenster kann den lokalen WorldDB-Host nicht verwenden.";
} else {
  let sessionId;
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
        operationStatus.textContent = "Projektprüfung abgeschlossen.";
        await refreshProject(sessionId);
      }

      createButton.disabled = false;
      openButton.disabled = false;
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
}
