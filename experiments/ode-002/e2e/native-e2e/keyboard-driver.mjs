import { createRequire } from 'node:module';
import { spawn, spawnSync } from 'node:child_process';
import { createServer } from 'node:net';
import { setTimeout as delay } from 'node:timers/promises';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createWriteStream } from 'node:fs';
import { mkdir, readFile, writeFile } from 'node:fs/promises';

const packageRoot = fileURLToPath(new URL('.', import.meta.url));
const require = createRequire(join(process.env.WORLDDB_E2E_NODE_MODULE_ROOT ?? packageRoot, 'package.json'));
const { remote } = require('webdriverio');

const TAURI_WEBDRIVER_PORT = 'TAURI_WEBDRIVER_PORT';
const META = '\uE03D';
const BACKSPACE = '\uE003';
const TAB = '\uE004';
const ENTER = '\uE007';
const PROJECT_NAME = 'KeyboardSuiteProject';

function unsetTestEnvironment(env) {
  for (const key of Object.keys(env)) {
    if (key.startsWith('WORLDDB_ODE_') || key.startsWith('WORLDDB_M8_26_')) delete env[key];
  }
  for (const key of [
    'WORLDDB_ODE_AUTOCLOSE_MS',
    'WORLDDB_ODE_IPC_RESULT',
    'WORLDDB_ODE_PANIC_TEST',
    'WORLDDB_ODE_PROJECT_SMOKE_SKIP_AUTORUN',
    'WORLDDB_ODE_STREAM_TEST',
    'WORLDDB_ODE_UPDATE_TEST',
    'WORLDDB_M8_26_CRASH_AFTER_WAL_COMMIT_SYNC',
    'WORLDDB_M8_26_CRASH_SIGNAL_PATH',
  ]) delete env[key];
  return env;
}

async function unusedLoopbackPort() {
  const server = createServer();
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  const port = server.address().port;
  await new Promise((resolve, reject) => server.close(error => error ? reject(error) : resolve()));
  return port;
}

async function waitForApp(app, exitPromise, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  const port = Number(app.port);
  while (Date.now() < deadline) {
    if (app.child.exitCode !== null) {
      const result = await exitPromise;
      throw new Error(`Tauri app exited before WebDriver was ready (code ${result.code}, signal ${result.signal ?? 'none'}).`);
    }
    try {
      const response = await fetch(`http://127.0.0.1:${port}/status`, { signal: AbortSignal.timeout(1000) });
      if (response.ok) {
        const body = await response.json();
        if (body.value?.ready !== false) return;
      }
    } catch {
      // The embedded WebDriver listener may still be starting.
    }
    await delay(150);
  }
  throw new Error(`Timed out waiting for embedded WebDriver on 127.0.0.1:${port}.`);
}

function waitForExit(child, exitPromise, timeoutMs) {
  if (child.exitCode !== null) return Promise.resolve({ code: child.exitCode, signal: child.signalCode });
  return Promise.race([
    exitPromise,
    delay(timeoutMs).then(() => null),
  ]);
}

async function stopApp(child, exitPromise) {
  if (child.exitCode === null) {
    child.kill('SIGTERM');
    const stopped = await waitForExit(child, exitPromise, 10000);
    if (!stopped && child.exitCode === null) {
      child.kill('SIGKILL');
      await waitForExit(child, exitPromise, 5000);
    }
  }
}

async function waitForFile(path, timeoutMs, exitPromise, child) {
  const deadline = Date.now() + timeoutMs;
  let exitedAt = null;
  while (Date.now() < deadline) {
    try {
      await readFile(path);
      return;
    } catch {
      if (child.exitCode !== null) {
        exitedAt ??= Date.now();
        await exitPromise;
        if (Date.now() - exitedAt >= 2000) {
          throw new Error(`The app exited before writing expected evidence file ${path}.`);
        }
      }
      await delay(100);
    }
  }
  throw new Error(`Timed out waiting for evidence file ${path}.`);
}

async function appReport(reportPath) {
  return JSON.parse(await readFile(reportPath, 'utf8'));
}

async function parseCrashSignal(signalPath) {
  const text = await readFile(signalPath, 'utf8');
  const field = name => new RegExp(`(?:^|\\n)${name}=(.+)$`, 'm').exec(text)?.[1]?.trim();
  const checkpoint = field('checkpoint');
  const exitCode = Number(field('exit_code'));
  const processId = Number(field('process_id'));
  if (checkpoint !== 'after_wal_commit_sync' || exitCode !== 86 || !Number.isInteger(processId) || processId < 1) {
    throw new Error('The crash signal did not identify the expected synced WAL commit boundary.');
  }
  return { checkpoint, exitCode, processId, raw: text };
}

async function inspectRecovery(recoveryCliPath, databaseRoot, logPath) {
  const result = spawnSync(recoveryCliPath, ['--format', 'jsonl', 'v1', 'recovery', 'inspect', databaseRoot], {
    encoding: 'utf8',
    env: unsetTestEnvironment({ ...process.env }),
    timeout: 30000,
  });
  await writeFile(logPath, `${result.stdout ?? ''}${result.stderr ?? ''}`, 'utf8');
  if (result.error || result.status !== 0) {
    throw new Error(`Read-only recovery inspection failed (exit ${result.status ?? 'unavailable'}).`);
  }
  const report = (result.stdout ?? '').split(/\r?\n/).filter(line => line.trim().startsWith('{')).map(line => JSON.parse(line)).at(-1);
  const data = report?.outcome?.data;
  if (!['recovery_inspect', 'verify'].includes(report?.outcome?.type) || data?.status !== 'completed' || !data.safe_revision) {
    throw new Error('Read-only recovery inspection did not return a completed safe revision.');
  }
  return { type: report.outcome.type, status: data.status, safeRevision: String(data.safe_revision) };
}

export async function runKeyboardDriver({
  mode,
  appPath,
  engineExecutablePath,
  recoveryCliPath,
  databaseRoot,
  caseRoot,
  tmpRoot,
  crashDuringCommit = false,
}) {
  await mkdir(caseRoot, { recursive: false });
  await mkdir(tmpRoot, { recursive: true });
  const port = await unusedLoopbackPort();
  const reportPath = join(caseRoot, 'app-startup.json');
  const crashSignalPath = join(caseRoot, 'crash-signal.txt');
  const recoveryPath = join(caseRoot, 'recovery-inspect.jsonl');
  const statePath = join(caseRoot, 'webdriver-checks.json');
  const stdoutPath = join(caseRoot, 'app.stdout.log');
  const stderrPath = join(caseRoot, 'app.stderr.log');
  const stdout = createWriteStream(stdoutPath);
  const stderr = createWriteStream(stderrPath);
  const childEnv = unsetTestEnvironment({ ...process.env });
  Object.assign(childEnv, {
    TMPDIR: tmpRoot,
    WORLDDB_ODE_DATABASE: databaseRoot,
    WORLDDB_ODE_PROJECT_SMOKE_ROOT: databaseRoot,
    WORLDDB_ODE_PROJECT_SMOKE_SKIP_AUTORUN: '1',
    WORLDDB_ODE_SHOW_WINDOWS: '1',
    WORLDDB_ODE_RESULT: reportPath,
    [TAURI_WEBDRIVER_PORT]: String(port),
  });
  if (crashDuringCommit) {
    childEnv.WORLDDB_M8_26_CRASH_AFTER_WAL_COMMIT_SYNC = '1';
    childEnv.WORLDDB_M8_26_CRASH_SIGNAL_PATH = crashSignalPath;
  }
  if (mode === 'sidecar') childEnv.WORLDDB_ODE_ENGINE_EXECUTABLE = engineExecutablePath;

  const child = spawn(appPath, [], { cwd: dirname(appPath), env: childEnv, stdio: ['ignore', 'pipe', 'pipe'] });
  child.stdout.pipe(stdout);
  child.stderr.pipe(stderr);
  const exitPromise = new Promise(resolve => child.once('exit', (code, signal) => resolve({ code, signal })));
  const app = { child, exitPromise, port };
  let browser;
  let body = {};
  let failure = null;
  try {
    await waitForApp(app, exitPromise, 90000);
    await waitForFile(reportPath, 30000, exitPromise, child);
    const startup = await appReport(reportPath);
    if (startup.mode !== (mode === 'in-process' ? 'in_process' : 'sidecar')) {
      throw new Error(`The app startup report has mode ${startup.mode}, expected ${mode}.`);
    }
    browser = await remote({
      protocol: 'http',
      hostname: '127.0.0.1',
      port,
      path: '/',
      logLevel: 'error',
      connectionRetryTimeout: 15000,
      connectionRetryCount: 2,
      capabilities: { browserName: 'tauri' },
    });

    const nameInput = await browser.$('#project-name');
    const createButton = await browser.$('#create-project');
    await nameInput.waitForDisplayed({ timeout: 30000 });
    await createButton.waitForDisplayed({ timeout: 30000 });
    if (!(await createButton.isEnabled())) throw new Error('The native Neues Projekt control was disabled.');

    const accessibility = await browser.execute(() => {
      const input = document.querySelector('#project-name');
      const button = document.querySelector('#create-project');
      return {
        inputLabel: input?.labels?.[0]?.textContent?.trim() ?? '',
        inputRole: input?.getAttribute('role') ?? 'textbox',
        buttonName: button?.textContent?.trim() ?? '',
        buttonRole: button?.getAttribute('role') ?? 'button',
      };
    });
    if (accessibility.inputLabel !== 'Neuer Projektname' || accessibility.buttonName !== 'Neues Projekt') {
      throw new Error('The native WebView did not expose the expected accessible names for the project controls.');
    }

    await nameInput.click();
    await browser.keys([META, 'a']);
    await browser.keys(BACKSPACE);
    const clearedValue = await nameInput.getValue();
    await nameInput.addValue(PROJECT_NAME);
    const enteredValue = await nameInput.getValue();
    if (clearedValue !== '' || enteredValue !== PROJECT_NAME) {
      throw new Error(`Native keyboard input mismatch (cleared=${JSON.stringify(clearedValue)}, entered=${JSON.stringify(enteredValue)}).`);
    }

    await browser.keys(TAB);
    await browser.waitUntil(async () => browser.execute(() => document.activeElement?.id === 'create-project'), {
      timeout: 10000,
      timeoutMsg: 'Tab did not move native keyboard focus to Neues Projekt.',
    });
    const focusedId = await browser.execute(() => document.activeElement?.id ?? '');
    let enterDispatchError = null;
    try {
      await browser.keys(ENTER);
    } catch (error) {
      if (!crashDuringCommit) throw error;
      enterDispatchError = error instanceof Error ? error.message : String(error);
    }

    if (crashDuringCommit) {
      await waitForFile(crashSignalPath, 60000, exitPromise, child);
      const signal = await parseCrashSignal(crashSignalPath);
      if (mode === 'in-process') {
        const result = await waitForExit(child, exitPromise, 15000);
        if (!result || result.code !== 86 || signal.processId !== child.pid) {
          throw new Error(`The in-process desktop did not exit at the synced WAL boundary (exit ${result?.code ?? 'still running'}).`);
        }
      } else {
        if (signal.processId === child.pid || signal.processId !== startup.engine?.engine_process_id) {
          throw new Error('The sidecar crash signal did not identify the engine process.');
        }
      }
      await stopApp(child, exitPromise);
      const recovery = await inspectRecovery(recoveryCliPath, databaseRoot, recoveryPath);
      body = { mode, accessibility, keyboardTabToCreate: 'PASS', enterActivatedCreate: 'PASS', enterDispatchError, durableCommitCrash: 'PASS', readOnlyRecovery: 'PASS', signal, recovery };
    } else {
      await browser.waitUntil(async () => (await browser.$('#project-status').getText()) === PROJECT_NAME, {
        timeout: 60000,
        timeoutMsg: 'Enter did not create the temporary project.',
      });
      await browser.waitUntil(async () => browser.$('#close-project').isEnabled(), {
        timeout: 30000,
        timeoutMsg: 'The created project did not enable Projekt schließen.',
      });
      body = { mode, accessibility, keyboardTextEntry: 'PASS', keyboardTabToCreate: 'PASS', enterActivatedCreate: 'PASS', projectName: PROJECT_NAME };
      await stopApp(child, exitPromise);
    }

    if (mode === 'sidecar' && startup.engine?.engine_process_id) {
      const enginePid = Number(startup.engine.engine_process_id);
      const deadline = Date.now() + 10000;
      let alive = true;
      while (Date.now() < deadline) {
        try { process.kill(enginePid, 0); alive = true; } catch { alive = false; break; }
        await delay(100);
      }
      if (alive) throw new Error(`The sidecar engine process ${enginePid} remained alive after desktop shutdown.`);
      body.sidecarChildReaped = 'PASS';
    }

    await writeFile(statePath, `${JSON.stringify(body, null, 2)}\n`, 'utf8');
    return { checksPassed: Object.values(body).filter(value => value === 'PASS').length, details: `Native macOS WebDriver keyboard${crashDuringCommit ? ' and recovery' : ''} checks passed for ${mode}.`, artifacts: [reportPath, crashDuringCommit ? crashSignalPath : null, crashDuringCommit ? recoveryPath : null, statePath, stdoutPath, stderrPath].filter(Boolean) };
  } catch (error) {
    failure = error;
    body = { mode, crashDuringCommit, error: error instanceof Error ? error.message : String(error) };
    try { await writeFile(statePath, `${JSON.stringify(body, null, 2)}\n`, 'utf8'); } catch { }
    throw error;
  } finally {
    if (browser) {
      try { await Promise.race([browser.deleteSession(), delay(5000)]); } catch { }
    }
    if (child.exitCode === null) await stopApp(child, exitPromise);
    await new Promise(resolve => stdout.end(resolve));
    await new Promise(resolve => stderr.end(resolve));
    if (failure && child.exitCode === null) child.kill('SIGKILL');
  }
}
