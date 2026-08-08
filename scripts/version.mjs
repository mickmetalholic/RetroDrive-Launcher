import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDirectory = path.dirname(fileURLToPath(import.meta.url));
const projectRoot = process.env.RETRODRIVE_VERSION_ROOT
  ? path.resolve(process.env.RETRODRIVE_VERSION_ROOT)
  : path.resolve(scriptDirectory, "..");

const files = {
  package: path.join(projectRoot, "package.json"),
  tauri: path.join(projectRoot, "src-tauri", "tauri.conf.json"),
  cargo: path.join(projectRoot, "src-tauri", "Cargo.toml"),
  lock: path.join(projectRoot, "src-tauri", "Cargo.lock"),
};

const semverPattern = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;

function fail(message) {
  process.stderr.write(`${message}\n`);
  process.exit(1);
}

function readText(file) {
  try {
    return fs.readFileSync(file, "utf8");
  } catch (error) {
    fail(`Unable to read ${file}: ${error.message}`);
  }
}

function replaceSingle(source, pattern, replacement, label) {
  const matches = [...source.matchAll(new RegExp(pattern.source, pattern.flags.includes("g") ? pattern.flags : `${pattern.flags}g`))];
  if (matches.length !== 1) {
    fail(`Expected exactly one ${label}, found ${matches.length}`);
  }
  return source.replace(pattern, replacement);
}

function load() {
  const packageSource = readText(files.package);
  const tauriSource = readText(files.tauri);
  const cargoSource = readText(files.cargo);
  const lockSource = readText(files.lock);
  let packageJson;
  let tauriJson;
  try {
    packageJson = JSON.parse(packageSource);
    tauriJson = JSON.parse(tauriSource);
  } catch (error) {
    fail(`Unable to parse JSON version source: ${error.message}`);
  }
  const cargoMatch = cargoSource.match(
    /^\[package\][\s\S]*?^version\s*=\s*"([^"]+)"/m,
  );
  const lockMatch = lockSource.match(
    /^\[\[package\]\]\r?\nname = "retrodrive-launcher"\r?\nversion = "([^"]+)"/m,
  );
  if (!cargoMatch || !lockMatch) fail("Unable to locate Rust package versions");
  const versions = {
    package: packageJson.version,
    tauri: tauriJson.version,
    cargo: cargoMatch[1],
    lock: lockMatch[1],
  };
  for (const [source, version] of Object.entries(versions)) {
    if (typeof version !== "string" || !semverPattern.test(version)) {
      fail(`Invalid ${source} version: ${String(version)}`);
    }
  }
  const unique = new Set(Object.values(versions));
  if (unique.size !== 1) {
    fail(`Version sources disagree: ${JSON.stringify(versions)}`);
  }
  return { packageJson, tauriJson, cargoSource, lockSource, version: versions.package };
}

function nextVersion(current, bump) {
  const match = current.match(semverPattern);
  if (!match) fail(`Invalid current version: ${current}`);
  let major = Number(match[1]);
  let minor = Number(match[2]);
  let patch = Number(match[3]);
  if (bump === "major") {
    major += 1;
    minor = 0;
    patch = 0;
  } else if (bump === "minor") {
    minor += 1;
    patch = 0;
  } else if (bump === "patch") {
    patch += 1;
  } else {
    fail(`Unsupported version increment: ${bump}`);
  }
  return `${major}.${minor}.${patch}`;
}

function writeVersion(state, version) {
  state.packageJson.version = version;
  state.tauriJson.version = version;
  const cargoSource = replaceSingle(
    state.cargoSource,
    /(^\[package\][\s\S]*?^version\s*=\s*")[^"]+(".*$)/m,
    (_match, prefix, suffix) => `${prefix}${version}${suffix}`,
    "Cargo.toml package version",
  );
  const lockSource = replaceSingle(
    state.lockSource,
    /(^\[\[package\]\]\r?\nname = "retrodrive-launcher"\r?\nversion = ")[^"]+(".*$)/m,
    (_match, prefix, suffix) => `${prefix}${version}${suffix}`,
    "Cargo.lock package version",
  );
  fs.writeFileSync(files.package, `${JSON.stringify(state.packageJson, null, 2)}\n`);
  fs.writeFileSync(files.tauri, `${JSON.stringify(state.tauriJson, null, 2)}\n`);
  fs.writeFileSync(files.cargo, cargoSource);
  fs.writeFileSync(files.lock, lockSource);
}

const [command, value] = process.argv.slice(2);
const state = load();

if (command === "current") {
  process.stdout.write(`${state.version}\n`);
} else if (command === "verify") {
  if (!value || state.version !== value) {
    fail(`Expected version ${value || "<missing>"}, found ${state.version}`);
  }
  process.stdout.write(`${state.version}\n`);
} else if (command === "next") {
  process.stdout.write(`${nextVersion(state.version, value)}\n`);
} else if (command === "bump") {
  const version = nextVersion(state.version, value);
  writeVersion(state, version);
  process.stdout.write(`${version}\n`);
} else {
  fail("Usage: node scripts/version.mjs current | verify <version> | next <patch|minor|major> | bump <patch|minor|major>");
}
