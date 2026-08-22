// Diffs the snapshots in tests/snapshots against the reference renders in
// tests/reference, so that the corpus is checked against what the original C++
// implementation produces rather than only against itself.
//
// Both sides are committed WAV files, so this needs nothing but Deno and runs in
// CI. The port does not agree with the reference everywhere yet, so the pass
// mark is `tests/reference/expected.toml`, which records where each file stands:
// a file that drifts further from the reference fails, and one that gets closer
// asks to be recorded.
//
//   deno task test:refs [ptcop|ptnoise]
//   UPDATE_REFS=1 deno task test:refs      # rewrite expected.toml
//
// See tests/reference/README.md for how the reference side was produced.

import { parse, stringify } from "@std/toml";

const EXPECTED_PATH = "tests/reference/expected.toml";
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

interface Expected {
  path: string;
  differing: number;
  worst: number;
}

async function readExpected(): Promise<Map<string, Expected>> {
  const expected = new Map<string, Expected>();
  try {
    const table = parse(await Deno.readTextFile(EXPECTED_PATH));
    for (const entry of (table.file ?? []) as Expected[]) {
      expected.set(entry.path, entry);
    }
  } catch {
    // No file yet; every result counts as new.
  }
  return expected;
}

const only = Deno.args[0];
const update = Deno.env.get("UPDATE_REFS") === "1";
const expected = await readExpected();

const results: { path: string; d: Difference }[] = [];
const rows: string[] = [];
const worse: string[] = [];
const better: string[] = [];
const missing: string[] = [];

for (const suite of SUITES) {
  if (only && only !== suite.kind) continue;

  const names: string[] = [];
  for await (const entry of Deno.readDir(suite.reference)) {
    if (entry.isFile && entry.name.endsWith(".wav")) names.push(entry.name);
  }
  names.sort();

  for (const name of names) {
    const stem = name.replace(/\.wav$/, "");
    const path = `${suite.kind}/${stem}`;
    const snapshot = `${suite.snapshots}/${name}`;
    try {
      await Deno.stat(snapshot);
    } catch {
      missing.push(path);
      continue;
    }

    const d = compare(
      await Deno.readFile(`${suite.reference}/${name}`),
      await Deno.readFile(snapshot),
    );
    results.push({ path, d });

    const share = (100 * d.differing / d.compared).toFixed(3);
    rows.push(
      `${stem.padEnd(34)} ${String(d.differing).padStart(9)} ${
        (share + "%").padStart(8)
      } ${String(d.worst).padStart(7)} ${String(d.firstAt ?? "-").padStart(9)}`,
    );

    const was = expected.get(path);
    if (!was) {
      if (d.differing > 0) worse.push(`${path}: new, ${d.differing} differing`);
    } else if (d.differing > was.differing || d.worst > was.worst) {
      worse.push(
        `${path}: ${was.differing} differing / worst ${was.worst} -> ` +
          `${d.differing} / ${d.worst}`,
      );
    } else if (d.differing < was.differing || d.worst < was.worst) {
      better.push(
        `${path}: ${was.differing} differing / worst ${was.worst} -> ` +
          `${d.differing} / ${d.worst}`,
      );
    }
  }
}

console.log(
  `\n${"file".padEnd(34)} ${"differing".padStart(9)} ${"share".padStart(8)} ${
    "worst".padStart(7)
  } ${"first".padStart(9)}`,
);
for (const row of rows) console.log(row);

const identical = results.filter(({ d }) => d.differing === 0).length;
console.log(
  `\n${identical} of ${results.length} snapshot(s) identical to the reference`,
);

if (update) {
  const file = results.map(({ path, d }) => ({
    path,
    differing: d.differing,
    worst: d.worst,
  }));
  await Deno.writeTextFile(
    EXPECTED_PATH,
    "# Where each snapshot stands against tests/reference, as recorded by\n" +
      "# `UPDATE_REFS=1 deno task test:refs`. A file that drifts further from\n" +
      "# the reference fails the check; one that gets closer asks to be\n" +
      "# recorded here.\n\n" + stringify({ file }),
  );
  console.log(`wrote ${EXPECTED_PATH}`);
  Deno.exit(0);
}

for (const line of better) console.log(`closer:  ${line}`);
for (const line of missing) console.log(`skipped: ${line}, no snapshot`);
for (const line of worse) console.error(`WORSE:   ${line}`);

if (better.length > 0) {
  console.log(
    `\n${better.length} file(s) moved closer to the reference; rerun with ` +
      `UPDATE_REFS=1 to record it.`,
  );
}
if (worse.length > 0) {
  console.error(`\n${worse.length} file(s) moved away from the reference.`);
  Deno.exit(1);
}
