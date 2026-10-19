#!/usr/bin/env python3
"""Generate the MLX engine pack locks (ADR-0025 decision 4).

Writes, from scripts/mlx-pack.in and scripts/mlx-pack-linux.in:

  engine/os-models/src/mlx_pack.lock.json        macOS, Apple silicon, macOS 14+
  engine/os-models/src/mlx_pack.linux.lock.json  Linux x86_64 CPU (test mode only)
  docs/mlx-pack-pins.md                          the human-readable list

Every file is pinned by URL, size and SHA-256:

  - uv's cross-platform resolver picks the versions (binary wheels only,
    Python 3.12, MACOSX_DEPLOYMENT_TARGET=14.0, and nothing released in the
    last 7 days: --exclude-newer is a week before the day this runs);
  - one wheel per package is chosen, and checked against PyPI's JSON API;
  - the standalone CPython archive is checked against its release's
    SHA256SUMS and GitHub's asset digest;
  - with --verify, everything is downloaded into a new, empty folder,
    re-hashed, and inspected (no wheel may carry a "*.data/" folder; the
    Python archive may hold no hard links and no link leaving it), and the
    unpacked sizes are recorded.

mlx-lm's supported architectures are read from the pinned wheel by parsing
its source with `ast`. Downloaded code is never imported or run.

Run it in WSL (Ubuntu 24.04) with uv 0.13.1 and Python 3.11 or later:

    python3 -I scripts/mlx-pack-lock.py --uv /path/to/uv --verify --scratch /tmp/mlx-lock-dl

The lock files are generated: never edit them by hand.
"""

from __future__ import annotations

import argparse
import ast
import datetime as dt
import hashlib
import json
import os
import re
import subprocess
import sys
import tarfile
import tempfile
import tomllib
import urllib.request
import zipfile
from pathlib import Path, PurePosixPath

UV_VERSION = "0.13.1"
PY_TAG = "cp312"
PY_MINOR = 12
PBS_REPO = "astral-sh/python-build-standalone"
PBS_RELEASE = "20261009"
PBS_PYTHON = "3.12.15"
WHEEL_HOST = "https://files.pythonhosted.org/"
USER_AGENT = "OpenBark-lock-script (https://github.com/ANI-ML/openbark)"

ROOT = Path(__file__).resolve().parent.parent

TARGETS = [
    {
        "key": "mac",
        "title": "macOS, Apple silicon (macOS 14 or later)",
        "input": "scripts/mlx-pack.in",
        "lock": "engine/os-models/src/mlx_pack.lock.json",
        "target": "aarch64-apple-darwin",
        "uv_platform": "aarch64-apple-darwin",
        "macos_min": "14.0",
        # Checked by hand on 2026-10-18 against SHA256SUMS and the GitHub digest.
        "python_expected": (25_165_569, "83dcb04777c798bd95f3687acf9ae84b09860272ee6bdfb05be8bcf9447f75b8"),
    },
    {
        "key": "linux",
        "title": "Linux x86_64, CPU only (test mode, OPENBARK_MLX_DEV=1)",
        "input": "scripts/mlx-pack-linux.in",
        "lock": "engine/os-models/src/mlx_pack.linux.lock.json",
        "target": "x86_64-unknown-linux-gnu",
        "uv_platform": "x86_64-manylinux_2_35",
        "macos_min": None,
        "python_expected": (66_913_335, "5864001bf969db329fadc2b654c4878c6027d170c6bd77e28f88666260bcaf63"),
    },
]


def fail(msg: str) -> None:
    sys.exit(f"mlx-pack-lock: {msg}")


def http_get(url: str, accept: str | None = None) -> bytes:
    if not url.startswith("https://"):
        fail(f"refusing a non-https URL: {url}")
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    if accept:
        req.add_header("Accept", accept)
    with urllib.request.urlopen(req, timeout=60) as r:
        return r.read()


def http_json(url: str) -> dict | list:
    return json.loads(http_get(url, "application/json"))


def download(url: str, dest: Path) -> tuple[int, str]:
    """Stream to dest; returns (size, sha256)."""
    if not url.startswith("https://"):
        fail(f"refusing a non-https URL: {url}")
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    h = hashlib.sha256()
    n = 0
    with urllib.request.urlopen(req, timeout=120) as r, open(dest, "wb") as f:
        while True:
            chunk = r.read(1 << 20)
            if not chunk:
                break
            h.update(chunk)
            f.write(chunk)
            n += len(chunk)
    return n, h.hexdigest()


# ---------------------------------------------------------------- wheel tags

def _py_ok(py: str, abi: str) -> bool:
    for p in py.split("."):
        if abi == PY_TAG and p == PY_TAG:
            return True
        if abi == "abi3":
            m = re.fullmatch(r"cp3(\d+)", p)
            if m and int(m.group(1)) <= PY_MINOR:
                return True
        if abi == "none" and p in ("py3", f"py3{PY_MINOR}", PY_TAG):
            return True
    return False


LEGACY_MANYLINUX = {"manylinux1": (2, 5), "manylinux2010": (2, 12), "manylinux2014": (2, 17)}


def _plat_score(plat: str, target: dict) -> tuple | None:
    """Higher is more specific. None: not usable on this target."""
    best = None
    for p in plat.split("."):
        score = None
        if p == "any":
            score = (0, (0, 0), 0)
        elif target["macos_min"]:
            m = re.fullmatch(r"macosx_(\d+)_(\d+)_(arm64|universal2)", p)
            if m:
                ver = (int(m.group(1)), int(m.group(2)))
                lim = tuple(int(x) for x in target["macos_min"].split("."))
                if ver <= lim:
                    score = (1, ver, 1 if m.group(3) == "arm64" else 0)
        else:
            m = re.fullmatch(r"manylinux_(\d+)_(\d+)_x86_64", p)
            ver = None
            if m:
                ver = (int(m.group(1)), int(m.group(2)))
            else:
                m = re.fullmatch(r"(manylinux1|manylinux2010|manylinux2014)_x86_64", p)
                if m:
                    ver = LEGACY_MANYLINUX[m.group(1)]
            if ver is not None and ver <= (2, 35):
                score = (1, ver, 1)
        if score is not None and (best is None or score > best):
            best = score
    return best


def wheel_score(file_name: str, target: dict) -> tuple | None:
    if not file_name.endswith(".whl"):
        return None
    parts = file_name[:-4].split("-")
    if len(parts) not in (5, 6):
        return None
    py, abi, plat = parts[-3], parts[-2], parts[-1]
    if abi not in ("none", "abi3", PY_TAG) or not _py_ok(py, abi):
        return None
    plat_score = _plat_score(plat, target)
    if plat_score is None:
        return None
    # Prefer the most specific platform, then cp312 over abi3 over pure Python.
    abi_rank = {PY_TAG: 2, "abi3": 1, "none": 0}[abi]
    return (plat_score, abi_rank)


# ---------------------------------------------------------------- resolution

def resolve(uv: str, target: dict, exclude_newer: str, tmp: Path) -> list[dict]:
    out = tmp / f"pylock.{target['key']}.toml"
    cmd = [
        uv, "pip", "compile", str(ROOT / target["input"]),
        "--no-config",
        "--index-url", "https://pypi.org/simple",
        "--python-platform", target["uv_platform"],
        "--python-version", "3.12",
        "--only-binary", ":all:",
        "--exclude-newer", exclude_newer,
        "--format", "pylock.toml",
        "--quiet",
        "-o", str(out),
    ]
    env = dict(os.environ)
    for k in list(env):
        if k.startswith("UV_") or k.startswith("PIP_"):
            del env[k]
    env["MACOSX_DEPLOYMENT_TARGET"] = target["macos_min"] or "14.0"
    print("+", " ".join(cmd), flush=True)
    subprocess.run(cmd, check=True, env=env)
    data = tomllib.loads(out.read_text(encoding="utf-8"))
    chosen = []
    for pkg in data["packages"]:
        cands = []
        for w in pkg.get("wheels", []):
            name = w["url"].rsplit("/", 1)[1]
            s = wheel_score(name, target)
            if s is not None:
                cands.append((s, name, w))
        if not cands:
            fail(f"{pkg['name']} {pkg['version']}: no usable wheel for {target['target']} (source builds are never allowed)")
        _, file_name, w = max(cands, key=lambda c: c[0])
        chosen.append({
            "name": pkg["name"],
            "version": pkg["version"],
            "file": file_name,
            "url": w["url"],
            "size": int(w["size"]),
            "sha256": w["hashes"]["sha256"].lower(),
            "unpacked": 0,
        })
    chosen.sort(key=lambda w: w["name"])
    return chosen


def licence_of(info: dict) -> str:
    lic = (info.get("license_expression") or "").strip()
    if not lic:
        plain = (info.get("license") or "").strip()
        if plain and len(plain) <= 80 and "\n" not in plain:
            lic = plain
    if not lic:
        classifiers = [c.split(" :: ")[-1] for c in info.get("classifiers") or [] if c.startswith("License :: ")]
        lic = ", ".join(classifiers)
    return lic or "see the package"


def check_pypi(wheel: dict, licences: dict) -> None:
    data = http_json(f"https://pypi.org/pypi/{wheel['name']}/{wheel['version']}/json")
    match = [u for u in data["urls"] if u["filename"] == wheel["file"]]
    if len(match) != 1:
        fail(f"{wheel['file']} is not listed on PyPI for {wheel['name']} {wheel['version']}")
    u = match[0]
    if u.get("yanked"):
        fail(f"{wheel['file']} is yanked on PyPI")
    if u["url"] != wheel["url"]:
        fail(f"{wheel['file']}: URL differs from PyPI ({u['url']})")
    if int(u["size"]) != wheel["size"]:
        fail(f"{wheel['file']}: size {wheel['size']} differs from PyPI ({u['size']})")
    if u["digests"]["sha256"].lower() != wheel["sha256"]:
        fail(f"{wheel['file']}: SHA-256 differs from PyPI")
    if not wheel["url"].startswith(WHEEL_HOST):
        fail(f"{wheel['file']}: not on {WHEEL_HOST}")
    licences[wheel["name"]] = licence_of(data["info"])


# ---------------------------------------------------------------- python

_sums_cache: dict[str, str] = {}
_assets_cache: list[dict] = []


def pbs_sums() -> dict[str, str]:
    if not _sums_cache:
        text = http_get(f"https://github.com/{PBS_REPO}/releases/download/{PBS_RELEASE}/SHA256SUMS").decode()
        for line in text.splitlines():
            parts = line.split()
            if len(parts) == 2:
                _sums_cache[parts[1].lstrip("*")] = parts[0].lower()
    return _sums_cache


def pbs_asset(name: str) -> dict:
    if not _assets_cache:
        rel = http_json(f"https://api.github.com/repos/{PBS_REPO}/releases/tags/{PBS_RELEASE}")
        page = 1
        while True:
            batch = http_json(f"https://api.github.com/repos/{PBS_REPO}/releases/{rel['id']}/assets?per_page=100&page={page}")
            if not batch:
                break
            _assets_cache.extend(batch)
            page += 1
            if page > 30:
                fail("too many release asset pages")
    for a in _assets_cache:
        if a["name"] == name:
            return a
    fail(f"{name} is not an asset of release {PBS_RELEASE}")
    raise AssertionError


def python_entry(target: dict) -> dict:
    file_name = f"cpython-{PBS_PYTHON}+{PBS_RELEASE}-{target['target']}-install_only.tar.gz"
    url = (f"https://github.com/{PBS_REPO}/releases/download/{PBS_RELEASE}/"
           + file_name.replace("+", "%2B"))
    sums = pbs_sums()
    if file_name not in sums:
        fail(f"{file_name} is not in SHA256SUMS")
    sha = sums[file_name]
    asset = pbs_asset(file_name)
    digest = (asset.get("digest") or "").lower()
    if digest != f"sha256:{sha}":
        fail(f"{file_name}: GitHub digest {digest!r} differs from SHA256SUMS")
    size = int(asset["size"])
    exp_size, exp_sha = target["python_expected"]
    if (size, sha) != (exp_size, exp_sha):
        fail(f"{file_name}: {size} {sha} differs from the values checked by hand ({exp_size} {exp_sha})")
    return {
        "version": PBS_PYTHON,
        "release": PBS_RELEASE,
        "file": file_name,
        "url": url,
        "size": size,
        "sha256": sha,
        "root": "python",
    }


# ---------------------------------------------------------------- verification

def unsafe_member(name: str) -> bool:
    if not name or "\\" in name or "\0" in name or ":" in name:
        return True
    if name.startswith("/"):
        return True
    parts = name.rstrip("/").split("/")
    return any(p in ("", ".", "..") for p in parts)


def inspect_wheel(path: Path) -> int:
    with zipfile.ZipFile(path) as z:
        total = 0
        for info in z.infolist():
            n = info.filename
            if unsafe_member(n):
                fail(f"{path.name}: unsafe entry {n!r}")
            if n.split("/", 1)[0].endswith(".data"):
                fail(f"{path.name}: has a .data folder ({n}); the pack unzips wheels without an installer")
            if not n.endswith("/"):
                total += info.file_size
        return total


def inspect_python(path: Path, root: str) -> int:
    total = 0
    with tarfile.open(path, "r:gz") as t:
        for m in t.getmembers():
            n = m.name
            if unsafe_member(n):
                fail(f"{path.name}: unsafe entry {n!r}")
            if PurePosixPath(n).parts[0] != root:
                fail(f"{path.name}: entry {n!r} is outside {root}/")
            if m.islnk():
                fail(f"{path.name}: hard link {n!r} (the unpacker refuses hard links)")
            if m.issym():
                target = PurePosixPath(n).parent / m.linkname
                if m.linkname.startswith("/"):
                    fail(f"{path.name}: absolute symlink {n!r} -> {m.linkname}")
                depth = 0
                for part in target.parts:
                    depth += -1 if part == ".." else (0 if part == "." else 1)
                    if depth < 1:
                        fail(f"{path.name}: symlink {n!r} leaves {root}/")
            elif m.isfile():
                total += m.size
            elif not m.isdir():
                fail(f"{path.name}: unexpected entry type for {n!r}")
    return total


def verify_all(lock: dict, folder: Path) -> None:
    folder.mkdir(parents=True, exist_ok=True)
    py = lock["python"]
    for item in [py] + lock["wheels"]:
        dest = folder / item["file"]
        if dest.exists():
            fail(f"{dest} already exists: --scratch must be a new, empty folder")
        size, sha = download(item["url"], dest)
        if size != item["size"] or sha != item["sha256"]:
            fail(f"{item['file']}: downloaded {size} bytes, SHA-256 {sha}, expected {item['size']} {item['sha256']}")
        print(f"  ok {item['file']} {size:,}", flush=True)
    py_unpacked = inspect_python(folder / py["file"], py["root"])
    total = py_unpacked
    for w in lock["wheels"]:
        w["unpacked"] = inspect_wheel(folder / w["file"])
        total += w["unpacked"]
    lock["unpacked_bytes"] = total


def architectures(wheel_path: Path) -> list[str]:
    """Model types mlx-lm can load: modules under mlx_lm/models defining
    Model and ModelArgs, plus MODEL_REMAPPING keys that point at one."""
    modules = set()
    remap: dict[str, str] = {}
    with zipfile.ZipFile(wheel_path) as z:
        for n in z.namelist():
            p = PurePosixPath(n)
            if len(p.parts) == 3 and p.parts[:2] == ("mlx_lm", "models") and p.suffix == ".py":
                tree = ast.parse(z.read(n), filename=n)
                names = set()
                for s in tree.body:
                    if isinstance(s, ast.ClassDef):
                        names.add(s.name)
                    elif isinstance(s, (ast.Import, ast.ImportFrom)):
                        names |= {a.asname or a.name for a in s.names}
                    elif isinstance(s, ast.Assign):
                        names |= {t.id for t in s.targets if isinstance(t, ast.Name)}
                if {"Model", "ModelArgs"} <= names:
                    modules.add(p.stem)
        tree = ast.parse(z.read("mlx_lm/utils.py"), filename="mlx_lm/utils.py")
        for s in tree.body:
            if isinstance(s, ast.Assign) and any(isinstance(t, ast.Name) and t.id == "MODEL_REMAPPING" for t in s.targets):
                remap = ast.literal_eval(s.value)
    if not remap:
        fail("MODEL_REMAPPING not found in mlx_lm/utils.py")
    arches = set(modules) | {k for k, v in remap.items() if v in modules}
    return sorted(arches)


def mlx_lm_wheel(lock: dict, scratch: Path | None, tmp: Path) -> Path:
    w = next((w for w in lock["wheels"] if w["name"] == "mlx-lm"), None)
    if w is None:
        fail("mlx-lm is not in the lock")
    if scratch is not None and (scratch / w["file"]).exists():
        return scratch / w["file"]
    dest = tmp / w["file"]
    size, sha = download(w["url"], dest)
    if size != w["size"] or sha != w["sha256"]:
        fail(f"{w['file']}: hash mismatch")
    return dest


# ---------------------------------------------------------------- output

def lock_text(lock: dict) -> str:
    return json.dumps(lock, indent=2, ensure_ascii=True) + "\n"


def pins_md(locks: list[tuple[dict, dict]], licences: dict, uv_version: str) -> str:
    first = locks[0][1]
    out = [
        "# MLX engine pack pins",
        "",
        "Generated by `scripts/mlx-pack-lock.py` (uv " + uv_version + ") on " + first["generated_on"]
        + ". Don't edit by hand: change `scripts/mlx-pack.in` or `scripts/mlx-pack-linux.in` and run the script again.",
        "",
        "Release cut-off (`--exclude-newer`): " + first["exclude_newer"]
        + ", a week before the day the script ran (ADR-0025 decision 4). Every file is pinned by URL, size and"
        " SHA-256 in the lock files compiled into OpenBark, checked against PyPI's JSON API, and for Python against"
        " the release's `SHA256SUMS` and GitHub's asset digest.",
        "",
        "Licences are as the packages declare them on PyPI.",
        "",
    ]
    for target, lock in locks:
        dl = lock["python"]["size"] + sum(w["size"] for w in lock["wheels"])
        out += [
            f"## {target['title']}",
            "",
            f"Lock: `{target['lock']}` (target `{lock['target']}`). {len(lock['wheels'])} wheels plus Python."
            f" Download {dl:,} bytes; unpacked {lock['unpacked_bytes']:,} bytes (before precompiling).",
            "",
            "| Package | Version | Licence | File | Size (bytes) | SHA-256 |",
            "|---|---|---|---|---:|---|",
        ]
        py = lock["python"]
        out.append(f"| CPython (python-build-standalone {py['release']}) | {py['version']} | PSF-2.0 (build scripts MPL-2.0)"
                   f" | `{py['file']}` | {py['size']:,} | `{py['sha256']}` |")
        for w in lock["wheels"]:
            lic = licences.get(w["name"], "").replace("|", "/")
            out.append(f"| {w['name']} | {w['version']} | {lic} | `{w['file']}` | {w['size']:,} | `{w['sha256']}` |")
        out.append("")
    return "\n".join(out)


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--uv", default="uv", help="path to uv " + UV_VERSION)
    ap.add_argument("--verify", action="store_true", help="download, re-hash and inspect every file")
    ap.add_argument("--scratch", type=Path, help="new, empty folder for --verify downloads")
    ap.add_argument("--today", help="YYYY-MM-DD (default: today, UTC)")
    args = ap.parse_args()

    if sys.version_info < (3, 11):
        fail("needs Python 3.11 or later")
    ver = subprocess.run([args.uv, "--version"], check=True, capture_output=True, text=True).stdout.split()
    if len(ver) < 2 or ver[1] != UV_VERSION:
        fail(f"needs uv {UV_VERSION}, found {' '.join(ver)}")
    if args.verify:
        if args.scratch is None:
            fail("--verify needs --scratch <new empty folder>")
        if args.scratch.exists() and any(args.scratch.iterdir()):
            fail(f"{args.scratch} is not empty")

    today = dt.date.fromisoformat(args.today) if args.today else dt.datetime.now(dt.timezone.utc).date()
    cutoff = today - dt.timedelta(days=7)
    exclude_newer = f"{cutoff.isoformat()}T00:00:00Z"
    licences: dict[str, str] = {}
    results = []
    with tempfile.TemporaryDirectory(prefix="mlx-lock-") as t:
        tmp = Path(t)
        for target in TARGETS:
            print(f"== {target['title']}", flush=True)
            wheels = resolve(args.uv, target, exclude_newer, tmp)
            names = [w["name"] for w in wheels]
            if len(set(names)) != len(names):
                fail("duplicate package names")
            for w in wheels:
                check_pypi(w, licences)
            lock = {
                "format": 1,
                "pack": "mlx-engine-pack",
                "target": target["target"],
                "macos_min": target["macos_min"],
                "generated_by": f"scripts/mlx-pack-lock.py, uv {UV_VERSION}",
                "generated_on": today.isoformat(),
                "exclude_newer": exclude_newer,
                "python": python_entry(target),
                "wheels": wheels,
                "mlx_lm_architectures": [],
                "unpacked_bytes": 0,
            }
            scratch = None
            if args.verify:
                scratch = args.scratch / target["key"]
                verify_all(lock, scratch)
            lock["mlx_lm_architectures"] = architectures(mlx_lm_wheel(lock, scratch, tmp))
            for w in wheels:
                print(f"  {w['name']:24} {w['version']:12} {w['file']}")
            print(f"  {len(wheels)} wheels; {len(lock['mlx_lm_architectures'])} architectures;"
                  f" unpacked {lock['unpacked_bytes']:,} bytes", flush=True)
            results.append((target, lock))

    for target, lock in results:
        (ROOT / target["lock"]).write_text(lock_text(lock), encoding="utf-8", newline="\n")
        print("wrote", target["lock"])
    (ROOT / "docs" / "mlx-pack-pins.md").write_text(pins_md(results, licences, UV_VERSION), encoding="utf-8", newline="\n")
    print("wrote docs/mlx-pack-pins.md")


if __name__ == "__main__":
    main()
