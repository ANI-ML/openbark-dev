#!/usr/bin/env python3
"""Fetch sherpa-onnx's iOS library for the phone build (ADR-0018 sections 1
and 10): the official k2-fsa v1.13.8 xcframework, the same version as the
`sherpa-onnx` crate, pinned by URL, size and SHA-256 (docs/versions.md).

It puts the archive where the crate's build script reads it
(SHERPA_ONNX_ARCHIVE_DIR, so the build downloads nothing itself) and unpacks
SherpaOnnxC.xcframework into the Xcode project (gen/apple/Frameworks), which
links and embeds it (project.yml). Without that link the Rust library's
sherpa-onnx calls have nothing to resolve against.

    python3 scripts/ci/ios-sherpa.py [archive dir]   (default: build/sherpa-onnx-archive)

Prints the archive dir on its last line.
"""
import hashlib
import os
import shutil
import sys
import time
import urllib.request
import zipfile

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
NAME = "sherpa-onnx-v1.13.8-ios-shared-onnxruntime-static.xcframework.zip"
URL = f"https://github.com/k2-fsa/sherpa-onnx/releases/download/xcframework/{NAME}"
SIZE = 31858229
SHA256 = "e259a7d3b38ad7dec49bb078252a30bb42ede8355e2bb130cf8c1c78ed131f75"
FRAMEWORKS = os.path.join(ROOT, "apps", "desktop", "src-tauri", "gen", "apple", "Frameworks")


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def main():
    archive_dir = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else os.path.join(ROOT, "build", "sherpa-onnx-archive"))
    os.makedirs(archive_dir, exist_ok=True)
    archive = os.path.join(archive_dir, NAME)
    if not (os.path.isfile(archive) and os.path.getsize(archive) == SIZE and sha256(archive) == SHA256):
        part = archive + ".part"
        for attempt in range(1, 4):
            try:
                with urllib.request.urlopen(URL, timeout=60) as r, open(part, "wb") as out:
                    shutil.copyfileobj(r, out, 1 << 20)
                break
            except OSError as e:
                print(f"{URL}: {e} (attempt {attempt})", file=sys.stderr)
                if attempt == 3:
                    raise
                time.sleep(10 * attempt)
        size, digest = os.path.getsize(part), sha256(part)
        if size != SIZE or digest != SHA256:
            os.remove(part)
            sys.exit(f"{NAME}: {size} bytes, SHA-256 {digest}; pinned {SIZE} and {SHA256}")
        os.replace(part, archive)
    print(f"{NAME}: {SIZE:,} bytes, SHA-256 matches the pin", file=sys.stderr)

    dest = os.path.join(FRAMEWORKS, "SherpaOnnxC.xcframework")
    if os.path.isdir(dest):
        shutil.rmtree(dest)
    os.makedirs(FRAMEWORKS, exist_ok=True)
    with zipfile.ZipFile(archive) as z:
        members = [m for m in z.namelist() if m.startswith("SherpaOnnxC.xcframework/")]
        if not members:
            sys.exit(f"{NAME} holds no SherpaOnnxC.xcframework")
        for m in members:
            if ".." in m.split("/"):
                sys.exit(f"unsafe path in {NAME}: {m}")
        z.extractall(FRAMEWORKS, members)
    # zipfile drops the executable bit and symlinks; the framework binaries
    # are plain files, so only the mode needs restoring.
    for d, _, files in os.walk(dest):
        for f in files:
            os.chmod(os.path.join(d, f), 0o755 if f == "SherpaOnnxC" else 0o644)
    print(f"SherpaOnnxC.xcframework -> {dest}", file=sys.stderr)
    print(archive_dir)


if __name__ == "__main__":
    main()
