const role = new URLSearchParams(location.search).get("role") ?? "unknown";
const status = document.querySelector("#window-role");
const invoke = window.__TAURI__?.core?.invoke;

if (!invoke || !["primary", "secondary"].includes(role)) {
  status.textContent = "Dieses Fenster kann den lokalen WorldDB-Host nicht verwenden.";
} else {
  invoke("open_host_session")
    .then((ticket) => {
      if (ticket.protocol_version !== 1) {
        throw new Error("Unsupported protocol version");
      }
      return invoke("security_smoke_mode").then(async (smokeMode) => {
        if (smokeMode.protocol_version !== 1) {
          throw new Error("Unsupported protocol version");
        }
        if (smokeMode.enabled) {
          const rejects = (promise) => promise.then(() => false, () => true);
          const invalidSessionRejected = await rejects(invoke("health", {
            request: { protocol_version: 1, session_id: "00".repeat(16) },
          }));
          const rendererPathRejected = await rejects(invoke("begin_transfer", {
            sessionId: ticket.session_id,
            request: {
              protocol_version: 1,
              total_bytes: 4,
              chunk_bytes: 4,
              project_path: "C:/renderer/chosen/database",
            },
          }));
          const filesystemCommandRejected = await rejects(invoke("plugin:fs|read_text_file", {
            path: "C:/Windows/win.ini",
          }));
          if (!invalidSessionRejected || !rendererPathRejected || !filesystemCommandRejected) {
            throw new Error("A renderer security probe was unexpectedly accepted");
          }
        }
        return invoke("health", {
          request: {
            protocol_version: 1,
            session_id: ticket.session_id,
          },
        });
      });
    })
    .then((result) => {
      if (result.protocol_version !== 1) {
        throw new Error("Unsupported protocol version");
      }
      status.textContent = `Native ${role}-Ansicht mit lokaler Host-Sitzung verbunden.`;
    })
    .catch(() => {
      status.textContent = "Eine lokale Host-Sitzung konnte nicht eingerichtet werden.";
    });
}
