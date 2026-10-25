#!/usr/bin/env python3
"""Fetch the speech models the iPhone app ships inside itself (ADR-0018
section 10): voice detection, Parakeet and the two speaker models, about
0.7 GB.

Every file comes from the URL engine/os-models/catalog.json pins (the Hugging
Face revision, or the release asset) and must match the catalog's size and
SHA-256, or the script fails. Files already in place and correct are kept,
so a cached folder is only checked. The layout is the model store's,
`<dest>/<model id>/<file>`, which is what the app looks for in
`<app>/assets/models` (tauri.ios.conf.json, bundle > resources).

    python3 scripts/ci/ios-models.py [dest]   (default: build/ios-models)

(Not under target/, so the cargo caches never carry the models.)

Run it before every iOS build: Tauri refuses to build when a resource is
missing.
"""
import hashlib
import json
import os
import sys
import time
import urllib.parse
import urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
CATALOG = os.path.join(ROOT, "engine", "os-models", "catalog.json")
# The models the iPhone needs to record and transcribe (os_engine::models
# required_status_for on iOS). Keep in step with ios-smoke.sh.
BUNDLED = ["silero-vad", "parakeet-tdt-0.6b-v3-int8", "pyannote-segmentation-3.0", "wespeaker-en-resnet34-lm"]


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def url_for(model, f):
    src = model["source"]
    if src["kind"] == "huggingface":
        path = urllib.parse.quote(f["path"], safe="/-._~")
        return f"https://huggingface.co/{src['repo']}/resolve/{src['revision']}/{path}"
    if src["kind"] == "url":
        return f["url"]
    sys.exit(f"{model['id']}: can't bundle a model from {src['kind']}")


def fetch(url, dest):
    part = dest + ".part"
    for attempt in range(1, 4):
        try:
            with urllib.request.urlopen(url, timeout=60) as r, open(part, "wb") as out:
                while True:
                    block = r.read(1 << 20)
                    if not block:
                        break
                    out.write(block)
            os.replace(part, dest)
            return
        except OSError as e:
            print(f"  {url}: {e} (attempt {attempt})", file=sys.stderr)
            if attempt == 3:
                raise
            time.sleep(10 * attempt)


def main():
    dest_root = sys.argv[1] if len(sys.argv) > 1 else os.path.join(ROOT, "build", "ios-models")
    catalog = {m["id"]: m for m in json.load(open(CATALOG, encoding="utf-8"))["models"]}
    total = 0
    expected = set()
    for mid in BUNDLED:
        model = catalog[mid]
        assert model.get("mobile"), f"{mid} isn't marked mobile in the catalog"
        for f in model["files"]:
            rel = f["path"]
            assert rel and ".." not in rel.split("/") and not rel.startswith("/"), rel
            dest = os.path.join(dest_root, mid, *rel.split("/"))
            os.makedirs(os.path.dirname(dest), exist_ok=True)
            if not (os.path.isfile(dest) and os.path.getsize(dest) == f["size"] and sha256(dest) == f["sha256"]):
                print(f"{mid}/{rel}: downloading {f['size']:,} bytes", file=sys.stderr)
                fetch(url_for(model, f), dest)
            size, digest = os.path.getsize(dest), sha256(dest)
            if size != f["size"] or digest != f["sha256"]:
                os.remove(dest)
                sys.exit(f"{mid}/{rel}: {size} bytes, SHA-256 {digest}; the catalog pins {f['size']} and {f['sha256']}")
            print(f"{mid}/{rel}: {size:,} bytes, SHA-256 matches the catalog")
            total += size
            expected.add(os.path.normpath(dest))
    # Nothing else may ride along into the app.
    for folder, _, files in os.walk(dest_root):
        for name in files:
            path = os.path.normpath(os.path.join(folder, name))
            if path not in expected:
                sys.exit(f"unexpected {path}: only the catalog files of {BUNDLED} are bundled")
    print(f"bundled speech models: {total:,} bytes in {dest_root}")


if __name__ == "__main__":
    main()
