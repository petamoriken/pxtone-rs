// Requires every snapshot in tests/snapshots to be sample for sample what the
// original C++ implementation renders, held in tests/reference.
//
// Both sides are committed WAV files, so this needs nothing but Deno and runs in
// CI. The whole corpus matches, so anything at all is a failure: a decode that
// drifts from the original is a bug in the port, and this is what catches it.
//
//   deno task test:refs [ptcop|ptnoise]
//
// See tests/reference/README.md for how the reference side was produced and for
// the one place this port diverges on purpose.

const WAV_HEADER_LEN = 44;

const SUITES = [
  {
    kind: "ptcop",
    reference: "tests/reference/ptcop",
    snapshots: "tests/snapshots/ptcop",
  },
  {
    kind: "ptnoise",
    reference: "tests/reference/ptnoise",
    snapshots: "tests/snapshots/ptnoise",
  },
];

interface Difference {
  compared: number;
  differing: number;
  worst: number;
  firstAt: number | null;
}

/** Compares the sample data of two WAV files, up to the shorter of the two. */
function compare(reference: Uint8Array, snapshot: Uint8Array): Difference {
  const a = new DataView(
    reference.buffer,
    reference.byteOffset + WAV_HEADER_LEN,
    reference.byteLength - WAV_HEADER_LEN,
  );
  const b = new DataView(
    snapshot.buffer,
    snapshot.byteOffset + WAV_HEADER_LEN,
    snapshot.byteLength - WAV_HEADER_LEN,
  );
  const compared = Math.min(a.byteLength, b.byteLength) >> 1;
  let differing = 0;
  let worst = 0;
  let firstAt: number | null = null;
  for (let i = 0; i < compared; i++) {
    const x = a.getInt16(i * 2, true);
    const y = b.getInt16(i * 2, true);
    if (x !== y) {
      differing++;
      worst = Math.max(worst, Math.abs(x - y));
      if (firstAt === null) firstAt = i;
    }
  }
  return { compared, differing, worst, firstAt };
}

async function names(dir: string, suffix: string): Promise<string[]> {
  const found: string[] = [];
  for await (const entry of Deno.readDir(dir)) {
    if (entry.isFile && entry.name.endsWith(suffix)) found.push(entry.name);
  }
  found.sort();
  return found;
}

const only = Deno.args[0];
const failures: string[] = [];
let checked = 0;

for (const suite of SUITES) {
  if (only && only !== suite.kind) continue;

  const rendered = new Set(await names(suite.reference, ".wav"));
  const snapshots = new Set(await names(suite.snapshots, ".wav"));

  // A sample with no reference render, or a render with no snapshot, would
  // otherwise slip past the comparison entirely.
  for (const name of snapshots) {
    if (!rendered.has(name)) {
      failures.push(`${suite.kind}/${name}: no reference render`);
    }
  }
  for (const name of rendered) {
    if (!snapshots.has(name)) {
      failures.push(`${suite.kind}/${name}: no snapshot`);
      continue;
    }

    const d = compare(
      await Deno.readFile(`${suite.reference}/${name}`),
      await Deno.readFile(`${suite.snapshots}/${name}`),
    );
    checked++;
    if (d.differing > 0) {
      const share = (100 * d.differing / d.compared).toFixed(3);
      failures.push(
        `${suite.kind}/${name}: ${d.differing} of ${d.compared} samples ` +
          `(${share}%), worst ${d.worst}, first at ${d.firstAt}`,
      );
    }
  }
}

if (failures.length === 0) {
  console.log(`${checked} snapshot(s) match the reference exactly`);
  Deno.exit(0);
}

console.error(`${checked} snapshot(s) checked, ${failures.length} problem(s):`);
for (const failure of failures) console.error(`  ${failure}`);
console.error(
  "\nThe reference is what the original renders; a difference is this port " +
    "drifting from it.",
);
Deno.exit(1);
