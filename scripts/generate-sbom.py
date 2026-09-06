#!/usr/bin/env python3
"""Generate a deterministic CycloneDX SBOM from locked Rust/npm metadata."""

import hashlib
import json
import pathlib
import subprocess
import sys
import uuid
from urllib.parse import quote

ROOT = pathlib.Path(__file__).resolve().parent.parent


def component(name: str, version: str, ecosystem: str) -> dict:
    purl = f"pkg:{ecosystem}/{quote(name, safe='/')}@{quote(version, safe='')}"
    return {"type": "library", "name": name, "version": version, "purl": purl, "bom-ref": purl}


def main() -> int:
    cargo = json.loads(subprocess.check_output(["cargo", "metadata", "--locked", "--format-version", "1"], cwd=ROOT))
    rust = [component(item["name"], item["version"], "cargo") for item in cargo["packages"]]
    lock = json.loads((ROOT / "package-lock.json").read_text(encoding="utf-8"))
    npm = []
    for path, item in lock.get("packages", {}).items():
        if not path.startswith("node_modules/") or not item.get("version"):
            continue
        npm.append(component(path.rsplit("node_modules/", 1)[-1], item["version"], "npm"))
    runtime = ROOT / "apps/desktop/src-tauri/resources/llama-runtime/RUNTIME.json"
    runtime_digest = hashlib.sha256(runtime.read_bytes()).hexdigest()
    components = sorted(rust + npm, key=lambda value: value["bom-ref"])
    serial_seed = "\n".join(value["bom-ref"] for value in components) + runtime_digest
    document = {
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "serialNumber": f"urn:uuid:{uuid.uuid5(uuid.NAMESPACE_URL, serial_seed)}",
        "version": 1,
        "metadata": {"component": {"type": "application", "name": "personal-assistant", "version": "0.1.0"}},
        "components": components,
        "properties": [{"name": "personal-assistant:llama-runtime-manifest-sha256", "value": runtime_digest}],
    }
    output = pathlib.Path(sys.argv[1]) if len(sys.argv) == 2 else ROOT / "outputs/personal-assistant.cdx.json"
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(document, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
