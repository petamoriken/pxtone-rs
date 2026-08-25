// WASM load-time benchmark
// Usage: deno run --allow-read tools/bench_load.ts <wasm_path> [wasm_path_baseline]

import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const projectRoot = join(dirname(fileURLToPath(import.meta.url)), "..");

const WARMUP_RUNS = 5;
const MEASURE_RUNS = 21;

const PTCOP_DIR = "tests/sample/ptcop";
const PTNOISE_DIR = "tests/sample/ptnoise";

interface WasmExports {
  memory: WebAssembly.Memory;
  alloc: (size: number) => number;
  dealloc: (ptr: number, size: number) => void;
  service_new: (channels: number, sampleRate: number) => number;
  service_free: (svc: number) => void;
  service_read: (svc: number, ptr: number, len: number) => number;
  service_tones_ready: (svc: number) => number;
  service_render_noise: (
    svc: number,
    ptr: number,
    len: number,
  ) => [number, number];
}

/// The three phases a file goes through before it can be played, in ms summed
/// over the sample corpus.
interface Phases {
  read: number;
  tonesReady: number;
  renderNoise: number;
}

async function loadWasm(wasmPath: string): Promise<WasmExports> {
  const bytes = await Deno.readFile(wasmPath);
  const { instance } = await WebAssembly.instantiate(bytes, {});
  return instance.exports as unknown as WasmExports;
}

function median(times: number[]): number {
  times.sort((a, b) => a - b);
  return times[Math.floor(times.length / 2)];
}

function samples(dir: string): string[] {
  return [...Deno.readDirSync(join(projectRoot, dir))]
    .map((entry) => entry.name)
    .sort()
    .map((name) => join(projectRoot, dir, name));
}

// Reading a song and getting its voices ready. Both are timed separately:
// parsing is dominated by the event list and getting ready by the voices.
async function benchSong(
  exports: WasmExports,
  filePath: string,
): Promise<[number, number]> {
  const data = await Deno.readFile(filePath);
  const { memory, alloc, dealloc } = exports;
  const reads: number[] = [];
  const tones: number[] = [];

  for (let i = 0; i < WARMUP_RUNS + MEASURE_RUNS; i++) {
    const dataPtr = alloc(data.length);
    new Uint8Array(memory.buffer, dataPtr, data.length).set(data);

    const svc = exports.service_new(2, 44100);
    const t0 = performance.now();
    exports.service_read(svc, dataPtr, data.length);
    const t1 = performance.now();
    exports.service_tones_ready(svc);
    const t2 = performance.now();

    dealloc(dataPtr, data.length);
    exports.service_free(svc);
    if (i >= WARMUP_RUNS) {
      reads.push(t1 - t0);
      tones.push(t2 - t1);
    }
  }

  return [median(reads), median(tones)];
}

// Rendering a noise design to PCM, which a song reaches through its noise
// voices as well.
async function benchNoise(
  exports: WasmExports,
  filePath: string,
): Promise<number> {
  const data = await Deno.readFile(filePath);
  const { memory, alloc, dealloc } = exports;
  const times: number[] = [];

  for (let i = 0; i < WARMUP_RUNS + MEASURE_RUNS; i++) {
    const dataPtr = alloc(data.length);
    new Uint8Array(memory.buffer, dataPtr, data.length).set(data);

    const svc = exports.service_new(2, 44100);
    const t0 = performance.now();
    const [samplesPtr, samplesLen] = exports.service_render_noise(
      svc,
      dataPtr,
      data.length,
    );
    const elapsed = performance.now() - t0;

    if (samplesLen !== 0) dealloc(samplesPtr, samplesLen);
    dealloc(dataPtr, data.length);
    exports.service_free(svc);
    if (i >= WARMUP_RUNS) times.push(elapsed);
  }

  return median(times);
}

async function benchWasm(wasmPath: string): Promise<Phases> {
  const exports = await loadWasm(wasmPath);
  const phases: Phases = { read: 0, tonesReady: 0, renderNoise: 0 };

  for (const file of samples(PTCOP_DIR)) {
    const [read, tonesReady] = await benchSong(exports, file);
    phases.read += read;
    phases.tonesReady += tonesReady;
  }
  for (const file of samples(PTNOISE_DIR)) {
    phases.renderNoise += await benchNoise(exports, file);
  }

  return phases;
}

const LABELS: [keyof Phases, string][] = [
  ["read", "service_read"],
  ["tonesReady", "service_tones_ready"],
  ["renderNoise", "service_render_noise"],
];

const [wasmPathA, wasmPathB] = Deno.args;
if (!wasmPathA) {
  console.error(
    "Usage: deno run --allow-read tools/bench_load.ts <wasm_a> [wasm_b]",
  );
  Deno.exit(1);
}

console.log(`Benchmarking: ${wasmPathA}`);
const resultsA = await benchWasm(wasmPathA);

if (wasmPathB) {
  console.log(`Benchmarking: ${wasmPathB}`);
  const resultsB = await benchWasm(wasmPathB);

  console.log("\n--- Results (corpus totals, median of 21 runs a file) ---");
  console.log(
    `${"phase".padEnd(24)} ${"before".padStart(9)} ${"after".padStart(9)} ${
      "change".padStart(9)
    }`,
  );
  for (const [key, label] of LABELS) {
    const before = resultsB[key];
    const after = resultsA[key];
    const pct = ((after - before) / before * 100).toFixed(1);
    const sign = after < before ? "" : "+";
    console.log(
      `${label.padEnd(24)} ${before.toFixed(2).padStart(8)}ms ${
        after.toFixed(2).padStart(8)
      }ms ${(sign + pct + "%").padStart(9)}`,
    );
  }
} else {
  console.log("\n--- Results (corpus totals, median of 21 runs a file) ---");
  for (const [key, label] of LABELS) {
    console.log(`${label.padEnd(24)} ${resultsA[key].toFixed(2)}ms`);
  }
}
