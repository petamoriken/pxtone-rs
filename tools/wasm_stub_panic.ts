/**
 * Replace the bodies of the panic entry points with `unreachable`.
 *
 * Usage: deno run --allow-run --allow-read --allow-write --allow-env \
 *          tools/wasm_stub_panic.ts <wasm_file> [extra binaryen flags...]
 *
 * `panic = "abort"` still formats the panic message first, which keeps
 * `core::fmt` and the panic hook in the binary. Nothing can observe that
 * message here (the module imports nothing, so it has nowhere to write), so the
 * entry points are replaced by a trap and the following `wasm-opt` pass drops
 * everything that only they reached. This is what the nightly-only
 * `panic_immediate_abort` does, applied to the finished module.
 *
 * Needs the name section, so the wasm profile keeps it (`strip = "debuginfo"`);
 * the final `wasm-opt` run drops it again.
 */

/** Mangled name fragments of the functions a panic goes through. */
const PANIC_ENTRY_POINTS = [
  "9panicking",
  "unwrap_failed",
  "expect_failed",
  "assert_failed",
  "slice_index_fail",
  "len_mismatch_fail",
  "capacity_overflow",
  "handle_alloc_error",
  "rust_begin_unwind",
  "rust_panic",
  "rust_start_panic",
  "begin_panic",
];

const [wasmFile, ...extraFlags] = Deno.args;
if (!wasmFile) {
  console.error("usage: wasm_stub_panic.ts <wasm_file> [flags...]");
  Deno.exit(1);
}

async function run(command: string, args: string[]) {
  const { success, stderr } = await new Deno.Command(command, { args })
    .output();
  if (!success) {
    console.error(new TextDecoder().decode(stderr));
    Deno.exit(1);
  }
}

/** Replaces the body of `(func $name ...)` at `start` with a trap. */
function stubFunction(wat: string, start: number): string {
  const header = /^ \(func (\$[^\s)]+)((?: \((?:param|result)[^)]*\))*)/.exec(
    wat.slice(start),
  );
  if (!header) return wat;

  let depth = 0;
  let end = start;
  for (;; end++) {
    if (wat[end] === "(") depth++;
    else if (wat[end] === ")" && --depth === 0) break;
  }

  const stub = ` (func ${header[1]}${header[2]}\n  (unreachable)\n )`;
  return wat.slice(0, start) + stub + wat.slice(end + 1);
}

const watFile = await Deno.makeTempFile({ suffix: ".wat" });
try {
  await run("wasm-dis", [wasmFile, "-o", watFile, ...extraFlags]);
  let wat = await Deno.readTextFile(watFile);

  for (const entry of PANIC_ENTRY_POINTS) {
    const pattern = new RegExp(`^ \\(func \\$\\S*${entry}\\S*`, "m");
    // Every match, and re-scanned each time because stubbing moves the rest.
    for (let match = pattern.exec(wat); match; match = pattern.exec(wat)) {
      const stubbed = stubFunction(wat, match.index);
      if (stubbed === wat) break;
      wat = stubbed;
    }
  }

  await Deno.writeTextFile(watFile, wat);
  await run("wasm-as", [watFile, "-o", wasmFile, ...extraFlags]);
} finally {
  await Deno.remove(watFile);
}
