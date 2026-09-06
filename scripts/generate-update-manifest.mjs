#!/usr/bin/env node

import { readFileSync, writeFileSync } from "node:fs";
import { basename } from "node:path";

const [version, tag, archivePath, signaturePath, outputPath] = process.argv.slice(2);
if (!version || !tag || !archivePath || !signaturePath || !outputPath) {
  throw new Error("usage: generate-update-manifest.mjs VERSION TAG ARCHIVE SIGNATURE OUTPUT");
}
if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(version) || tag !== `v${version}`) {
  throw new Error("release version and tag do not match");
}
const archive = basename(archivePath);
if (archive !== `PersonalAssistant-${version}-aarch64.app.tar.gz`) {
  throw new Error("unexpected updater archive name");
}
const signature = readFileSync(signaturePath, "utf8").trim();
if (!signature || signature.length > 4096 || /[\0]/.test(signature)) {
  throw new Error("invalid updater signature");
}
const url = `https://github.com/xvsystemslimerick/personalassistant/releases/download/${tag}/${encodeURIComponent(archive)}`;
const manifest = {
  version,
  platforms: {
    "darwin-aarch64": { signature, url }
  }
};
writeFileSync(outputPath, `${JSON.stringify(manifest, null, 2)}\n`, { encoding: "utf8", mode: 0o644, flag: "wx" });
