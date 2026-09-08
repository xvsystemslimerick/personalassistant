import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import test from "node:test";
import { qualifyUpdatePreservation } from "./qualify-update-preservation.mjs";

function fixture() {
  const root = mkdtempSync(join(tmpdir(), "personal-assistant-update-test-"));
  const source = join(root, "source", "Personal Assistant.app", "Contents", "MacOS");
  mkdirSync(source, { recursive: true });
  writeFileSync(join(source, "personal-assistant-desktop"), "new-code");
  const version = "1.2.3";
  const archive = join(root, `PersonalAssistant-${version}-aarch64.app.tar.gz`);
  assert.equal(spawnSync("tar", ["-czf", archive, "-C", join(root, "source"), "Personal Assistant.app"]).status, 0);
  const signature = join(root, `${archive.split("/").at(-1)}.sig`);
  writeFileSync(signature, "trusted-signature-fixture\n");
  const manifest = join(root, "latest.json");
  writeFileSync(manifest, JSON.stringify({
    version,
    platforms: {
      "darwin-aarch64": {
        signature: "trusted-signature-fixture",
        url: `https://github.com/xvsystemslimerick/personalassistant/releases/download/v${version}/PersonalAssistant-${version}-aarch64.app.tar.gz`,
      },
    },
  }));
  return { root, archive, signature, manifest, version };
}

test("qualifies an app-only update while preserving all user-state classes", () => {
  const value = fixture();
  try {
    assert.match(qualifyUpdatePreservation(value.archive, value.signature, value.manifest, value.version).preservedDigest, /^[a-f0-9]{64}$/);
  } finally {
    rmSync(value.root, { recursive: true, force: true });
  }
});

test("rejects a manifest that redirects the signed artifact", () => {
  const value = fixture();
  try {
    writeFileSync(value.manifest, JSON.stringify({
      version: value.version,
      platforms: {
        "darwin-aarch64": { signature: "trusted-signature-fixture", url: "https://example.invalid/update" },
      },
    }));
    assert.throws(
      () => qualifyUpdatePreservation(value.archive, value.signature, value.manifest, value.version),
      /does not bind the exact signed Apple Silicon artifact/,
    );
  } finally {
    rmSync(value.root, { recursive: true, force: true });
  }
});

test("rejects an archive containing data outside the application bundle", () => {
  const value = fixture();
  try {
    const outside = join(value.root, "source", "Application Support");
    mkdirSync(outside);
    writeFileSync(join(outside, "personal-assistant.db"), "must-not-ship");
    assert.equal(
      spawnSync("tar", [
        "-czf",
        value.archive,
        "-C",
        join(value.root, "source"),
        "Personal Assistant.app",
        "Application Support",
      ]).status,
      0,
    );
    assert.throws(
      () => qualifyUpdatePreservation(value.archive, value.signature, value.manifest, value.version),
      /escapes or duplicates the application bundle/,
    );
  } finally {
    rmSync(value.root, { recursive: true, force: true });
  }
});
