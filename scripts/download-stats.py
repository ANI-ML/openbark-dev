#!/usr/bin/env python3
"""Record OpenBark installer downloads, per version and platform.

GitHub counts every download of a release file, but keeps only a running
total. This script takes a daily snapshot of those totals and writes:

  <out>/downloads.csv   one row per file per day: date,tag,file,platform,format,count
  <out>/README.md       the summary people read: totals, last 7 and 30 days,
                        per version, and new downloads per day

It reads the public release list of ANI-ML/openbark-releases from the GitHub
API and nothing else. The app is never involved: OpenBark itself has no
telemetry (docs/network.md). Run daily by .github/workflows/download-stats.yml,
which commits the output to the private `stats` branch.

Usage: python scripts/download-stats.py <out-dir> [--date YYYY-MM-DD]
"""

from __future__ import annotations

import csv
import datetime as dt
import json
import os
import sys
import urllib.request
from collections import defaultdict
from pathlib import Path

REPO = "ANI-ML/openbark-releases"
PLATFORMS = ("Windows", "macOS", "Linux")

# File name suffix -> (platform, format). Anything else (checksums,
# signatures) isn't an installer and isn't counted.
FORMATS = [
    (".exe", "Windows", "Setup .exe"),
    (".msi", "Windows", ".msi"),
    (".dmg", "macOS", ".dmg"),
    (".deb", "Linux", ".deb"),
    (".rpm", "Linux", ".rpm"),
    (".appimage", "Linux", "AppImage"),
]


def classify(name: str) -> tuple[str, str] | None:
    lower = name.lower()
    for suffix, platform, fmt in FORMATS:
        if lower.endswith(suffix):
            return platform, fmt
    return None


def fetch_releases() -> list[dict]:
    releases, page = [], 1
    token = os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN")
    while True:
        req = urllib.request.Request(
            f"https://api.github.com/repos/{REPO}/releases?per_page=100&page={page}",
            headers={"Accept": "application/vnd.github+json", "User-Agent": "openbark-download-stats"},
        )
        if token:
            req.add_header("Authorization", f"Bearer {token}")
        with urllib.request.urlopen(req, timeout=30) as res:
            batch = json.load(res)
        releases.extend(r for r in batch if not r.get("draft"))
        if len(batch) < 100:
            return releases
        page += 1


def snapshot(releases: list[dict], date: str) -> list[dict]:
    rows = []
    for r in releases:
        for a in r.get("assets", []):
            kind = classify(a["name"])
            if kind:
                rows.append(
                    {"date": date, "tag": r["tag_name"], "file": a["name"],
                     "platform": kind[0], "format": kind[1], "count": int(a["download_count"])}
                )
    return rows


def load(path: Path) -> list[dict]:
    if not path.exists():
        return []
    with path.open(newline="", encoding="utf-8") as f:
        return [{**r, "count": int(r["count"])} for r in csv.DictReader(f)]


def save(path: Path, rows: list[dict]) -> None:
    rows = sorted(rows, key=lambda r: (r["date"], r["tag"], r["file"]))
    with path.open("w", newline="", encoding="utf-8") as f:
        w = csv.DictWriter(f, fieldnames=["date", "tag", "file", "platform", "format", "count"], lineterminator="\n")
        w.writeheader()
        w.writerows(rows)


def totals_on(rows: list[dict], date: str) -> dict[tuple[str, str], int]:
    """Each file's running total as of `date` (its latest snapshot on or before it)."""
    latest: dict[tuple[str, str], tuple[str, int]] = {}
    for r in rows:
        if r["date"] <= date:
            key = (r["tag"], r["file"])
            if key not in latest or r["date"] > latest[key][0]:
                latest[key] = (r["date"], r["count"])
    return {k: v[1] for k, v in latest.items()}


def render(rows: list[dict], today: str) -> str:
    meta = {(r["tag"], r["file"]): (r["platform"], r["format"]) for r in rows}
    dates = sorted({r["date"] for r in rows})
    first = dates[0]
    now = totals_on(rows, today)

    def by(totals: dict, keyfn) -> dict:
        out: dict = defaultdict(int)
        for k, n in totals.items():
            out[keyfn(k)] += n
        return out

    def since(days: int) -> dict[str, int] | None:
        """Downloads per platform in the last `days` days; None until there is that much history."""
        start = (dt.date.fromisoformat(today) - dt.timedelta(days=days)).isoformat()
        if start < first:
            return None
        then = totals_on(rows, start)
        diff = {k: n - then.get(k, 0) for k, n in now.items()}
        return by(diff, lambda k: meta[k][0])

    plat = by(now, lambda k: meta[k][0])
    d7, d30 = since(7), since(30)
    fmt_n = lambda n: "–" if n is None else f"{n:,}"
    total = sum(plat.values())

    out = [
        "# OpenBark downloads (internal)",
        "",
        f"Updated {today}. Installer downloads from [{REPO}](https://github.com/{REPO}/releases), "
        "which is where every download button on www.openbark.io points.",
        "",
        "## By platform",
        "",
        "| Platform | All time | Share | Last 7 days | Last 30 days |",
        "|---|---:|---:|---:|---:|",
    ]
    for p in PLATFORMS:
        share = f"{plat.get(p, 0) / total:.0%}" if total else "–"
        out.append(f"| {p} | {plat.get(p, 0):,} | {share} | {fmt_n(d7 and d7.get(p, 0))} | {fmt_n(d30 and d30.get(p, 0))} |")
    s7 = None if d7 is None else sum(d7.values())
    s30 = None if d30 is None else sum(d30.values())
    out.append(f"| **Total** | **{total:,}** | | **{fmt_n(s7)}** | **{fmt_n(s30)}** |")

    out += ["", "## By version", "", "| Version | Windows | macOS | Linux | Total |", "|---|---:|---:|---:|---:|"]
    by_tag = defaultdict(lambda: defaultdict(int))
    for (tag, file), n in now.items():
        by_tag[tag][meta[(tag, file)][0]] += n
    for tag in sorted(by_tag, key=lambda t: [int(x) if x.isdigit() else x for x in t.lstrip("v").replace("-", ".").split(".")], reverse=True):
        c = by_tag[tag]
        out.append(f"| {tag} | {c['Windows']:,} | {c['macOS']:,} | {c['Linux']:,} | {sum(c.values()):,} |")

    out += ["", "## Linux formats", "", "| Format | All time |", "|---|---:|"]
    for fmt, n in sorted(by(now, lambda k: meta[k][1]).items()):
        if fmt in (".deb", ".rpm", "AppImage"):
            out.append(f"| {fmt} | {n:,} |")

    out += ["", "## New downloads per day (last 30 days)", ""]
    recent = [d for d in dates if d > (dt.date.fromisoformat(today) - dt.timedelta(days=30)).isoformat()]
    if len(dates) < 2:
        out.append("Daily numbers start after the second snapshot.")
    else:
        out += ["| Date | Windows | macOS | Linux | Total |", "|---|---:|---:|---:|---:|"]
        for d in reversed(recent):
            i = dates.index(d)
            if i == 0:
                continue
            prev = totals_on(rows, dates[i - 1])
            cur = totals_on(rows, d)
            diff = by({k: n - prev.get(k, 0) for k, n in cur.items()}, lambda k: meta[k][0])
            out.append(f"| {d} | {diff['Windows']:,} | {diff['macOS']:,} | {diff['Linux']:,} | {sum(diff.values()):,} |")

    out += [
        "",
        "## How to read this",
        "",
        "- GitHub counts each completed download of a file, so the same person downloading twice counts twice, and automated checks count too.",
        f"- Counting started on {first}. The totals on that day include a handful of the team's own test downloads of v0.1.0.",
        "- Downloads aren't installs, and the app reports nothing back: OpenBark has no telemetry.",
        "- Raw daily snapshots are in `downloads.csv`. Written by `scripts/download-stats.py` on `main`.",
        "",
    ]
    return "\n".join(out)


def main() -> int:
    args = sys.argv[1:]
    if not args:
        print(__doc__)
        return 2
    out = Path(args[0])
    today = args[args.index("--date") + 1] if "--date" in args else dt.datetime.now(dt.timezone.utc).date().isoformat()
    out.mkdir(parents=True, exist_ok=True)
    csv_path = out / "downloads.csv"
    rows = [r for r in load(csv_path) if r["date"] != today] + snapshot(fetch_releases(), today)
    save(csv_path, rows)
    (out / "README.md").write_text(render(rows, today), encoding="utf-8")
    # The stats branch is data, not a site: never let Vercel build it.
    (out / "vercel.json").write_text('{\n  "git": { "deploymentEnabled": false }\n}\n', encoding="utf-8")
    print(f"{today}: {sum(r['count'] for r in rows if r['date'] == today)} downloads recorded across "
          f"{len({r['tag'] for r in rows})} release(s)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
