import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import vm from "node:vm";
import test from "node:test";

const helperPath = fileURLToPath(
  new URL("../crates/desktop-shell/frontend/renderer-health.js", import.meta.url),
);
const helperSource = await readFile(helperPath, "utf8");
const indexPath = fileURLToPath(
  new URL("../crates/desktop-shell/frontend/index.html", import.meta.url),
);
const indexSource = await readFile(indexPath, "utf8");

function installWithFakeDom() {
  const listeners = new Map();
  const status = { hidden: true, textContent: "" };
  const window = {
    addEventListener(eventName, listener) {
      listeners.set(eventName, listener);
    },
  };
  const document = {
    querySelector(selector) {
      return selector === "#renderer-status" ? status : null;
    },
  };

  vm.runInNewContext(helperSource, { document, window });
  return { listeners, status };
}

test("the error monitor loads before the application renderer", () => {
  const healthScript = indexSource.indexOf('<script src="renderer-health.js"></script>');
  const appScript = indexSource.indexOf('<script src="main.js"></script>');

  assert.ok(healthScript >= 0);
  assert.ok(appScript > healthScript);
  assert.match(indexSource, /id="renderer-status"[^>]*aria-live="assertive"[^>]*hidden/);
});

test("uncaught renderer errors show a safe WAL-status message", () => {
  const { listeners, status } = installWithFakeDom();
  listeners.get("error")({ message: "private path: C:\\Users\\example\\db" });

  assert.equal(status.hidden, false);
  assert.match(status.textContent, /WAL-Schreibstatus/);
  assert.doesNotMatch(status.textContent, /C:\\Users\\example/);
});

test("unhandled renderer rejections show the same safe status", () => {
  const { listeners, status } = installWithFakeDom();
  listeners.get("unhandledrejection")({ reason: "private backend detail" });

  assert.equal(status.hidden, false);
  assert.match(status.textContent, /unerwarteten Fehler/);
  assert.doesNotMatch(status.textContent, /private backend detail/);
});
