#!/usr/bin/env node
import { createHash } from "node:crypto";
import {
  existsSync,
  readFileSync,
  readdirSync,
  statSync,
  writeFileSync
} from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const packageJson = JSON.parse(readFileSync(path.join(repoRoot, "package.json"), "utf8"));

const installerPath = process.argv[2]
  ? path.resolve(process.argv[2])
  : findInstaller(path.join(repoRoot, "src-tauri/target/release/bundle/nsis"));
const version = process.argv[3] ?? process.env.GRIDMODE_VERSION ?? packageJson.version;
const outputPath = process.argv[4]
  ? path.resolve(process.argv[4])
  : path.join(path.dirname(installerPath), "latest.yml");

if (!existsSync(installerPath)) {
  console.error(`Installer not found: ${installerPath}`);
  process.exit(1);
}

const installer = readFileSync(installerPath);
const sha512 = createHash("sha512").update(installer).digest("base64");
const size = statSync(installerPath).size;
const fileName = path.basename(installerPath);

const latestYml = [
  `version: ${JSON.stringify(version)}`,
  "files:",
  `  - url: ${JSON.stringify(fileName)}`,
  `    sha512: ${JSON.stringify(sha512)}`,
  `    size: ${size}`,
  `path: ${JSON.stringify(fileName)}`,
  `sha512: ${JSON.stringify(sha512)}`,
  `releaseDate: ${JSON.stringify(new Date().toISOString())}`,
  ""
].join("\n");

writeFileSync(outputPath, latestYml);
console.log(`Wrote Electron updater metadata: ${outputPath}`);

function findInstaller(nsisDir) {
  if (!existsSync(nsisDir)) {
    console.error(`NSIS output directory not found: ${nsisDir}`);
    process.exit(1);
  }

  const candidates = readdirSync(nsisDir)
    .filter((fileName) => fileName.toLowerCase().endsWith(".exe"))
    .sort((left, right) => scoreInstaller(right) - scoreInstaller(left));

  if (candidates.length === 0) {
    console.error(`No Windows installer .exe found in ${nsisDir}`);
    process.exit(1);
  }

  return path.join(nsisDir, candidates[0]);
}

function scoreInstaller(fileName) {
  const normalized = fileName.toLowerCase();
  let score = 0;
  if (normalized.includes("gridmode")) score += 2;
  if (normalized.includes("setup")) score += 2;
  if (normalized.includes(packageJson.version)) score += 1;
  return score;
}
