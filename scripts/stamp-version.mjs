#!/usr/bin/env node
import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const version = process.argv[2] ?? process.env.GRIDMODE_VERSION;

if (!version || !/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(version)) {
  console.error("Usage: node scripts/stamp-version.mjs <major.minor.patch>");
  process.exit(1);
}

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

function readJson(relativePath) {
  return JSON.parse(readFileSync(path.join(repoRoot, relativePath), "utf8"));
}

function writeJson(relativePath, value) {
  writeFileSync(path.join(repoRoot, relativePath), `${JSON.stringify(value, null, 2)}\n`);
}

const packageJson = readJson("package.json");
packageJson.version = version;
writeJson("package.json", packageJson);

const tauriConfig = readJson("src-tauri/tauri.conf.json");
tauriConfig.version = version;
writeJson("src-tauri/tauri.conf.json", tauriConfig);

const cargoPath = path.join(repoRoot, "src-tauri/Cargo.toml");
const cargoToml = readFileSync(cargoPath, "utf8");
const updatedCargoToml = cargoToml.replace(/^version = ".*"$/m, `version = "${version}"`);

if (updatedCargoToml === cargoToml) {
  console.error("Could not find Cargo.toml package version to update.");
  process.exit(1);
}

writeFileSync(cargoPath, updatedCargoToml);
console.log(`Stamped GridMode version ${version}`);
