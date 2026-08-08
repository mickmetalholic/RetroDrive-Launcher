import { createHash } from "node:crypto";
import { readFile, readdir, lstat, writeFile } from "node:fs/promises";
import { join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const projectDir = fileURLToPath(new URL("../", import.meta.url));
const skillsDir = resolve(projectDir, "skills");
const manifestPath = join(skillsDir, "skills-manifest.json");
const writeManifest = process.argv.includes("--write");
const MAX_FILE_BYTES = 32 * 1024 * 1024;
const MAX_TOTAL_BYTES = 128 * 1024 * 1024;

function fail(message) {
  throw new Error(message);
}

function normalizedRelative(path) {
  return relative(skillsDir, path).split(sep).join("/");
}

async function collectFiles(root, current = root, output = []) {
  const entries = await readdir(current, { withFileTypes: true });
  entries.sort((left, right) => left.name.localeCompare(right.name));
  for (const entry of entries) {
    const path = join(current, entry.name);
    const info = await lstat(path);
    if (info.isSymbolicLink()) fail(`Symlinks are not allowed in skills: ${path}`);
    if (info.isDirectory()) {
      await collectFiles(root, path, output);
    } else if (info.isFile()) {
      if (info.size > MAX_FILE_BYTES) fail(`Skill file is too large: ${path}`);
      output.push({ path, relative: relative(root, path).split(sep).join("/"), size: info.size });
    } else {
      fail(`Unsupported skill filesystem entry: ${path}`);
    }
  }
  return output;
}

async function treeHash(root, files) {
  const hash = createHash("sha256");
  let bytes = 0;
  const orderedFiles = [...files].sort((left, right) =>
    left.relative < right.relative ? -1 : left.relative > right.relative ? 1 : 0,
  );
  for (const file of orderedFiles) {
    const content = await readFile(file.path);
    bytes += content.byteLength;
    hash.update(Buffer.from(file.relative, "utf8"));
    hash.update(Buffer.from([0]));
    hash.update(content);
  }
  return { hash: hash.digest("hex"), bytes };
}

function frontmatterValue(frontmatter, key) {
  const line = frontmatter.match(new RegExp(`^${key}:\\s*(.+)$`, "m"))?.[1]?.trim();
  if (!line) return null;
  if (line.startsWith('"') && line.endsWith('"')) return JSON.parse(line);
  return line;
}

async function validateSkill(skillId) {
  const skillRoot = join(skillsDir, skillId);
  const skillFile = join(skillRoot, "SKILL.md");
  const agentFile = join(skillRoot, "agents", "openai.yaml");
  const source = await readFile(skillFile, "utf8");
  const match = source.match(/^---\r?\n([\s\S]*?)\r?\n---\r?\n/);
  if (!match) fail(`Skill frontmatter is missing: ${skillFile}`);
  const name = frontmatterValue(match[1], "name");
  const description = frontmatterValue(match[1], "description");
  if (name !== skillId) fail(`Skill name does not match directory: ${skillId}`);
  if (!description || description.length < 20) fail(`Skill description is too short: ${skillId}`);
  await lstat(agentFile).catch(() => fail(`agents/openai.yaml is missing: ${skillId}`));
  const agentSource = await readFile(agentFile, "utf8");
  const displayName = agentSource.match(/^\s*display_name:\s*["']([^"']+)["']\s*$/m)?.[1] ?? skillId;
  if (displayName.length < 1 || displayName.length > 80) fail(`Invalid display_name in agents/openai.yaml: ${skillId}`);
  const files = await collectFiles(skillRoot);
  const forbidden = files.find((file) => /(^|\/)README\.md$|(^|\/)(CHANGELOG|INSTALLATION_GUIDE)\.md$/i.test(file.relative));
  if (forbidden) fail(`Auxiliary documentation is not allowed inside a skill: ${forbidden.path}`);
  const result = await treeHash(skillRoot, files);
  files.sort((left, right) =>
    left.relative < right.relative ? -1 : left.relative > right.relative ? 1 : 0,
  );
  for (const file of files) {
    if (!/^[A-Za-z0-9._/-]+$/.test(file.relative)) fail(`Unsafe skill file path: ${file.relative}`);
  }
  return {
    id: skillId,
    displayName,
    description,
    sha256: result.hash,
    bytes: result.bytes,
    files: files.map((file) => file.relative),
  };
}

const entries = await readdir(skillsDir, { withFileTypes: true });
const skillIds = entries
  .filter((entry) => entry.isDirectory())
  .map((entry) => entry.name)
  .sort();
if (skillIds.length === 0) fail(`No skills found under ${skillsDir}`);
for (const skillId of skillIds) {
  if (!/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(skillId)) fail(`Invalid skill directory name: ${skillId}`);
}

const skills = [];
let totalBytes = 0;
for (const skillId of skillIds) {
  const skill = await validateSkill(skillId);
  totalBytes += skill.bytes;
  skills.push(skill);
}
if (totalBytes > MAX_TOTAL_BYTES) fail(`Skills exceed the total size limit: ${totalBytes}`);

const manifest = { schemaVersion: 1, skills };
const serialized = `${JSON.stringify(manifest, null, 2)}\n`;
if (writeManifest) {
  await writeFile(manifestPath, serialized, "utf8");
  console.log(`Wrote ${manifestPath}`);
} else {
  const existing = await readFile(manifestPath, "utf8").catch(() => null);
  if (existing !== serialized) fail(`skills-manifest.json is stale. Run: node scripts/validate-skills.mjs --write`);
  console.log(`Validated ${skills.length} skill(s), ${totalBytes} bytes.`);
}
