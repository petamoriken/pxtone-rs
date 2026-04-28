import source pxtoneWasm from "../target/wasm32-unknown-unknown/release/pxtone.wasm";

import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const projectRoot = join(dirname(fileURLToPath(import.meta.url)), "..");

interface WasmExports {
  memory: WebAssembly.Memory;
  alloc: (size: number) => number;
  dealloc: (ptr: number, size: number) => void;
  service_new: () => number;
  service_free: (ptr: number) => void;
  service_read: (svc: number, data: number, len: number) => number;
  service_tones_ready: (svc: number) => number;
  service_moo_preparation: (svc: number) => number;
  service_moo: (svc: number, buf: number, len: number) => number;
  service_is_end_vomit: (svc: number) => number;
  service_get_channels: (svc: number) => number;
  service_get_sample_rate: (svc: number) => number;
  service_render_noise: (
    svc: number,
    data: number,
    dataLen: number,
    outChannels: number,
    outSampleRate: number,
    outSamplesLen: number,
  ) => number;
}

function instantiate(): WasmExports {
  const instance = new WebAssembly.Instance(pxtoneWasm);
  return instance.exports as unknown as WasmExports;
}

function pcmToWav(samples: Uint8Array, channels: number, sampleRate: number): Uint8Array {
  const buf = new ArrayBuffer(44 + samples.length);
  const view = new DataView(buf);
  const enc = new TextEncoder();
  const str = (s: string, off: number) => enc.encode(s).forEach((b, i) => view.setUint8(off + i, b));
  str("RIFF", 0);
  view.setUint32(4, 36 + samples.length, true);
  str("WAVE", 8);
  str("fmt ", 12);
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true);
  view.setUint16(22, channels, true);
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * channels * 2, true);
  view.setUint16(32, channels * 2, true);
  view.setUint16(34, 16, true);
  str("data", 36);
  view.setUint32(40, samples.length, true);
  new Uint8Array(buf, 44).set(samples);
  return new Uint8Array(buf);
}

Deno.test("decoded ptcop matches reference (wasm)", async () => {
  const ex = instantiate();
  const sampleDir = join(projectRoot, "tests/sample/ptcop");
  const snapshotDir = join(projectRoot, "tests/snapshots/ptcop");

  const names: string[] = [];
  for await (const entry of Deno.readDir(sampleDir)) {
    if (entry.isFile && entry.name.endsWith(".ptcop")) names.push(entry.name);
  }
  names.sort();

  if (names.length === 0) throw new Error("no .ptcop files found in tests/sample/ptcop/");

  const failures: string[] = [];

  for (const name of names) {
    const stem = name.slice(0, -6);
    const ptcopData = await Deno.readFile(join(sampleDir, name));

    const dataPtr = ex.alloc(ptcopData.length);
    new Uint8Array(ex.memory.buffer, dataPtr, ptcopData.length).set(ptcopData);

    const svc = ex.service_new();

    if (ex.service_read(svc, dataPtr, ptcopData.length) !== 0) {
      failures.push(`${name}: service_read failed`);
      ex.dealloc(dataPtr, ptcopData.length);
      ex.service_free(svc);
      continue;
    }
    ex.dealloc(dataPtr, ptcopData.length);

    if (ex.service_tones_ready(svc) !== 0) {
      failures.push(`${name}: service_tones_ready failed`);
      ex.service_free(svc);
      continue;
    }
    if (ex.service_moo_preparation(svc) !== 0) {
      failures.push(`${name}: service_moo_preparation failed`);
      ex.service_free(svc);
      continue;
    }

    const channels = ex.service_get_channels(svc);
    const sampleRate = ex.service_get_sample_rate(svc);
    const chunkSize = channels * 2 * 4096;
    const bufPtr = ex.alloc(chunkSize);

    const chunks: Uint8Array[] = [];
    while (ex.service_is_end_vomit(svc) === 0) {
      if (ex.service_moo(svc, bufPtr, chunkSize) === 0) break;
      chunks.push(new Uint8Array(ex.memory.buffer, bufPtr, chunkSize).slice());
    }

    ex.dealloc(bufPtr, chunkSize);
    ex.service_free(svc);

    const totalLen = chunks.reduce((n, c) => n + c.length, 0);
    const pcm = new Uint8Array(totalLen);
    let off = 0;
    for (const c of chunks) { pcm.set(c, off); off += c.length; }

    const wav = pcmToWav(pcm, channels, sampleRate);
    const wavPath = join(snapshotDir, `${stem}.wav`);
    const expected = await Deno.readFile(wavPath);

    if (wav.length !== expected.length || wav.some((b, i) => b !== expected[i])) {
      failures.push(wavPath);
    }
  }

  if (failures.length > 0) {
    throw new Error(
      `Decoded output does not match reference (${failures.length} file(s)):\n${failures.join("\n")}`,
    );
  }
});

Deno.test("decoded ptnoise matches reference (wasm)", async () => {
  const ex = instantiate();
  const sampleDir = join(projectRoot, "tests/sample/ptnoise");
  const snapshotDir = join(projectRoot, "tests/snapshots/ptnoise");

  const names: string[] = [];
  for await (const entry of Deno.readDir(sampleDir)) {
    if (entry.isFile && entry.name.endsWith(".ptnoise")) names.push(entry.name);
  }
  names.sort();

  if (names.length === 0) throw new Error("no .ptnoise files found in tests/sample/ptnoise/");

  const failures: string[] = [];
  const svc = ex.service_new();

  // Allocate output pointer slots (4 bytes each; u32 on wasm32)
  const outChannels = ex.alloc(4);
  const outSampleRate = ex.alloc(4);
  const outSamplesLen = ex.alloc(4);

  for (const name of names) {
    const stem = name.slice(0, -8); // remove ".ptnoise"
    const ptnoiseData = await Deno.readFile(join(sampleDir, name));

    const dataPtr = ex.alloc(ptnoiseData.length);
    new Uint8Array(ex.memory.buffer, dataPtr, ptnoiseData.length).set(ptnoiseData);

    const samplesPtr = ex.service_render_noise(
      svc,
      dataPtr,
      ptnoiseData.length,
      outChannels,
      outSampleRate,
      outSamplesLen,
    );
    ex.dealloc(dataPtr, ptnoiseData.length);

    if (samplesPtr === 0) {
      failures.push(`${name}: service_render_noise failed`);
      continue;
    }

    const channels = new Uint32Array(ex.memory.buffer, outChannels, 1)[0];
    const sampleRate = new Uint32Array(ex.memory.buffer, outSampleRate, 1)[0];
    const samplesLen = new Uint32Array(ex.memory.buffer, outSamplesLen, 1)[0];

    const samples = new Uint8Array(ex.memory.buffer, samplesPtr, samplesLen).slice();
    ex.dealloc(samplesPtr, samplesLen);

    const wav = pcmToWav(samples, channels, sampleRate);
    const wavPath = join(snapshotDir, `${stem}.wav`);
    const expected = await Deno.readFile(wavPath);

    if (wav.length !== expected.length || wav.some((b, i) => b !== expected[i])) {
      failures.push(wavPath);
    }
  }

  ex.dealloc(outChannels, 4);
  ex.dealloc(outSampleRate, 4);
  ex.dealloc(outSamplesLen, 4);
  ex.service_free(svc);

  if (failures.length > 0) {
    throw new Error(
      `Decoded output does not match reference (${failures.length} file(s)):\n${failures.join("\n")}`,
    );
  }
});
