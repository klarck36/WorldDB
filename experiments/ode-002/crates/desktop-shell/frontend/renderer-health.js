(() => {
  const status = document.querySelector("#renderer-status");
  if (!status) return;

  const reportRendererFault = () => {
    status.hidden = false;
    status.textContent =
      "Die Oberfläche hat einen unerwarteten Fehler erkannt. Prüfe den Projektstatus und den WAL-Schreibstatus, bevor du eine Schreibaktion wiederholst.";
  };

  window.addEventListener("error", reportRendererFault);
  window.addEventListener("unhandledrejection", reportRendererFault);
})();
