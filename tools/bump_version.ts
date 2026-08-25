const version = Deno.args[0];
if (version === undefined || !/^\d+\.\d+\.\d+$/.test(version)) {
  console.error(
    "Usage: deno run --allow-read --allow-write tools/bump_version.ts <version>",
  );
  Deno.exit(1);
}

// Only the root [package] version. The workspace members under libs/ keep the
// upstream versions they were forked from, and the dependency tables carry
// version requirements of their own.
const packageVersion = /^\[package\]$[\s\S]*?^version = "[^"]+"$/m;

const cargoTomlPath = "./Cargo.toml";
const cargoToml = await Deno.readTextFile(cargoTomlPath);
if (!packageVersion.test(cargoToml)) {
  console.error("Could not find the [package] version in Cargo.toml");
  Deno.exit(1);
}

await Deno.writeTextFile(
  cargoTomlPath,
  cargoToml.replace(
    packageVersion,
    (section) => section.replace(/version = "[^"]+"/, `version = "${version}"`),
  ),
);

console.log(`Bumped version to v${version}`);
