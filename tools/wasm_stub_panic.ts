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
 * The `Location`s and source paths the removed calls pointed at are blanked as
 * well, so that the following `wasm-opt` run can pack them out of the data
 * section.
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

/** Returns the index just past the s-expression starting at `start`. */
function endOfExpression(wat: string, start: number): number {
  let depth = 0;
  for (let i = start;; i++) {
    if (wat[i] === "(") depth++;
    else if (wat[i] === ")" && --depth === 0) return i + 1;
  }
}

/** Replaces the body of `(func $name ...)` at `start` with a trap. */
function stubFunction(wat: string, start: number): string {
  const header = /^ \(func (\$[^\s)]+)((?: \((?:param|result)[^)]*\))*)/.exec(
    wat.slice(start),
  );
  if (!header) return wat;

  const stub = ` (func ${header[1]}${header[2]}\n  (unreachable)\n )`;
  return wat.slice(0, start) + stub + wat.slice(endOfExpression(wat, start));
}

/**
 * Replaces `(call $name ...)` with a trap.
 *
 * Trapping inside the callee is not enough: the caller still builds the
 * arguments, which is what keeps the message strings and the `Location`s of the
 * panic sites in the data section. `unreachable` is valid in any position, so
 * dropping the whole call expression is safe.
 */
function stubCalls(wat: string, name: string): string {
  const needle = `(call ${name}`;
  for (let at = wat.indexOf(needle); at !== -1; at = wat.indexOf(needle, at)) {
    const after = wat[at + needle.length];
    if (after !== "\n" && after !== ")") {
      at += needle.length;
      continue;
    }
    wat = wat.slice(0, at) + "(unreachable)" +
      wat.slice(endOfExpression(wat, at));
    at += "(unreachable)".length;
  }
  return wat;
}

/** One `(data ...)` segment of the module. */
interface Segment {
  start: number;
  end: number;
  address: number;
  bytes: Uint8Array;
}

/** Decodes the escapes binaryen writes inside a data string. */
function decodeData(body: string): Uint8Array {
  const escapes: Record<string, number> = {
    t: 9,
    n: 10,
    r: 13,
    '"': 34,
    "'": 39,
    "\\": 92,
  };
  const bytes: number[] = [];
  for (let i = 0; i < body.length;) {
    if (body[i] !== "\\") {
      bytes.push(body.charCodeAt(i++));
      continue;
    }
    const hex = body.slice(i + 1, i + 3);
    if (/^[0-9a-fA-F]{2}$/.test(hex)) {
      bytes.push(parseInt(hex, 16));
      i += 3;
    } else {
      bytes.push(escapes[body[i + 1]]);
      i += 2;
    }
  }
  return new Uint8Array(bytes);
}

/** The inverse of {@link decodeData}. */
function encodeData(bytes: Uint8Array): string {
  let out = "";
  for (const byte of bytes) {
    const char = String.fromCharCode(byte);
    if (byte >= 0x20 && byte < 0x7f && char !== '"' && char !== "\\") {
      out += char;
    } else {
      out += "\\" + byte.toString(16).padStart(2, "0");
    }
  }
  return out;
}

/** Collects the `(data ...)` segments of the module. */
function dataSegments(wat: string): Segment[] {
  const segments: Segment[] = [];
  const header = /^ \(data (?:\$\S+ )?\(i32\.const (\d+)\) "/gm;
  for (let match = header.exec(wat); match; match = header.exec(wat)) {
    let end = match.index + match[0].length;
    while (wat[end] !== '"' || countTrailingBackslashes(wat, end) % 2 === 1) {
      end++;
    }
    segments.push({
      start: match.index + match[0].length,
      end,
      address: Number(match[1]),
      bytes: decodeData(wat.slice(match.index + match[0].length, end)),
    });
  }
  return segments;
}

function countTrailingBackslashes(wat: string, at: number): number {
  let count = 0;
  while (wat[at - 1 - count] === "\\") count++;
  return count;
}

/**
 * Zeroes the panic `Location`s and the source paths they name.
 *
 * A range is only cleared when no `i32.const` in the module points into it, so
 * anything still live is left alone.
 */
function blankPanicData(wat: string): string {
  const segments = dataSegments(wat);
  if (segments.length === 0) return wat;

  const live = new Set<number>();
  for (const match of wat.matchAll(/\(i32\.const (\d+)\)/g)) {
    live.add(Number(match[1]));
  }
  const isFree = (from: number, to: number) => {
    for (let address = from; address < to; address++) {
      if (live.has(address)) return false;
    }
    return true;
  };

  // The source paths of the panic sites, and the Location records naming them.
  const paths = new Map<number, number>();
  for (const segment of segments) {
    const text = new TextDecoder("latin1").decode(segment.bytes);
    for (const match of text.matchAll(/[\x20-\x7e]{4,200}\.rs/g)) {
      paths.set(segment.address + match.index, match[0].length);
    }
  }

  let cleared = 0;
  const clear = (segment: Segment, offset: number, length: number) => {
    const address = segment.address + offset;
    if (!isFree(address, address + length)) return;
    segment.bytes.fill(0, offset, offset + length);
    cleared += length;
  };

  for (const segment of segments) {
    const view = new DataView(segment.bytes.buffer, segment.bytes.byteOffset);
    for (let offset = 0; offset + 16 <= segment.bytes.length; offset += 4) {
      const pointer = view.getUint32(offset, true);
      const length = view.getUint32(offset + 4, true);
      const line = view.getUint32(offset + 8, true);
      const column = view.getUint32(offset + 12, true);
      if (paths.get(pointer) !== length || line === 0 || column > 1000) {
        continue;
      }
      clear(segment, offset, 16);
    }
    for (const [address, length] of paths) {
      const offset = address - segment.address;
      if (offset >= 0 && offset + length <= segment.bytes.length) {
        clear(segment, offset, length);
      }
    }
  }

  // Whatever is left of a panic site is its message. A printable run can be
  // cleared once nothing points into it, neither code nor a pointer that is
  // still stored in the data itself.
  const anchored = new Set(live);
  for (const segment of segments) {
    const view = new DataView(segment.bytes.buffer, segment.bytes.byteOffset);
    for (let offset = 0; offset + 4 <= segment.bytes.length; offset += 4) {
      anchored.add(view.getUint32(offset, true));
    }
  }
  const isUnanchored = (from: number, to: number) => {
    for (let address = from; address < to; address++) {
      if (anchored.has(address)) return false;
    }
    return true;
  };

  for (const segment of segments) {
    const text = new TextDecoder("latin1").decode(segment.bytes);
    for (const match of text.matchAll(/[\x20-\x7e]{8,}/g)) {
      const address = segment.address + match.index;
      if (!isUnanchored(address, address + match[0].length)) continue;
      segment.bytes.fill(0, match.index, match.index + match[0].length);
      cleared += match[0].length;
    }
  }

  if (cleared > 0) {
    console.error(`wasm_stub_panic: blanked ${cleared} bytes of panic data`);
  }

  // Rewrite the segments back, last one first so the offsets stay valid.
  for (const segment of segments.reverse()) {
    wat = wat.slice(0, segment.start) + encodeData(segment.bytes) +
      wat.slice(segment.end);
  }
  return wat;
}

const watFile = await Deno.makeTempFile({ suffix: ".wat" });
try {
  await run("wasm-dis", [wasmFile, "-o", watFile, ...extraFlags]);
  let wat = await Deno.readTextFile(watFile);

  const names = [...wat.matchAll(/^ \(func (\$[^\s)]+)/gm)]
    .map((match) => match[1])
    .filter((name) => PANIC_ENTRY_POINTS.some((entry) => name.includes(entry)));

  // Drop the calls first, so that the arguments go with them, then trap in the
  // bodies that are still reachable through the function table.
  for (const name of names) wat = stubCalls(wat, name);
  for (const name of names) {
    const at = wat.indexOf(` (func ${name}\n`) >= 0
      ? wat.indexOf(` (func ${name}\n`)
      : wat.indexOf(` (func ${name} `);
    if (at >= 0) wat = stubFunction(wat, at);
  }

  wat = blankPanicData(wat);

  await Deno.writeTextFile(watFile, wat);
  await run("wasm-as", [watFile, "-o", wasmFile, ...extraFlags]);
} finally {
  await Deno.remove(watFile);
}
