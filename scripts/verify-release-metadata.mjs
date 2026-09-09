#!/usr/bin/env node

import { readFileSync } from "node:fs";

const readJson = (path) => JSON.parse(readFileSync(path, "utf8"));
const cargo = readFileSync("Cargo.toml", "utf8");
const version = cargo.match(/\[workspace\.package\][\s\S]*?version\s*=\s*"([^"]+)"/)?.[1];
if (!version || !/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(version)) {
  throw new Error("workspace release version is invalid");
}

for (const path of ["package.json", "apps/desktop/package.json", "apps/family-display/package.json"]) {
  if (readJson(path).version !== version) throw new Error(`${path} version differs from Cargo workspace`);
}
const tauri = readJson("apps/desktop/src-tauri/tauri.conf.json");
if (tauri.version !== version) throw new Error("Tauri bundle version differs from Cargo workspace");
if (tauri.identifier !== "com.pattobin.personal-assistant") throw new Error("production bundle identifier changed");

const embeddedKey = readFileSync("apps/desktop/src-tauri/updater.pub", "utf8").trim();
if (!embeddedKey || tauri.plugins?.updater?.pubkey !== embeddedKey) {
  throw new Error("Tauri updater trust anchor differs from the embedded native key");
}
process.stdout.write(`Release metadata verified for ${version}.\n`);
