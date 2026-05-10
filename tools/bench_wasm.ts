// WASM moo() benchmark
// Usage: deno run --allow-read tools/bench_wasm.ts <wasm_path> [wasm_path_baseline]

import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const projectRoot = join(dirname(fileURLToPath(import.meta.url)), "..");

const WARMUP_RUNS = 3;
const MEASURE_RUNS = 10;

const FILES = [
  "tests/sample/ptcop/overworld2_orche[Se-ko].ptcop",
  "tests/sample/ptcop/overworld2_nes[Se-ko].ptcop",
  "tests/sample/ptcop/5LOVE[Quois].ptcop",
];

interface WasmExports {
  memory: WebAssembly.Memory;
  alloc: (size: number) => number;
  dealloc: (ptr: number, size: number) => void;
  service_new: (channels: number, sampleRate: number) => number;
  service_free: (svc: number) => void;
  service_read: (svc: number, ptr: number, len: number) => number;
  service_tones_ready: (svc: number) => number;
  service_moo_preparation: (
    svc: number,
    start: number,
    meas: number,
    clockDiff: number,
  ) => number;
  service_moo: (svc: number, ptr: number, len: number) => [number, number];
}

async function loadWasm(wasmPath: string): Promise<WasmExports> {
  const bytes = await Deno.readFile(wasmPath);
  const { instance } = await WebAssembly.instantiate(bytes, {});
  return instance.exports as unknown as WasmExports;
}

async function benchFile(
  exports: WasmExports,
  filePath: string,
): Promise<number> {
  const data = await Deno.readFile(join(projectRoot, filePath));
  const {
    memory,
    alloc,
    dealloc,
    service_new,
    service_free,
    service_read,
    service_tones_ready,
    service_moo_preparation,
    service_moo,
  } = exports;

  function run(): number {
    const dataPtr = alloc(data.length);
    new Uint8Array(memory.buffer, dataPtr, data.length).set(data);

    const svc = service_new(2, 44100);
    service_read(svc, dataPtr, data.length);
    dealloc(dataPtr, data.length);
    service_tones_ready(svc);
    service_moo_preparation(svc, 0, 0, 0);

    const chunkSize = 2 * 2 * 4096;
    const bufPtr = alloc(chunkSize);

    const t0 = performance.now();
    while (true) {
      const [, written] = service_moo(svc, bufPtr, chunkSize);
      if (written === 0) break;
    }
    const elapsed = performance.now() - t0;

    dealloc(bufPtr, chunkSize);
    service_free(svc);
    return elapsed;
  }

  // warmup
  for (let i = 0; i < WARMUP_RUNS; i++) run();

  // measure
  const times: number[] = [];
  for (let i = 0; i < MEASURE_RUNS; i++) times.push(run());
  times.sort((a, b) => a - b);

  // median
  return times[Math.floor(times.length / 2)];
}

async function benchWasm(
  wasmPath: string,
): Promise<Map<string, number>> {
  const exports = await loadWasm(wasmPath);
  const results = new Map<string, number>();
  for (const file of FILES) {
    const stem = file.split("/").pop()!.replace(/\.ptcop$/, "");
    const ms = await benchFile(exports, file);
    results.set(stem, ms);
  }
  return results;
}

const [wasmPathA, wasmPathB] = Deno.args;
if (!wasmPathA) {
  console.error(
    "Usage: deno run --allow-read tools/bench_wasm.ts <wasm_a> [wasm_b]",
  );
  Deno.exit(1);
}

console.log(`Benchmarking: ${wasmPathA}`);
const resultsA = await benchWasm(wasmPathA);

if (wasmPathB) {
  console.log(`Benchmarking: ${wasmPathB}`);
  const resultsB = await benchWasm(wasmPathB);

  console.log("\n--- Results (median of 10 runs) ---");
  console.log(
    `${"file".padEnd(40)} ${"before".padStart(9)} ${"after".padStart(9)} ${
      "change".padStart(9)
    }`,
  );
  for (const [stem, msBefore] of resultsB) {
    const msAfter = resultsA.get(stem)!;
    const pct = ((msAfter - msBefore) / msBefore * 100).toFixed(1);
    const sign = msAfter < msBefore ? "" : "+";
    console.log(
      `${stem.padEnd(40)} ${msBefore.toFixed(1).padStart(8)}ms ${
        msAfter.toFixed(1).padStart(8)
      }ms ${(sign + pct + "%").padStart(9)}`,
    );
  }
} else {
  console.log("\n--- Results (median of 10 runs) ---");
  for (const [stem, ms] of resultsA) {
    console.log(`${stem.padEnd(40)} ${ms.toFixed(1)}ms`);
  }
}
