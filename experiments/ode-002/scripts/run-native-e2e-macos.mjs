import { createHash, randomBytes } from 'node:crypto';
import { spawn, spawnSync } from 'node:child_process';
import { createReadStream } from 'node:fs';
import { access, mkdir, readFile, readdir, stat, writeFile } from 'node:fs/promises';
import { basename, dirname, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { runKeyboardDriver } from '../e2e/native-e2e/keyboard-driver.mjs';

const scriptRoot = dirname(fileURLToPath(import.meta.url));
const odeRoot = resolve(scriptRoot, '..');
const repositoryRoot = resolve(odeRoot, '..', '..');
const suitePath = join(odeRoot, 'e2e', 'native-suite.json');
const suite = JSON.parse(await readFile(suitePath, 'utf8'));
const stamp = new Date().toISOString().replaceAll('-', '').replaceAll(':', '').replace(/\.\d{3}Z$/, 'Z');
const runId = `m8-26b-${stamp}-${randomBytes(4).toString('hex')}`;
const runStarted = new Date();
const runRootArgIndex = process.argv.indexOf('--evidence-root');
const evidenceRoot = resolve(runRootArgIndex >= 0 ? process.argv[runRootArgIndex + 1] : join(odeRoot, 'evidence', 'native-e2e'));
const runRoot = join(evidenceRoot, runId);
const logRoot = join(runRoot, 'logs');
const rawRoot = join(runRoot, 'raw');
const tempRoot = join(runRoot, 'tmp');
const targetRoot = resolve(process.env.CARGO_TARGET_DIR ?? join(odeRoot, 'target'));
const odeManifest = join(odeRoot, 'Cargo.toml');
const rootManifest = join(repositoryRoot, 'Cargo.toml');
const platformRunner = 'scripts/run-native-e2e-macos.mjs';
const cargo = process.env.CARGO ?? 'cargo';
const pwsh = process.env.PWSH ?? 'pwsh';
const cases = [];
const builds = [];
const failures = [];
let recoveryCliEvidence = null;
let environment;
const repositoryState = {
  commit: spawnSync('git', ['rev-parse', 'HEAD'], { cwd: repositoryRoot, encoding: 'utf8' }).stdout?.trim() ?? '',
  branch: spawnSync('git', ['branch', '--show-current'], { cwd: repositoryRoot, encoding: 'utf8' }).stdout?.trim() ?? '',
  dirty: (spawnSync('git', ['status', '--porcelain'], { cwd: repositoryRoot, encoding: 'utf8' }).stdout ?? '').trim().length > 0,
};
const cleanParentEnvironment = { ...process.env };
for (const name of Object.keys(cleanParentEnvironment)) {
  if (name.startsWith('WORLDDB_ODE_') || name.startsWith('WORLDDB_M8_26_')) delete cleanParentEnvironment[name];
}
let runnerEnvironment = cleanParentEnvironment;

function sanitized(value) {
  let text = String(value ?? '');
  for (const [localPath, replacement] of [
    [repositoryRoot, '<repository>'],
    [process.env.HOME, '<home>'],
    [process.env.RUNNER_TEMP, '<runner-temp>'],
    [runRoot, '<evidence-run>'],
  ]) {
    if (localPath) text = text.split(localPath).join(replacement);
  }
  return text;
}

function formatUtc(date) {
  return date.toISOString();
}

function addCase(caseId, mode, status, startedAt, finishedAt, exitCode, checksPassed, details, artifacts = []) {
  cases.push({
    case_id: caseId,
    mode,
    status,
    started_at: startedAt ? formatUtc(startedAt) : null,
    finished_at: finishedAt ? formatUtc(finishedAt) : null,
    exit_code: exitCode,
    checks_passed: checksPassed,
    details: sanitized(details),
    artifacts,
  });
}

function addFailure(caseId, error, artifacts = []) {
  failures.push({ case_id: caseId, message: sanitized(error instanceof Error ? error.message : error), artifact_paths: artifacts });
}

async function exists(path) {
  try { await access(path); return true; } catch { return false; }
}

async function runCommand(label, command, args, options = {}) {
  const startedAt = new Date();
  const stdoutPath = join(logRoot, `${label}.stdout.log`);
  const stderrPath = join(logRoot, `${label}.stderr.log`);
  const child = spawn(command, args, {
    cwd: options.cwd ?? repositoryRoot,
    env: options.env ?? runnerEnvironment,
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  const stdoutChunks = [];
  const stderrChunks = [];
  child.stdout.on('data', chunk => stdoutChunks.push(chunk));
  child.stderr.on('data', chunk => stderrChunks.push(chunk));
  const [exitCode, signal] = await new Promise((resolveExit, reject) => {
    child.once('error', reject);
    child.once('close', (code, signalName) => resolveExit([code, signalName]));
  });
  const stdout = sanitized(Buffer.concat(stdoutChunks).toString('utf8'));
  const stderr = sanitized(Buffer.concat(stderrChunks).toString('utf8'));
  await writeFile(stdoutPath, stdout, 'utf8');
  await writeFile(stderrPath, stderr, 'utf8');
  return { startedAt, finishedAt: new Date(), exitCode, signal, stdout, stderr, artifacts: [`logs/${basename(stdoutPath)}`, `logs/${basename(stderrPath)}`] };
}

async function sha256File(path) {
  const hash = createHash('sha256');
  for await (const chunk of createReadStream(path)) hash.update(chunk);
  return hash.digest('hex');
}

function evidencePath(path) {
  const fromRepository = relative(repositoryRoot, path);
  if (fromRepository.split(sep)[0] !== '..') {
    return fromRepository.split(sep).join('/');
  }
  return sanitized(path).split(sep).join('/');
}

async function addBuildEvidence(mode, appPath, enginePath = null) {
  const appInfo = await stat(appPath);
  const engineInfo = enginePath ? await stat(enginePath) : null;
  const row = {
    mode,
    app_path: evidencePath(appPath),
    app_sha256: await sha256File(appPath),
    app_bytes: appInfo.size,
    engine_path: enginePath ? evidencePath(enginePath) : null,
    engine_sha256: enginePath ? await sha256File(enginePath) : null,
    engine_bytes: engineInfo ? engineInfo.size : null,
  };
  builds.push(row);
  return row;
}

async function buildStep(caseId, mode, cargoArgs, appPath = null, enginePath = null) {
  const result = await runCommand(caseId, cargo, cargoArgs, { env: runnerEnvironment, cwd: repositoryRoot });
  const passed = result.exitCode === 0 && (!appPath || await exists(appPath)) && (!enginePath || await exists(enginePath));
  const checks = passed ? 1 : 0;
  addCase(caseId, mode, passed ? 'PASS' : 'FAIL', result.startedAt, result.finishedAt, result.exitCode, checks,
    passed ? 'Locked native Cargo build completed.' : `Native Cargo build failed (exit ${result.exitCode ?? 'unavailable'}${result.signal ? `, signal ${result.signal}` : ''}).`, result.artifacts);
  if (!passed) {
    addFailure(caseId, `Cargo build failed with exit ${result.exitCode ?? 'unavailable'}.`, result.artifacts);
    return false;
  }
  if (appPath && caseId !== 'build_recovery_cli') await addBuildEvidence(mode, appPath, enginePath);
  return true;
}

function parseLastJsonLine(text) {
  for (const line of text.split(/\r?\n/).reverse()) {
    const trimmed = line.trim();
    if (!trimmed.startsWith('{') || !trimmed.endsWith('}')) continue;
    try { return JSON.parse(trimmed); } catch { }
  }
  return null;
}

function passCount(summary) {
  return Object.values(summary ?? {}).filter(value => value === 'PASS').length;
}

function ensureCatalogCase(caseId, mode) {
  const spec = suite.cases.find(item => item.id === caseId);
  if (!spec?.required || spec.drivers.macos !== platformRunner || !spec.modes.includes(mode)) {
    throw new Error(`The shared native E2E catalog does not bind ${caseId}/${mode} to the macOS runner.`);
  }
}

async function runPowerShellSmoke(caseId, mode, scriptName, appPath, enginePath = null) {
  ensureCatalogCase(caseId, mode);
  const caseStarted = new Date();
  const rawPath = join(rawRoot, caseId + '-' + mode);
  const args = ['-NoLogo', '-NoProfile', '-NonInteractive', '-File', join(scriptRoot, scriptName), '-Mode', mode, '-ExecutablePath', appPath, '-KeepArtifacts', '-ArtifactsRoot', rawPath];
  if (scriptName === 'run-writer-lock-smoke.ps1') args.push('-WorkspaceRoot', repositoryRoot);
  if (enginePath && scriptName === 'run-ipc-security-smoke.ps1') args.push('-EngineExecutablePath', enginePath);
  if (enginePath && scriptName === 'run-writer-lock-smoke.ps1') args.push('-EngineExecutablePath', enginePath);
  if (scriptName === 'run-ipc-security-smoke.ps1') args.push('-FactsTimeoutSeconds', '600');
  const result = await runCommand(caseId + '-' + mode, pwsh, args, { env: runnerEnvironment, cwd: repositoryRoot });
  const summary = parseLastJsonLine(result.stdout);
  const passed = result.exitCode === 0 && summary !== null && summary.mode === mode;
  const checks = passed ? passCount(summary) : 0;
  const artifacts = [...result.artifacts];
  if (await exists(rawPath)) artifacts.push(`raw/${basename(rawPath)}`);
  const details = passed ? `${scriptName} reported ${checks} passing checks.` : `${scriptName} failed (exit ${result.exitCode ?? 'unavailable'}); inspect the archived logs.`;
  addCase(caseId, mode, passed ? 'PASS' : 'FAIL', caseStarted, result.finishedAt, result.exitCode, checks, details, artifacts);
  if (!passed) addFailure(caseId, details, artifacts);
  return passed;
}

async function runKeyboardSmoke(caseId, mode, appPath, enginePath, recoveryCliPath, crashDuringCommit) {
  ensureCatalogCase(caseId, mode);
  const caseRootName = `${caseId}-${mode}`;
  const caseRoot = join(rawRoot, caseRootName);
  const databaseRoot = join(caseRoot, 'database');
  const caseTmpRoot = join(tempRoot, caseRootName);
  const startedAt = new Date();
  try {
    const result = await runKeyboardDriver({
      mode,
      appPath,
      engineExecutablePath: enginePath,
      recoveryCliPath,
      databaseRoot,
      caseRoot,
      tmpRoot: caseTmpRoot,
      crashDuringCommit,
    });
    const artifacts = result.artifacts.map(path => relative(runRoot, path).split(sep).join('/'));
    addCase(caseId, mode, 'PASS', startedAt, new Date(), 0, result.checksPassed, result.details, artifacts);
    return true;
  } catch (error) {
    const artifacts = await exists(caseRoot) ? [`raw/${caseRootName}`] : [];
    const details = error instanceof Error ? error.message : String(error);
    addCase(caseId, mode, 'FAIL', startedAt, new Date(), 1, 0, details, artifacts);
    addFailure(caseId, details, artifacts);
    return false;
  }
}

async function listFiles(directory) {
  const result = [];
  for (const item of await readdir(directory, { withFileTypes: true })) {
    const path = join(directory, item.name);
    if (item.isDirectory()) result.push(...await listFiles(path));
    else if (item.isFile() && item.name !== 'manifest.json') result.push(path);
  }
  return result;
}

async function fileSystemInfo(path) {
  const df = await runCommand('filesystem-df', 'df', ['-P', path]);
  const mountPoint = df.stdout.trim().split(/\r?\n/).at(-1)?.trim().split(/\s+/).at(-1);
  if (!mountPoint) throw new Error('Could not determine the native E2E evidence filesystem mount.');
  const disk = await runCommand('filesystem-diskutil', 'diskutil', ['info', mountPoint]);
  const combined = `${disk.stdout}\n${disk.stderr}`;
  const personality = /File System Personality:\s*([^\r\n]+)/i.exec(combined)?.[1]?.trim();
  const type = /Type \(Bundle\):\s*([^\r\n]+)/i.exec(combined)?.[1]?.trim();
  const fsName = /APFS/i.test(personality ?? '') || /\bapfs\b/i.test(type ?? '') ? 'APFS' : personality ?? type ?? 'unknown';
  return { filesystem: fsName, filesystemRoot: mountPoint, detail: sanitized(combined) };
}

async function commandVersion(command, args) {
  const result = await runCommand(`version-${basename(command)}`, command, args);
  if (result.exitCode !== 0) throw new Error(`Could not query ${command} version.`);
  return result.stdout.trim().split(/\r?\n/)[0];
}

async function markRemainingCasesNotRun(details) {
  const expected = suite.cases.filter(spec => spec.required).flatMap(spec => spec.modes.map(mode => [spec.id, mode]));
  for (const [caseId, mode] of expected) {
    if (!cases.some(item => item.case_id === caseId && item.mode === mode)) {
      addCase(caseId, mode, 'NOT_RUN', null, null, null, 0, details, []);
    }
  }
}

await mkdir(evidenceRoot, { recursive: true });
await mkdir(runRoot, { recursive: false });
await mkdir(logRoot, { recursive: true });
await mkdir(rawRoot, { recursive: true });
await mkdir(tempRoot, { recursive: true });
runnerEnvironment = {
  ...cleanParentEnvironment,
  CARGO_TARGET_DIR: targetRoot,
  CARGO_TERM_COLOR: 'never',
  TMPDIR: tempRoot,
};

try {
  if (process.platform !== 'darwin') throw new Error('The M8-26b runner only executes on native macOS.');
  if (suite.platform_profiles?.macos?.filesystem !== 'APFS') throw new Error('The shared native suite does not bind macOS to APFS.');
  const fsInfo = await fileSystemInfo(runRoot);
  if (fsInfo.filesystem !== 'APFS') throw new Error(`Native E2E evidence is on ${fsInfo.filesystem}, expected APFS.`);
  const osVersion = await commandVersion('sw_vers', ['-productVersion']);
  const osBuild = await commandVersion('sw_vers', ['-buildVersion']);
  const cargoVersion = await commandVersion(cargo, ['--version']);
  const rustcVersion = await commandVersion(process.env.RUSTC ?? 'rustc', ['--version']);
  const powershellVersion = await commandVersion(pwsh, ['--version']);
  environment = {
    os: 'macOS',
    os_version: `${osVersion} (${osBuild})`,
    architecture: process.arch,
    filesystem: fsInfo.filesystem,
    filesystem_root: fsInfo.filesystemRoot,
    shell: `Node ${process.version}; ${powershellVersion}`,
    runner: process.env.GITHUB_ACTIONS === 'true' ? 'GitHub Actions macOS hosted runner' : 'local native macOS runner',
    rustc: rustcVersion,
    cargo: cargoVersion,
  };

  const exe = process.platform === 'win32' ? '.exe' : '';
  const appPath = join(targetRoot, 'debug', `worlddb-ode-desktop-shell${exe}`);
  const enginePath = join(targetRoot, 'debug', `worlddb_ode_engine${exe}`);
  const recoveryCliPath = join(targetRoot, 'debug', `worlddb-cli${exe}`);
  let allBuildsOk = true;

  allBuildsOk &&= await buildStep('build_recovery_cli', 'both', ['build', '--locked', '--offline', '--manifest-path', rootManifest, '-p', 'worlddb-cli'], recoveryCliPath);
  if (allBuildsOk) {
    const cliInfo = await stat(recoveryCliPath);
    recoveryCliEvidence = {
      path: evidencePath(recoveryCliPath),
      sha256: await sha256File(recoveryCliPath),
      bytes: cliInfo.size,
    };
  }

  if (allBuildsOk) allBuildsOk &&= await buildStep('build_engine', 'sidecar', ['build', '--locked', '--offline', '--manifest-path', odeManifest, '-p', 'worlddb-ode-engine', '--bin', 'worlddb_ode_engine'], null, enginePath);
  if (allBuildsOk) allBuildsOk &&= await buildStep('build_in_process', 'in-process', ['build', '--locked', '--offline', '--manifest-path', odeManifest, '-p', 'worlddb-ode-desktop-shell'], appPath);
  if (allBuildsOk) {
    await runPowerShellSmoke('native_ipc_in_process', 'in-process', 'run-ipc-security-smoke.ps1', appPath);
    await runPowerShellSmoke('competing_process_in_process', 'in-process', 'run-writer-lock-smoke.ps1', appPath);
  }

  if (allBuildsOk) allBuildsOk &&= await buildStep('build_sidecar', 'sidecar', ['build', '--locked', '--offline', '--manifest-path', odeManifest, '-p', 'worlddb-ode-desktop-shell', '--no-default-features', '--features', 'sidecar'], appPath, enginePath);
  if (allBuildsOk) {
    await runPowerShellSmoke('native_ipc_sidecar', 'sidecar', 'run-ipc-security-smoke.ps1', appPath, enginePath);
    await runPowerShellSmoke('competing_process_sidecar', 'sidecar', 'run-writer-lock-smoke.ps1', appPath, enginePath);
  }

  if (allBuildsOk) allBuildsOk &&= await buildStep('build_webdriver_in_process', 'in-process', ['build', '--locked', '--offline', '--manifest-path', odeManifest, '-p', 'worlddb-ode-desktop-shell', '--features', 'native-e2e'], appPath);
  if (allBuildsOk) {
    await runKeyboardSmoke('keyboard_navigation', 'in-process', appPath, enginePath, recoveryCliPath, false);
    await runKeyboardSmoke('commit_crash_recovery', 'in-process', appPath, enginePath, recoveryCliPath, true);
  }

  if (allBuildsOk) allBuildsOk &&= await buildStep('build_webdriver_sidecar', 'sidecar', ['build', '--locked', '--offline', '--manifest-path', odeManifest, '-p', 'worlddb-ode-desktop-shell', '--no-default-features', '--features', 'sidecar,native-e2e'], appPath, enginePath);
  if (allBuildsOk) {
    await runKeyboardSmoke('keyboard_navigation', 'sidecar', appPath, enginePath, recoveryCliPath, false);
    await runKeyboardSmoke('commit_crash_recovery', 'sidecar', appPath, enginePath, recoveryCliPath, true);
  }
  if (!allBuildsOk) await markRemainingCasesNotRun('Not run because a required native build failed.');
} catch (error) {
  addFailure('suite_setup', error);
  await markRemainingCasesNotRun('Not run because native suite setup failed.');
} finally {
  const failedCases = cases.filter(item => item.status === 'FAIL');
  const incompleteCases = cases.filter(item => item.status === 'NOT_RUN' || item.status === 'DEFERRED');
  const status = failures.length || failedCases.length ? 'FAIL' : incompleteCases.length ? 'INCOMPLETE' : 'PASS';
  const artifactRows = [];
  for (const path of (await listFiles(runRoot)).sort()) {
    const info = await stat(path);
    artifactRows.push({
      path: relative(runRoot, path).split(sep).join('/'),
      sha256: await sha256File(path),
      bytes: info.size,
    });
  }
  const report = {
    schema_version: 1,
    suite_id: suite.suite_id,
    suite_version: suite.suite_version,
    task_id: 'M8-26b',
    run_id: runId,
    status,
    started_at: formatUtc(runStarted),
    finished_at: formatUtc(new Date()),
    repository: {
      commit: repositoryState.commit,
      dirty: repositoryState.dirty,
      branch: repositoryState.branch || 'detached',
    },
    environment: environment ?? {
      os: 'macOS', os_version: 'unavailable', architecture: process.arch,
      filesystem: 'APFS', filesystem_root: tempRoot, shell: `Node ${process.version}`,
      runner: process.env.GITHUB_ACTIONS === 'true' ? 'GitHub Actions macOS hosted runner' : 'local native macOS runner',
      rustc: 'unavailable', cargo: 'unavailable',
    },
    builds,
    recovery_cli: recoveryCliEvidence,
    cases,
    artifacts: artifactRows,
    failures,
  };
  await writeFile(join(runRoot, 'manifest.json'), `${JSON.stringify(report, null, 2)}\n`, 'utf8');
  process.stdout.write(`Native E2E run ${runId} completed with status ${status}.\n`);
  process.stdout.write(`Evidence: ${join(runRoot, 'manifest.json')}\n`);
  process.stdout.write(`Cases: ${cases.length} total; ${cases.filter(item => item.status === 'PASS').length} PASS; ${incompleteCases.length} NOT_RUN.\n`);
  if (status !== 'PASS') process.exitCode = 1;
}
