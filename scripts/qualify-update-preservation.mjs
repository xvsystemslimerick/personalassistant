#!/usr/bin/env node

import { createHash } from "node:crypto";
import {
  cpSync,
  lstatSync,
  mkdtempSync,
  mkdirSync,
  readFileSync,
  realpathSync,
  readdirSync,
  renameSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { basename, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";

const MAX_ARCHIVE_BYTES = 2 * 1024 * 1024 * 1024;
const APP_ROOT = "Personal Assistant.app";

function fail(message) {
  throw new Error(message);
}

function regularFile(path, description, maximumBytes = Number.MAX_SAFE_INTEGER) {
  const details = lstatSync(path);
  if (!details.isFile() || details.isSymbolicLink() || details.size === 0 || details.size > maximumBytes) {
    fail(`${description} is invalid`);
  }
}

function run(command, args) {
  const result = spawnSync(command, args, { encoding: "utf8", maxBuffer: 8 * 1024 * 1024 });
  if (result.status !== 0) fail(`${command} failed during update qualification`);
  return result.stdout;
}

function archiveEntries(archivePath) {
  const entries = run("tar", ["-tzf", archivePath]).split("\n").filter(Boolean);
  if (entries.length === 0 || entries.length > 20_000) fail("updater archive entry count is invalid");
  const seen = new Set();
  for (const rawEntry of entries) {
    const entry = rawEntry.replace(/\/$/, "");
    const components = entry.split("/");
    if (
      (entry !== APP_ROOT && !entry.startsWith(`${APP_ROOT}/`)) ||
      components.some((component) => component === ".." || component === "") ||
      seen.has(entry)
    ) {
      fail("updater archive escapes or duplicates the application bundle");
    }
    seen.add(entry);
  }
  return entries;
}

function treeDigest(root) {
  const digest = createHash("sha256");
  function visit(path, relative) {
    const details = lstatSync(path);
    if (details.isSymbolicLink()) fail("qualification state contains a symbolic link");
    if (details.isDirectory()) {
      digest.update(`d:${relative}\0`);
      for (const name of readdirSync(path).sort()) visit(join(path, name), join(relative, name));
      return;
    }
    if (!details.isFile()) fail("qualification state contains an unsupported file");
    digest.update(`f:${relative}:${details.mode & 0o777}:${details.size}\0`);
    digest.update(readFileSync(path));
  }
  visit(root, ".");
  return digest.digest("hex");
}

function validateExtractedTree(root) {
  const canonicalRoot = realpathSync(root);
  function visit(path) {
    const details = lstatSync(path);
    if (details.isSymbolicLink()) {
      const target = realpathSync(path);
      if (target !== canonicalRoot && !target.startsWith(`${canonicalRoot}/`)) {
        fail("updater payload contains an escaping symbolic link");
      }
      return;
    }
    if (details.isDirectory()) {
      for (const name of readdirSync(path)) visit(join(path, name));
      return;
    }
    if (!details.isFile()) fail("updater payload contains an unsupported file type");
  }
  visit(root);
}

function validateManifest(manifestPath, signaturePath, archivePath, version) {
  regularFile(manifestPath, "update manifest", 64 * 1024);
  regularFile(signaturePath, "update signature", 4 * 1024);
  const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
  const signature = readFileSync(signaturePath, "utf8").trim();
  const expectedArchive = `PersonalAssistant-${version}-aarch64.app.tar.gz`;
  const expectedUrl = `https://github.com/xvsystemslimerick/personalassistant/releases/download/v${version}/${expectedArchive}`;
  const platform = manifest?.platforms?.["darwin-aarch64"];
  if (
    basename(archivePath) !== expectedArchive ||
    manifest?.version !== version ||
    Object.keys(manifest?.platforms ?? {}).length !== 1 ||
    platform?.url !== expectedUrl ||
    platform?.signature !== signature
  ) {
    fail("update manifest does not bind the exact signed Apple Silicon artifact");
  }
}

export function qualifyUpdatePreservation(archivePath, signaturePath, manifestPath, version) {
  archivePath = resolve(archivePath);
  signaturePath = resolve(signaturePath);
  manifestPath = resolve(manifestPath);
  if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(version)) fail("release version is invalid");
  regularFile(archivePath, "updater archive", MAX_ARCHIVE_BYTES);
  archiveEntries(archivePath);
  validateManifest(manifestPath, signaturePath, archivePath, version);

  const staging = mkdtempSync(join(tmpdir(), "personal-assistant-update-"));
  try {
    const payload = join(staging, "payload");
    const applications = join(staging, "Applications");
    const applicationSupport = join(staging, "Application Support", "com.pattobin.personal-assistant");
    mkdirSync(payload, { recursive: true, mode: 0o700 });
    mkdirSync(applications, { recursive: true, mode: 0o700 });
    mkdirSync(applicationSupport, { recursive: true, mode: 0o700 });
    for (const [name, content] of [
      ["personal-assistant.db", "database-preservation-sentinel"],
      ["account-metadata", "account-preservation-sentinel"],
      ["rules", "rules-preservation-sentinel"],
      ["tasks", "task-preservation-sentinel"],
      ["display-settings", "display-preservation-sentinel"],
    ]) {
      writeFileSync(join(applicationSupport, name), content, { mode: 0o600 });
    }
    const before = treeDigest(applicationSupport);
    run("tar", ["-xzf", archivePath, "-C", payload]);
    const extracted = join(payload, APP_ROOT);
    if (!statSync(extracted).isDirectory()) fail("updater payload does not contain the application bundle");
    validateExtractedTree(extracted);

    const installed = join(applications, APP_ROOT);
    mkdirSync(installed, { recursive: true, mode: 0o700 });
    writeFileSync(join(installed, "old-code"), "replace-me", { mode: 0o600 });
    const retired = join(staging, "retired.app");
    renameSync(installed, retired);
    cpSync(extracted, installed, { recursive: true, dereference: false, errorOnExist: true });
    const after = treeDigest(applicationSupport);
    if (before !== after) fail("application update changed preserved user data");
    return { version, preservedDigest: after };
  } finally {
    rmSync(staging, { recursive: true, force: true });
  }
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(new URL(import.meta.url).pathname)) {
  const [archivePath, signaturePath, manifestPath, version] = process.argv.slice(2);
  if (!archivePath || !signaturePath || !manifestPath || !version) {
    fail("usage: qualify-update-preservation.mjs ARCHIVE SIGNATURE MANIFEST VERSION");
  }
  const result = qualifyUpdatePreservation(archivePath, signaturePath, manifestPath, version);
  process.stdout.write(`Update preservation qualified for ${result.version}.\n`);
}
