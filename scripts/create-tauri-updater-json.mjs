#!/usr/bin/env node
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

const version = process.argv[2] ?? process.env.GRIDMODE_VERSION ?? packageJson.version;
const outputPath = process.argv[3]
  ? path.resolve(process.argv[3])
  : path.join(repoRoot, "src-tauri/target/release/bundle/latest.json");
const windowsDir = process.argv[4]
  ? path.resolve(process.argv[4])
  : path.join(repoRoot, "src-tauri/target/release/bundle/nsis");
const macDir = process.argv[5]
  ? path.resolve(process.argv[5])
  : path.join(repoRoot, "src-tauri/target/release/bundle/macos");

if (!version || !/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(version)) {
  console.error("Usage: node scripts/create-tauri-updater-json.mjs <major.minor.patch> [output] [windows-dir] [mac-dir]");
  process.exit(1);
}

const windowsInstaller = findFile(windowsDir, (fileName) => fileName.toLowerCase().endsWith(".exe"));
const windowsSignature = signatureFor(windowsInstaller);
const macArchive = findFile(macDir, (fileName) => fileName.toLowerCase().endsWith(".app.tar.gz"));
const macSignature = signatureFor(macArchive);
const releaseBaseUrl = `https://github.com/thedinz/GridMode/releases/download/v${version}`;

const manifest = {
  version,
  notes: `GridMode ${version}`,
  pub_date: new Date().toISOString(),
  platforms: {
    "windows-x86_64": {
      signature: readFileSync(windowsSignature, "utf8").trim(),
      url: `${releaseBaseUrl}/${encodeAssetName(path.basename(windowsInstaller))}`
    },
    "darwin-x86_64": {
      signature: readFileSync(macSignature, "utf8").trim(),
      url: `${releaseBaseUrl}/${encodeAssetName(path.basename(macArchive))}`
    }
  }
};

writeFileSync(outputPath, `${JSON.stringify(manifest, null, 2)}\n`);
console.log(`Wrote Tauri updater metadata: ${outputPath}`);

function findFile(directory, predicate) {
  if (!existsSync(directory) || !statSync(directory).isDirectory()) {
    console.error(`Directory not found: ${directory}`);
    process.exit(1);
  }

  const candidates = readdirSync(directory)
    .filter(predicate)
    .sort((left, right) => scoreArtifact(right) - scoreArtifact(left));

  if (candidates.length === 0) {
    console.error(`No matching artifact found in ${directory}`);
    process.exit(1);
  }

  return path.join(directory, candidates[0]);
}

function signatureFor(artifactPath) {
  const signaturePath = `${artifactPath}.sig`;
  if (!existsSync(signaturePath)) {
    console.error(`Signature not found: ${signaturePath}`);
    process.exit(1);
  }
  return signaturePath;
}

function scoreArtifact(fileName) {
  const normalized = fileName.toLowerCase();
  let score = 0;
  if (normalized.includes("gridmode")) score += 2;
  if (normalized.includes("setup")) score += 2;
  if (normalized.includes(version.toLowerCase())) score += 1;
  return score;
}

function encodeAssetName(fileName) {
  return fileName.split("/").map(encodeURIComponent).join("/");
}
