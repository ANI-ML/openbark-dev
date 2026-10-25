#!/usr/bin/env python3
"""Download one pinned GGUF from packages/eval/phone-models.json and check it.

The file comes from its Hugging Face repo at the pinned revision and must
match the pinned size and SHA-256, or it is deleted and the script fails.
CI only (the phone models eval and the Nemotron check); the app downloads
through os-models.

    python3 scripts/ci/fetch-gguf.py <candidate id> <dest dir>

Prints the path of the checked file on its last line.
"""
import hashlib
import json
import os
import sys
import time
import urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
LIST = os.path.join(ROOT, "packages", "eval", "phone-models.json")


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def main():
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    want, dest_dir = sys.argv[1], sys.argv[2]
    candidates = {c["id"]: c for c in json.load(open(LIST, encoding="utf-8"))["candidates"]}
    if want not in candidates:
        sys.exit(f"no candidate {want!r} in {LIST}")
    c = candidates[want]
    os.makedirs(dest_dir, exist_ok=True)
    dest = os.path.join(dest_dir, c["file"])
    if os.path.isfile(dest) and os.path.getsize(dest) == c["size"] and sha256(dest) == c["sha256"]:
        print(f"already here and checked: {dest}", file=sys.stderr)
        print(dest)
        return
    url = f"https://huggingface.co/{c['repo']}/resolve/{c['revision']}/{c['file']}"
    part = dest + ".part"
    for attempt in range(1, 4):
        try:
            print(f"downloading {url} (attempt {attempt})", file=sys.stderr)
            with urllib.request.urlopen(url, timeout=60) as r, open(part, "wb") as out:
                while True:
                    block = r.read(1 << 20)
                    if not block:
                        break
                    out.write(block)
            break
        except OSError as e:
            print(f"download failed: {e}", file=sys.stderr)
            if attempt == 3:
                raise
            time.sleep(10 * attempt)
    size = os.path.getsize(part)
    if size != c["size"]:
        os.remove(part)
        sys.exit(f"{c['file']}: {size} bytes, expected {c['size']}")
    got = sha256(part)
    if got != c["sha256"]:
        os.remove(part)
        sys.exit(f"{c['file']}: SHA-256 {got}, expected {c['sha256']}")
    os.replace(part, dest)
    print(f"{c['file']}: {size} bytes, SHA-256 {got} (pinned)", file=sys.stderr)
    print(dest)


if __name__ == "__main__":
    main()
