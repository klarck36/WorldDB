import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { performance } from "node:perf_hooks";
import { fileURLToPath } from "node:url";
import { isMainThread, parentPort, Worker } from "node:worker_threads";

import {
  MAX_BYTES_VALUE,
  MAX_JSON_ENVELOPE_BYTES,
  TransportError,
  decodeEnvelope,
  encodeEnvelope,
} from "../src/transport.ts";

interface GoldenEnvelope {
  readonly id: string;
  readonly json: string;
}

interface FuzzSeed {
  readonly id: string;
  readonly category: string;
  readonly description: string;
}

class Rng {
  private state: number;

  constructor(seed: number) {
    this.state = seed >>> 0 || 1;
  }

  next(): number {
    let value = this.state;
    value ^= value << 13;
    value ^= value >>> 17;
    value ^= value << 5;
    this.state = value >>> 0 || 1;
    return this.state;
  }
}

const REQUEST_ID = "00000000-0000-7000-8000-000000000002";
const INVENTORY_PATH = new URL("../../../policy/decoder-inventory.tsv", import.meta.url);
const SEEDS_PATH = new URL("./transport-fuzz-seeds.tsv", import.meta.url);
const GOLDENS_PATH = new URL("./data/transport-v1.0-golden.tsv", import.meta.url);

function rows(contents: string, header: string, columnCount: number): string[][] {
  const lines = contents.trimEnd().split(/\r?\n/);
  if (lines[0] !== header) throw new Error(`unexpected TSV header: ${lines[0] ?? "<empty>"}`);
  return lines.slice(1).map((line) => {
    const columns = line.split("\t");
    if (columns.length !== columnCount || columns.some((column) => column.length === 0)) {
      throw new Error(`invalid TSV row: ${line.slice(0, 120)}`);
    }
    return columns;
  });
}

function loadInventory(): void {
  const inventory = rows(readFileSync(INVENTORY_PATH, "utf8"), "decoder_id\tfamily\tseed_id", 3);
  const targets = inventory.filter(([, family]) => family === "typescript");
  if (targets.length !== 1 || targets[0]?.[0] !== "typescript.json_envelope"
      || targets[0]?.[2] !== "typescript/transport-v1.0-golden") {
    throw new Error("shared decoder inventory must register the TypeScript JSON envelope");
  }
}

function loadSeeds(): FuzzSeed[] {
  return rows(readFileSync(SEEDS_PATH, "utf8"), "seed_id\tcategory\tdescription", 3)
    .map((columns) => {
      const [id, category, description] = columns;
      if (id === undefined || category === undefined || description === undefined) {
        throw new Error("TypeScript fuzz seed row is incomplete");
      }
      return { id, category, description };
    });
}

function loadGoldens(): GoldenEnvelope[] {
  return rows(readFileSync(GOLDENS_PATH, "utf8"), "vector\ttype\tvalue\tjson", 4)
    .map((columns) => {
      const id = columns[0];
      const json = columns[3];
      if (id === undefined || json === undefined) throw new Error("TypeScript golden row is incomplete");
      return { id, json };
    });
}

function makeInput(seedId: string, golden: string, rng: Rng): string {
  switch (seedId) {
    case "canonical":
      return golden;
    case "empty_input":
      return "";
    case "truncate_last":
      return golden.slice(0, Math.max(0, golden.length - 1));
    case "flip_one_bit": {
      if (golden.length === 0) return "x";
      const index = rng.next() % golden.length;
      const code = golden.charCodeAt(index) ?? 0;
      return `${golden.slice(0, index)}${String.fromCharCode(code ^ (1 << (rng.next() % 8)))}${golden.slice(index + 1)}`;
    }
    case "append_junk":
      return `${golden}!`;
    case "duplicate_key":
      return golden.replace('"major":1', '"major":1,"major":1');
    case "malformed_escape": {
      const marker = '"request_id":"';
      const valueStart = golden.indexOf(marker);
      if (valueStart < 0) return golden;
      const contentStart = valueStart + marker.length;
      const contentEnd = golden.indexOf('"', contentStart);
      if (contentEnd < 0) return golden;
      return `${golden.slice(0, contentStart)}\\x${golden.slice(contentEnd)}`;
    }
    case "oversized_object_fields":
      return '{"a":0,"b":0,"c":0,"d":0}';
    case "oversized_object_graph": {
      let nested = "0";
      for (let index = 0; index < 17; index += 1) nested = `{"a":${nested}}`;
      return nested;
    }
    case "oversized_scalar_string": {
      const maxTextChars = Math.floor((MAX_BYTES_VALUE * 4 + 2) / 3);
      return `{"value":"${"A".repeat(maxTextChars + 1)}"}`;
    }
    case "oversized_bytes_value": {
      const maxBase64Chars = Math.floor((MAX_BYTES_VALUE * 4 + 2) / 3);
      const value = "A".repeat(maxBase64Chars + 1);
      return `{"protocol":{"major":1,"minor":0},"request_id":"${REQUEST_ID}","operation":{"type":"transport_probe","data":{"scalar":{"type":"bytes","data":{"value":"${value}"}}}}}`;
    }
    case "oversized_utf8_frame": {
      const value = "€".repeat(Math.ceil(MAX_JSON_ENVELOPE_BYTES / 3));
      return `{"padding":"${value}"}`;
    }
    default:
      throw new Error(`unknown TypeScript fuzz seed ${seedId}`);
  }
}

function exercise(input: string): string | undefined {
  try {
    const decoded = decodeEnvelope(input);
    const canonical = encodeEnvelope(decoded);
    if (encodeEnvelope(decodeEnvelope(canonical)) !== canonical) {
      return "canonical transport roundtrip changed its spelling";
    }
    return undefined;
  } catch (error) {
    if (error instanceof TransportError) return undefined;
    const message = error instanceof Error ? `${error.name}: ${error.message}` : String(error);
    return message.slice(0, 240);
  }
}

function runSmoke(): void {
  loadInventory();
  const seeds = loadSeeds();
  const goldens = loadGoldens();
  if (seeds.length !== 12 || goldens.length !== 9) {
    throw new Error(`unexpected TypeScript fuzz corpus size: seeds=${seeds.length}, goldens=${goldens.length}`);
  }
  const rng = new Rng(0x574f524c);
  for (const golden of goldens) {
    const crash = exercise(golden.json);
    if (crash) throw new Error(`canonical ${golden.id} crashed: ${crash}`);
  }
  for (const seed of seeds) {
    const input = makeInput(seed.id, goldens[0]?.json ?? "", rng);
    const crash = exercise(input);
    if (crash) throw new Error(`${seed.id} crashed: ${crash}`);
  }
  process.stdout.write(`TRANSPORT FUZZ SMOKE PASS: ${goldens.length} golden envelopes, ${seeds.length} seeds, 1 TypeScript decoder\n`);
}

function parseCampaignOptions(): {
  readonly seconds: number;
  readonly seed: bigint;
  readonly maxInputBytes: number;
  readonly inputTimeoutSeconds: number;
} {
  const durationText = process.env.WORLDDB_TRANSPORT_FUZZ_CPU_SECONDS ?? "3600";
  const seconds = Number(durationText);
  if (!Number.isSafeInteger(seconds) || seconds < 1) {
    throw new Error("WORLDDB_TRANSPORT_FUZZ_CPU_SECONDS must be a positive safe integer");
  }
  const seedText = process.env.WORLDDB_TRANSPORT_FUZZ_SEED ?? "0x574f524c44444232";
  const seed = BigInt(seedText);
  if (seed < 0n || seed > ((1n << 64n) - 1n)) {
    throw new Error("WORLDDB_TRANSPORT_FUZZ_SEED must fit in u64");
  }
  const maxInputBytes = Number(process.env.WORLDDB_TRANSPORT_FUZZ_MAX_INPUT_BYTES ?? "16777216");
  if (!Number.isSafeInteger(maxInputBytes) || maxInputBytes < 1) {
    throw new Error("WORLDDB_TRANSPORT_FUZZ_MAX_INPUT_BYTES must be a positive safe integer");
  }
  const inputTimeoutSeconds = Number(process.env.WORLDDB_TRANSPORT_FUZZ_INPUT_TIMEOUT_SECONDS ?? "5");
  if (!Number.isSafeInteger(inputTimeoutSeconds) || inputTimeoutSeconds < 1) {
    throw new Error("WORLDDB_TRANSPORT_FUZZ_INPUT_TIMEOUT_SECONDS must be a positive safe integer");
  }
  return { seconds, seed, maxInputBytes, inputTimeoutSeconds };
}

function parseSourceRevision(value: string | undefined): string {
  if (value === undefined || !/^[0-9a-f]{40}$/.test(value)) {
    throw new Error("WORLDDB_SOURCE_REVISION must be a full lowercase 40-character Git hash");
  }
  return value;
}

function createParserWorker(): Worker {
  return new Worker(import.meta.url, { argv: [] });
}

function exerciseWithTimeout(
  worker: Worker,
  id: number,
  input: string,
  timeoutMilliseconds: number,
): Promise<string | undefined> {
  return new Promise((resolvePromise, rejectPromise) => {
    let settled = false;
    const finish = (failure?: string): void => {
      if (settled) return;
      settled = true;
      clearTimeout(timeout);
      worker.off("message", onMessage);
      worker.off("error", onError);
      resolvePromise(failure);
    };
    const onMessage = (message: { readonly id: number; readonly failure?: string }): void => {
      if (message.id === id) finish(message.failure);
    };
    const onError = (error: Error): void => {
      if (settled) return;
      settled = true;
      clearTimeout(timeout);
      worker.off("message", onMessage);
      worker.off("error", onError);
      rejectPromise(error);
    };
    const timeout = setTimeout(() => finish(`input_timeout>${timeoutMilliseconds}ms`), timeoutMilliseconds);
    worker.on("message", onMessage);
    worker.once("error", onError);
    worker.postMessage({ id, input });
  });
}

async function runCampaign(): Promise<void> {
  loadInventory();
  const seeds = loadSeeds();
  const goldens = loadGoldens();
  const options = parseCampaignOptions();
  const sourceRevision = parseSourceRevision(process.env.WORLDDB_SOURCE_REVISION);
  const rng = new Rng(Number(options.seed & 0xffff_ffffn));
  const seedById = new Map(seeds.map((seed) => [seed.id, seed]));
  const firstGolden = goldens[0]?.json;
  if (!firstGolden || seedById.size !== seeds.length) throw new Error("TypeScript fuzz inputs are incomplete");

  const expensiveSeedIds = new Set([
    "oversized_scalar_string",
    "oversized_bytes_value",
    "oversized_utf8_frame",
  ]);
  const expensiveInputs = new Map<string, string>();
  for (const seedId of expensiveSeedIds) {
    expensiveInputs.set(seedId, makeInput(seedId, firstGolden, rng));
  }

  const callsBySeed = new Map(seeds.map((seed) => [seed.id, 0]));
  const crashes: string[] = [];
  let worker = createParserWorker();
  let nextInputId = 1;
  const cpuStart = process.cpuUsage();
  const wallStart = performance.now();
  const startedUnix = Math.floor(Date.now() / 1000);
  let totalCalls = 0;
  let rounds = 0;

  const runSeed = async (seedId: string, golden: string): Promise<void> => {
    const input = expensiveInputs.get(seedId) ?? makeInput(seedId, golden, rng);
    if (Buffer.byteLength(input, "utf8") > options.maxInputBytes) {
      crashes.push(`${seedId}:input_exceeds_max:${Buffer.byteLength(input, "utf8")}:${options.maxInputBytes}`);
      return;
    }
    const id = nextInputId;
    nextInputId += 1;
    const failure = await exerciseWithTimeout(
      worker,
      id,
      input,
      options.inputTimeoutSeconds * 1_000,
    );
    callsBySeed.set(seedId, (callsBySeed.get(seedId) ?? 0) + 1);
    totalCalls += 1;
    if (failure) {
      crashes.push(`${seedId}:${failure}`);
      if (failure.startsWith("input_timeout>")) {
        await worker.terminate();
      }
    }
  };

  try {
  for (const seed of seeds) {
    await runSeed(seed.id, firstGolden);
    if (crashes.length > 0) break;
  }
  const frequentSeeds = seeds.filter((seed) => !expensiveSeedIds.has(seed.id));
  while (crashes.length === 0) {
    const golden = goldens[rng.next() % goldens.length]?.json ?? firstGolden;
    for (const seed of frequentSeeds) {
      await runSeed(seed.id, golden);
      if (crashes.length > 0) break;
    }
    if (rounds % 1024 === 0) {
      for (const seedId of expensiveSeedIds) {
        await runSeed(seedId, firstGolden);
        if (crashes.length > 0) break;
      }
    }
    rounds += 1;
    const cpu = process.cpuUsage(cpuStart);
    if ((cpu.user + cpu.system) / 1_000_000 >= options.seconds) break;
  }
  } finally {
    await worker.terminate();
  }

  const cpu = process.cpuUsage(cpuStart);
  const report = {
    run_id: `M1-18-ts-${options.seed.toString(16).padStart(16, "0")}-${startedUnix}`,
    seed: `0x${options.seed.toString(16).padStart(16, "0")}`,
    started_unix_seconds: startedUnix,
    elapsed_seconds: Number(((performance.now() - wallStart) / 1000).toFixed(3)),
    cpu_seconds: Number(((cpu.user + cpu.system) / 1_000_000).toFixed(3)),
    rounds,
    source_revision: sourceRevision,
    decoder_targets: 1,
    calls_by_decoder: { "typescript.json_envelope": totalCalls },
    calls_by_seed: Object.fromEntries(callsBySeed),
    crashes,
    result: crashes.length === 0 ? "PASS_LOCAL" : "FAIL",
  };
  const reportPath = process.env.WORLDDB_TRANSPORT_FUZZ_REPORT;
  const outputDirectory = reportPath
    ? dirname(resolve(reportPath))
    : fileURLToPath(new URL("../../../target/fuzz-results/", import.meta.url));
  mkdirSync(outputDirectory, { recursive: true });
  const outputPath = reportPath
    ? resolve(reportPath)
    : resolve(outputDirectory, `m1-18-typescript-${startedUnix}.json`);
  writeFileSync(outputPath, `${JSON.stringify(report, null, 2)}\n`, "utf8");
  process.stdout.write(`TRANSPORT FUZZ ${report.result}: seed=${report.seed}, cpu=${report.cpu_seconds}s, rounds=${rounds}, calls=${totalCalls}, report=${outputPath}\n`);
  if (crashes.length > 0) process.exitCode = 1;
}

const invokedPath = process.argv[1];
if (!isMainThread) {
  parentPort?.on("message", (message: { readonly id: number; readonly input: string }) => {
    parentPort?.postMessage({ id: message.id, failure: exercise(message.input) });
  });
} else if (invokedPath && resolve(invokedPath) === fileURLToPath(import.meta.url)) {
  try {
    if (process.argv[2] === "--smoke") runSmoke();
    else if (process.argv[2] === "--campaign") await runCampaign();
    else throw new Error("choose --smoke or --campaign");
  } catch (error) {
    const message = error instanceof Error ? `${error.name}: ${error.message}` : String(error);
    process.stderr.write(`${message}\n`);
    process.exitCode = 1;
  }
}
