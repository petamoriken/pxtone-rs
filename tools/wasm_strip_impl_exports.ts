/**
 * Remove `_`-prefixed WASM exports using wasm-metadce.
 *
 * Usage: deno run --allow-run --allow-read --allow-write --allow-env \
 *          tools/wasm_strip_impl_exports.ts <wasm_file> [extra wasm-metadce flags...]
 *
 * Generates a metadce graph from all public (non-`_`-prefixed) exports, then
 * runs wasm-metadce to strip internal `_impl` export entries while keeping the
 * underlying functions (which remain reachable from the public wrappers).
 */

const [wasmFile, ...extraFlags] = Deno.args;

const wasmBytes = await Deno.readFile(wasmFile);
const wasmModule = await WebAssembly.compile(wasmBytes);
const names = WebAssembly.Module.exports(wasmModule)
  .map(({ name }) => name)
  .filter((name) => !name.startsWith("_"));

const graph = names.map((name) => ({ name, root: true, export: name }));

const tmpFile = await Deno.makeTempFile({ suffix: ".json" });
try {
  await Deno.writeTextFile(tmpFile, JSON.stringify(graph));
  const { success, stderr } = await new Deno.Command("wasm-metadce", {
    args: ["--graph-file", tmpFile, wasmFile, "-o", wasmFile, ...extraFlags],
  }).output();
  if (!success) {
    console.error(new TextDecoder().decode(stderr));
    Deno.exit(1);
  }
} finally {
  await Deno.remove(tmpFile);
}
