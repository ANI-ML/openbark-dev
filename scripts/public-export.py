#!/usr/bin/env python3
"""Export a clean copy of the repository for the public source repo.

The public repo starts from one fresh commit, never this repo's history. This
script builds the tree for that commit:

  1. `git archive <ref>` into an empty folder, so only committed files are
     exported (untracked and ignored files never are);
  2. removes the reviewed exclude list and the blocked files;
  3. applies the reviewed text scrubs (exact phrases), failing if any rule no
     longer matches the expected number of times, so a changed source is
     reviewed again instead of slipping through, then re-runs the configured
     generators inside the export folder (website/dist, CHANGELOG.md) so
     generated files match the scrubbed sources;
  4. runs a final gate that fails the export if any file (name or content,
     text or binary) matches a forbidden pattern or a secret, using gitleaks
     when it is available and a built-in regex scan in every case;
  5. checks that the files a public repo needs are present and, with
     --git-init, runs `git init` and `git add -A` inside the export folder
     only, to show the file list the first commit would contain.

Every rule lives in scripts/public-export.toml (TOML, so the script needs
only the Python 3.11+ standard library). The forbidden patterns live there
too, not in this file.

The script never writes inside the repository. It refuses an output folder
inside the repository, or one that contains it, and only replaces an existing
output folder that a previous export created.

Exit codes: 0 gate passed and no blockers; 3 gate passed but blockers remain
(not ready to publish); 1 gate failed; 2 configuration or usage error,
including a scrub rule that no longer matches.

Usage:
  python scripts/public-export.py --out <folder> [--force] [--git-init]
                                  [--config scripts/public-export.toml]
                                  [--ref HEAD] [--no-gitleaks]
"""

from __future__ import annotations

import argparse
import io
import json
import os
import re
import shutil
import stat
import subprocess
import sys
import tarfile
import tempfile
import time
import tomllib
from dataclasses import dataclass, field
from pathlib import Path

EXIT_OK = 0
EXIT_GATE_FAILED = 1
EXIT_CONFIG = 2
EXIT_BLOCKED = 3


class ConfigError(Exception):
    pass


# ---------------------------------------------------------------- helpers


def glob_to_regex(pattern: str) -> re.Pattern[str]:
    """Translate a path glob to a regex. `**` spans folders, `*` does not."""
    out = []
    i = 0
    while i < len(pattern):
        c = pattern[i]
        if pattern.startswith("**/", i):
            out.append("(?:.*/)?")
            i += 3
        elif pattern.startswith("**", i):
            out.append(".*")
            i += 2
        elif c == "*":
            out.append("[^/]*")
            i += 1
        elif c == "?":
            out.append("[^/]")
            i += 1
        else:
            out.append(re.escape(c))
            i += 1
    return re.compile("^" + "".join(out) + "$")


class PathMatcher:
    def __init__(self, globs: list[str]):
        self.globs = list(globs)
        self._res = [glob_to_regex(g) for g in self.globs]

    def matches(self, rel: str) -> bool:
        return any(r.match(rel) for r in self._res)


def as_list(value, what: str) -> list[str]:
    if value is None:
        return []
    if isinstance(value, str):
        return [value]
    if isinstance(value, list) and all(isinstance(v, str) for v in value):
        return value
    raise ConfigError(f"{what} must be a string or a list of strings")


def is_text(data: bytes) -> bool:
    if b"\x00" in data:
        return False
    try:
        data.decode("utf-8")
    except UnicodeDecodeError:
        return False
    return True


def rel_files(root: Path) -> list[str]:
    files = []
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = [d for d in dirnames if not (Path(dirpath) == root and d == ".git")]
        for name in filenames:
            full = Path(dirpath, name)
            files.append(full.relative_to(root).as_posix())
    return sorted(files)


def force_rmtree(path: Path) -> None:
    def onerror(func, p, _exc):
        os.chmod(p, stat.S_IWRITE)
        func(p)

    shutil.rmtree(path, onerror=onerror)


def is_inside(child: Path, parent: Path) -> bool:
    try:
        child.relative_to(parent)
        return True
    except ValueError:
        return False


def run_git(repo: Path, *args: str, binary: bool = False):
    proc = subprocess.run(["git", "-C", str(repo), *args], capture_output=True)
    if proc.returncode != 0:
        raise ConfigError(f"git {' '.join(args)} failed: {proc.stderr.decode(errors='replace').strip()}")
    return proc.stdout if binary else proc.stdout.decode("utf-8", errors="replace")


def snippet(line: str, start: int, end: int, width: int = 60) -> str:
    a = max(0, start - width)
    b = min(len(line), end + width)
    return ("..." if a else "") + line[a:b].strip() + ("..." if b < len(line) else "")


# ------------------------------------------------------------------ config


@dataclass
class Exclude:
    reason: str
    matcher: PathMatcher
    blocker: dict | None = None
    hits: list[str] = field(default_factory=list)


@dataclass
class Scrub:
    id: str
    files: PathMatcher
    find: str
    replace: str
    regex: bool
    expect: int | None
    optional: bool
    reason: str
    hits: dict[str, int] = field(default_factory=dict)


@dataclass
class GatePattern:
    name: str
    regex: re.Pattern[str]
    bregex: re.Pattern[bytes] | None
    reason: str
    scan_paths: bool = True


@dataclass
class Allow:
    pattern: str
    files: PathMatcher
    context: re.Pattern[str]
    reason: str
    used: int = 0


def load_config(path: Path) -> dict:
    try:
        with path.open("rb") as fh:
            raw = tomllib.load(fh)
    except (OSError, tomllib.TOMLDecodeError) as exc:
        raise ConfigError(f"cannot read {path}: {exc}") from exc

    cfg: dict = {"raw": raw}
    export = raw.get("export", {})
    cfg["ref"] = export.get("ref", "HEAD")
    cfg["required_files"] = as_list(export.get("required_files"), "export.required_files")

    excludes = []
    for i, e in enumerate(raw.get("exclude", [])):
        globs = as_list(e.get("paths"), f"exclude[{i}].paths")
        if not globs or not e.get("reason"):
            raise ConfigError(f"exclude[{i}] needs paths and a reason")
        excludes.append(Exclude(reason=e["reason"], matcher=PathMatcher(globs)))
    for i, b in enumerate(raw.get("blocker", [])):
        for key in ("id", "summary", "resolve"):
            if not b.get(key):
                raise ConfigError(f"blocker[{i}] needs {key}")
        globs = as_list(b.get("paths"), f"blocker[{i}].paths")
        action = b.get("action", "keep")
        if action not in ("exclude", "keep"):
            raise ConfigError(f"blocker {b['id']}: action must be 'exclude' or 'keep'")
        if action == "exclude" and not globs:
            raise ConfigError(f"blocker {b['id']}: action 'exclude' needs paths")
        excludes.append(
            Exclude(reason=f"blocked: {b['summary']}", matcher=PathMatcher(globs), blocker=b)
            if action == "exclude"
            else Exclude(reason="", matcher=PathMatcher([]), blocker=b)
        )
    cfg["excludes"] = excludes

    scrubs = []
    seen = set()
    for i, s in enumerate(raw.get("scrub", [])):
        sid = s.get("id") or f"scrub-{i}"
        if sid in seen:
            raise ConfigError(f"duplicate scrub id {sid}")
        seen.add(sid)
        files = as_list(s.get("files"), f"scrub {sid}.files")
        if not files or "find" not in s or "replace" not in s or not s.get("reason"):
            raise ConfigError(f"scrub {sid} needs files, find, replace and a reason")
        if not s["find"]:
            raise ConfigError(f"scrub {sid}: find is empty")
        if s.get("regex"):
            try:
                re.compile(s["find"])
            except re.error as exc:
                raise ConfigError(f"scrub {sid}: bad regex: {exc}") from exc
        expect = s.get("expect")
        if expect is not None and (not isinstance(expect, int) or expect < 0):
            raise ConfigError(f"scrub {sid}: expect must be a non-negative integer")
        scrubs.append(
            Scrub(
                id=sid,
                files=PathMatcher(files),
                find=s["find"],
                replace=s["replace"],
                regex=bool(s.get("regex")),
                expect=expect,
                optional=bool(s.get("optional")),
                reason=s["reason"],
            )
        )
    cfg["scrubs"] = scrubs

    rebuilds = []
    for i, r in enumerate(raw.get("rebuild", [])):
        cmd = r.get("command")
        if not isinstance(cmd, list) or not cmd or not all(isinstance(c, str) for c in cmd) or not r.get("reason"):
            raise ConfigError(f"rebuild[{i}] needs command (a list of strings) and a reason")
        rebuilds.append({"command": cmd, "reason": r["reason"]})
    cfg["rebuilds"] = rebuilds

    gate = raw.get("gate", {})

    def patterns(key: str) -> list[GatePattern]:
        out = []
        for i, p in enumerate(gate.get(key, [])):
            if not p.get("name") or not p.get("regex") or not p.get("reason"):
                raise ConfigError(f"gate.{key}[{i}] needs name, regex and reason")
            try:
                rx = re.compile(p["regex"])
            except re.error as exc:
                raise ConfigError(f"gate.{key} {p['name']}: bad regex: {exc}") from exc
            brx = None
            if p.get("binary", True):
                try:
                    brx = re.compile(p["regex"].encode("utf-8"))
                except re.error:
                    brx = None
            out.append(GatePattern(p["name"], rx, brx, p["reason"], bool(p.get("scan_paths", True))))
        return out

    cfg["fail"] = patterns("fail")
    cfg["warn"] = patterns("warn")
    cfg["secret"] = patterns("secret")
    names = {p.name for p in cfg["fail"] + cfg["warn"] + cfg["secret"]}
    allows = []
    for i, a in enumerate(gate.get("allow", [])):
        if a.get("pattern") not in names:
            raise ConfigError(f"gate.allow[{i}] names an unknown pattern {a.get('pattern')!r}")
        if not a.get("context") or not a.get("reason"):
            raise ConfigError(f"gate.allow[{i}] needs context and reason")
        allows.append(
            Allow(
                pattern=a["pattern"],
                files=PathMatcher(as_list(a.get("files"), f"gate.allow[{i}].files") or ["**"]),
                context=re.compile(a["context"]),
                reason=a["reason"],
            )
        )
    cfg["allow"] = allows
    cfg["gitleaks"] = as_list(gate.get("gitleaks"), "gate.gitleaks")
    cfg["gitleaks_allow"] = set(as_list(gate.get("gitleaks_allow_fingerprints"), "gate.gitleaks_allow_fingerprints"))
    return cfg


# ------------------------------------------------------------------ stages


def prepare_out(out: Path, repo: Path, force: bool) -> Path:
    report_path = out.with_name(out.name + ".report.json")
    if is_inside(out, repo) or is_inside(repo, out):
        raise ConfigError(f"refusing {out}: the output folder must be outside the repository and must not contain it")
    if out.exists():
        if not out.is_dir():
            raise ConfigError(f"{out} exists and is not a folder")
        if any(out.iterdir()):
            if not force:
                raise ConfigError(f"{out} is not empty; pass --force to replace a previous export")
            if not report_path.exists():
                raise ConfigError(
                    f"refusing to delete {out}: it is not empty and has no {report_path.name} beside it, "
                    "so it does not look like a previous export"
                )
            force_rmtree(out)
    out.mkdir(parents=True, exist_ok=True)
    return report_path


def archive(repo: Path, ref: str, out: Path) -> tuple[str, int]:
    commit = run_git(repo, "rev-parse", "--verify", f"{ref}^{{commit}}").strip()
    data = run_git(repo, "archive", "--format=tar", commit, binary=True)
    count = 0
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:") as tar:
        members = [m for m in tar.getmembers() if m.isfile() or m.isdir()]
        others = [m.name for m in tar.getmembers() if not (m.isfile() or m.isdir()) and m.type != tarfile.XGLTYPE]
        if others:
            raise ConfigError(f"archive contains links or special files, not supported: {others[:5]}")
        tar.extractall(out, members=members, filter="data")
        count = sum(1 for m in members if m.isfile())
    return commit, count


def apply_excludes(out: Path, excludes: list[Exclude]) -> list[str]:
    removed = []
    for rel in rel_files(out):
        for ex in excludes:
            if ex.matcher.matches(rel):
                ex.hits.append(rel)
                (out / rel).unlink()
                removed.append(rel)
                break
    # Drop folders the excludes emptied.
    for dirpath, _dirs, _files in sorted(os.walk(out), key=lambda t: -len(t[0])):
        p = Path(dirpath)
        if p != out and not any(p.iterdir()):
            p.rmdir()
    return removed


def apply_scrubs(out: Path, scrubs: list[Scrub]) -> list[str]:
    errors = []
    files = rel_files(out)
    for s in scrubs:
        targets = [f for f in files if s.files.matches(f)]
        total = 0
        for rel in targets:
            path = out / rel
            data = path.read_bytes()
            if not is_text(data):
                # A glob may sweep over images and fonts; naming one exactly is a mistake.
                if rel in s.files.globs:
                    errors.append(f"scrub {s.id}: {rel} is not a UTF-8 text file")
                continue
            text = data.decode("utf-8")
            if s.regex:
                new, n = re.subn(s.find, s.replace, text)
            else:
                n = text.count(s.find)
                new = text.replace(s.find, s.replace)
            if n:
                s.hits[rel] = n
                total += n
                path.write_bytes(new.encode("utf-8"))
        if s.expect is not None and total != s.expect:
            errors.append(f"scrub {s.id}: expected {s.expect} match(es), found {total} ({s.reason})")
        elif s.expect is None and total == 0 and not s.optional:
            errors.append(f"scrub {s.id}: no longer matches anything; review the source and the rule ({s.reason})")
    return errors


def run_rebuilds(out: Path, rebuilds: list[dict]) -> list[str]:
    """Regenerate derived files (for example website/dist) from the scrubbed sources.

    Commands run inside the export folder, never in the repository. `{python}`
    stands for this interpreter, run isolated (-I) so nothing from the user's
    environment or the current folder is imported.
    """
    errors = []
    for r in rebuilds:
        cmd = [sys.executable if c == "{python}" else c for c in r["command"]]
        if cmd[0] == sys.executable:
            cmd.insert(1, "-I")
        proc = subprocess.run(cmd, cwd=out, capture_output=True, text=True)
        if proc.returncode != 0:
            errors.append(f"rebuild {' '.join(r['command'])} failed ({proc.returncode}): "
                          f"{(proc.stderr or proc.stdout).strip()[:400]}")
        else:
            print(f"  rebuilt: {' '.join(r['command'])}")
    return errors


@dataclass
class Finding:
    level: str
    pattern: str
    file: str
    line: int | None
    text: str


def allowed(allows: list[Allow], pattern: str, rel: str, line: str) -> bool:
    for a in allows:
        if a.pattern == pattern and a.files.matches(rel) and a.context.search(line):
            a.used += 1
            return True
    return False


def scan_tree(out: Path, cfg: dict) -> list[Finding]:
    findings: list[Finding] = []
    groups = [("fail", cfg["fail"]), ("warn", cfg["warn"]), ("secret", cfg["secret"])]
    for rel in rel_files(out):
        for level, pats in groups:
            for p in pats:
                if p.scan_paths and p.regex.search(rel) and not allowed(cfg["allow"], p.name, rel, rel):
                    findings.append(Finding(level, p.name, rel, None, f"file name: {rel}"))
        data = (out / rel).read_bytes()
        if is_text(data):
            for no, line in enumerate(data.decode("utf-8").splitlines(), 1):
                for level, pats in groups:
                    for p in pats:
                        for m in p.regex.finditer(line):
                            if allowed(cfg["allow"], p.name, rel, line):
                                break
                            text = snippet(line, m.start(), m.end())
                            if level == "secret":
                                text = f"{m.group(0)[:6]}... (redacted) in: " + snippet(line, m.start(), m.start(), 30)
                            findings.append(Finding(level, p.name, rel, no, text))
                            break
        else:
            for level, pats in groups:
                for p in pats:
                    if p.bregex is not None:
                        m = p.bregex.search(data)
                        if m and not allowed(cfg["allow"], p.name, rel, m.group(0).decode("latin-1")):
                            findings.append(Finding(level, p.name, rel, None, f"binary file, at byte {m.start()}"))
    return findings


def run_gitleaks(out: Path, candidates: list[str], allow_fp: set[str]) -> tuple[str, list[Finding]]:
    exe = next((c for c in candidates if Path(c).is_file()), None) or shutil.which("gitleaks")
    if not exe:
        return "not found (regex secret scan only)", []
    for name in (".gitleaks.toml", ".gitleaksignore"):
        if (out / name).exists():
            return f"refused: the export contains {name}, which could silence rules", [
                Finding("fail", "gitleaks", name, None, "gitleaks configuration inside the exported tree")
            ]
    with tempfile.TemporaryDirectory() as tmp:
        report = Path(tmp, "gitleaks.json")
        proc = subprocess.run(
            [exe, "dir", str(out), "--no-banner", "--redact", "--ignore-gitleaks-allow",
             "--report-format", "json", "--report-path", str(report), "--log-level", "warn"],
            cwd=tmp, capture_output=True, text=True,
        )
        if proc.returncode not in (0, 1) or not report.exists():
            return f"error (exit {proc.returncode}): {proc.stderr.strip()[:300]}", [
                Finding("fail", "gitleaks", "-", None, "gitleaks did not complete; treat as failed")
            ]
        results = json.loads(report.read_text(encoding="utf-8") or "[]")
    findings = []
    for r in results:
        if r.get("Fingerprint") in allow_fp:
            continue
        try:
            rel = Path(r.get("File", "")).resolve().relative_to(out.resolve()).as_posix()
        except ValueError:
            rel = r.get("File", "")
        findings.append(Finding("fail", f"gitleaks:{r.get('RuleID')}", rel, r.get("StartLine"),
                                f"{r.get('Description')} (fingerprint {r.get('Fingerprint')})"))
    version = subprocess.run([exe, "version"], capture_output=True, text=True).stdout.strip()
    return f"gitleaks {version}: {len(results)} finding(s)", findings


def git_init(out: Path) -> dict:
    def git(*args: str) -> str:
        proc = subprocess.run(["git", "-c", "core.autocrlf=false", "-C", str(out), *args],
                              capture_output=True, text=True)
        if proc.returncode != 0:
            raise ConfigError(f"git {' '.join(args)} in the export failed: {proc.stderr.strip()}")
        return proc.stdout

    git("init", "-q", "-b", "main")
    git("add", "-A")
    staged = [l for l in git("ls-files").splitlines() if l]
    ignored = [l[3:] for l in git("status", "--porcelain", "--ignored").splitlines() if l.startswith("!! ")]
    remotes = [l for l in git("remote").splitlines() if l]
    return {"staged": len(staged), "ignored_not_staged": ignored, "remotes": remotes}


def tree_stats(out: Path) -> dict:
    files = rel_files(out)
    sizes = {f: (out / f).stat().st_size for f in files}
    text = sum(1 for f in files if is_text((out / f).read_bytes()))
    top: dict[str, list[int]] = {}
    for f, s in sizes.items():
        key = f.split("/", 1)[0] if "/" in f else "(root)"
        top.setdefault(key, [0, 0])
        top[key][0] += 1
        top[key][1] += s
    return {
        "files": len(files),
        "text_files": text,
        "binary_files": len(files) - text,
        "bytes": sum(sizes.values()),
        "largest": sorted(((s, f) for f, s in sizes.items()), reverse=True)[:10],
        "by_top_folder": dict(sorted(top.items())),
    }


# -------------------------------------------------------------------- main


def main(argv: list[str]) -> int:
    here = Path(__file__).resolve().parent
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--out", required=True, type=Path, help="export folder (outside the repository)")
    ap.add_argument("--config", type=Path, default=here / "public-export.toml")
    ap.add_argument("--ref", help="commit to export (default: export.ref in the config, normally HEAD)")
    ap.add_argument("--force", action="store_true", help="replace a previous export in --out")
    ap.add_argument("--git-init", action="store_true", help="after a passing gate, git init + git add -A in --out")
    ap.add_argument("--no-gitleaks", action="store_true", help="skip gitleaks (the regex scan still runs)")
    args = ap.parse_args(argv)

    for stream in (sys.stdout, sys.stderr):
        try:
            stream.reconfigure(encoding="utf-8", errors="replace")
        except (AttributeError, ValueError):
            pass

    t0 = time.time()
    try:
        repo = Path(run_git(here, "rev-parse", "--show-toplevel").strip()).resolve()
        cfg = load_config(args.config)
        ref = args.ref or cfg["ref"]
        out = args.out.resolve()
        report_path = prepare_out(out, repo, args.force)
        commit, archived = archive(repo, ref, out)
    except ConfigError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return EXIT_CONFIG

    print(f"Exported {archived} files from {commit[:12]} into {out}")
    removed = apply_excludes(out, cfg["excludes"])
    print(f"Excluded {len(removed)} files")
    for ex in cfg["excludes"]:
        if ex.blocker is None and not ex.hits:
            print(f"  note: exclude rule matched nothing (kept as a guard): {', '.join(ex.matcher.globs)}")

    errors = apply_scrubs(out, cfg["scrubs"])
    applied = sum(sum(s.hits.values()) for s in cfg["scrubs"])
    print(f"Applied {len(cfg['scrubs'])} scrub rules, {applied} replacements")
    if not errors:
        errors += run_rebuilds(out, cfg["rebuilds"])

    findings = scan_tree(out, cfg)
    gitleaks_status = "skipped (--no-gitleaks)"
    if not args.no_gitleaks:
        gitleaks_status, gl = run_gitleaks(out, cfg["gitleaks"], cfg["gitleaks_allow"])
        findings += gl
    print(f"Secret scan: {gitleaks_status}; built-in regex scan ran")

    missing = [f for f in cfg["required_files"] if not (out / f).is_file()]
    for f in missing:
        findings.append(Finding("fail", "required-file", f, None, "required file is missing from the export"))

    fails = [f for f in findings if f.level in ("fail", "secret")]
    warns = [f for f in findings if f.level == "warn"]
    blockers = [ex.blocker | {"excluded": ex.hits} for ex in cfg["excludes"] if ex.blocker]
    unused_allows = [f"{a.pattern}: {a.context.pattern}" for a in cfg["allow"] if a.used == 0]

    init = None
    if not errors and not fails and args.git_init:
        try:
            init = git_init(out)
        except ConfigError as exc:
            errors.append(str(exc))

    stats = tree_stats(out)
    report = {
        "generated": time.strftime("%Y-%m-%dT%H:%M:%S%z"),
        "commit": commit,
        "out": str(out),
        "archived_files": archived,
        "excluded": [{"reason": ex.reason, "rules": ex.matcher.globs, "files": ex.hits}
                     for ex in cfg["excludes"] if ex.hits],
        "scrubs": [{"id": s.id, "replacements": s.hits} for s in cfg["scrubs"]],
        "config_errors": errors,
        "gate": [vars(f) for f in findings],
        "unused_allow_rules": unused_allows,
        "secret_scan": gitleaks_status,
        "blockers": [{k: b[k] for k in ("id", "summary", "resolve", "excluded") if k in b} for b in blockers],
        "stats": stats,
        "git_init": init,
        "seconds": round(time.time() - t0, 1),
    }
    report_path.write_text(json.dumps(report, indent=2), encoding="utf-8")

    for f in warns:
        print(f"  warn [{f.pattern}] {f.file}{':' + str(f.line) if f.line else ''}: {f.text}")
    for f in fails:
        print(f"  FAIL [{f.pattern}] {f.file}{':' + str(f.line) if f.line else ''}: {f.text}")
    for e in errors:
        print(f"  CONFIG {e}")
    for u in unused_allows:
        print(f"  note: allow rule never used: {u}")
    print(f"Tree: {stats['files']} files ({stats['text_files']} text, {stats['binary_files']} binary), "
          f"{stats['bytes'] / 1e6:.2f} MB")
    if init:
        print(f"git init: {init['staged']} files staged, {len(init['ignored_not_staged'])} ignored, "
              f"remotes: {init['remotes'] or 'none'}")
    print(f"Report: {report_path}")

    if errors:
        print("RESULT: configuration error (see CONFIG lines); the export is incomplete")
        return EXIT_CONFIG
    if fails:
        print(f"RESULT: gate FAILED with {len(fails)} finding(s)")
        return EXIT_GATE_FAILED
    if blockers:
        print(f"RESULT: gate passed, but {len(blockers)} blocker(s) remain; not ready to publish:")
        for b in blockers:
            print(f"  BLOCKER {b['id']}: {b['summary']}")
        return EXIT_BLOCKED
    print("RESULT: gate passed")
    return EXIT_OK


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
