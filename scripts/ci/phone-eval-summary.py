#!/usr/bin/env python3
"""Summarise the phone models eval (.github/workflows/phone-models-eval.yml).

Reads every job's folder under <results dir> (meta.json, report.json from
`openbark eval --json`, time.txt from `/usr/bin/time -v`, status.txt with the
eval's exit code, hardware.txt) and prints one Markdown table per tier group
plus a per-fixture table, for docs/eval/phone-models.md. Also writes
summary.json next to the folders.

    python3 scripts/ci/phone-eval-summary.py <results dir>
"""
import json
import os
import re
import statistics
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
LIST = os.path.join(ROOT, "packages", "eval", "phone-models.json")


def load(path, default=None):
    try:
        with open(path, encoding="utf-8") as f:
            return json.load(f)
    except (OSError, ValueError):
        return default


def peak_rss_mb(path):
    try:
        text = open(path, encoding="utf-8", errors="replace").read()
    except OSError:
        return None
    m = re.search(r"Maximum resident set size \(kbytes\):\s*(\d+)", text)
    return int(m.group(1)) / 1024 if m else None


def pct(n, d):
    return f"{100 * n / d:.0f}%" if d else "-"


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    results = sys.argv[1]
    plan = load(LIST)
    shards = plan["shards"]
    models = {c["id"]: dict(c, scores=[], rss=[], failed=[], done=set()) for c in plan["candidates"]}
    hardware = set()
    for entry in sorted(os.listdir(results)):
        d = os.path.join(results, entry)
        meta = load(os.path.join(d, "meta.json"))
        if not os.path.isdir(d) or not meta or meta.get("id") not in models:
            continue
        m = models[meta["id"]]
        report = load(os.path.join(d, "report.json"), [])
        m["scores"].extend(report)
        rss = peak_rss_mb(os.path.join(d, "time.txt"))
        if rss:
            m["rss"].append(rss)
        status = (open(os.path.join(d, "status.txt")).read().strip() if os.path.exists(os.path.join(d, "status.txt")) else "missing")
        if status != "0":
            m["failed"].append(f"shard {meta.get('shard')}: exit {status}")
        m["done"].add(meta.get("shard"))
        hw = os.path.join(d, "hardware.txt")
        if os.path.exists(hw):
            hardware.add(open(hw, encoding="utf-8", errors="replace").read().strip())

    summary = []
    for m in models.values():
        s = m["scores"]
        checks = sum(x["checks"] for x in s)
        passed = sum(x["passed"] for x in s)
        invented = sum(x["invented_numbers"] for x in s)
        chart = [x["chart"] for x in s if x.get("chart") and x["chart"]["expected"] > 0]
        secs = [x["note_secs"] for x in s if x.get("note_secs")]
        tokens = sum(x.get("completion_tokens", 0) for x in s)
        species = [x["species"] for x in s if x.get("species")]
        row = {
            "id": m["id"],
            "name": m["name"],
            "tiers": m["tiers"],
            "license": m["license"],
            "size": m["size"],
            "params_b": m["params_b"],
            "fixtures": len(s),
            "facts_passed": passed,
            "facts_checked": checks,
            "invented_numbers": invented,
            "fixtures_with_invented": [x["fixture"] for x in s if x["invented_numbers"]],
            "invented": [f"{x['fixture']}: {i}" for x in s for i in x.get("invented", [])],
            "citation_errors": sum(x["citation_errors"] for x in s),
            "word_flags": sum(len(x.get("word_flags", [])) for x in s),
            "contaminated": sum(len(x.get("contaminated", [])) for x in s),
            "species_ok": sum(1 for x in species if x["ok"]),
            "species_total": len(species),
            "chart_found": sum(c["found"] for c in chart),
            "chart_expected": sum(c["expected"] for c in chart),
            "chart_matched": sum(c["matched"] for c in chart),
            "chart_charted": sum(c["charted"] for c in chart),
            "chart_invented_teeth": sum(len(c["invented_teeth"]) for c in chart),
            "chart_wrong_values": sum(len(c["wrong_values"]) for c in chart),
            "secs_mean": statistics.mean(secs) if secs else None,
            "secs_median": statistics.median(secs) if secs else None,
            "tokens_per_sec": tokens / sum(secs) if secs and sum(secs) else None,
            "peak_rss_mb": max(m["rss"]) if m["rss"] else None,
            "shards_run": len(m["done"]),
            "shards": shards,
            "failed": m["failed"],
            "per_fixture": {
                x["fixture"]: {
                    "facts": f"{x['passed']}/{x['checks']}",
                    "invented": x["invented_numbers"],
                    "citations": x["citation_errors"],
                    "secs": round(x["note_secs"], 1),
                    "template": x.get("template", ""),
                }
                for x in s
            },
        }
        summary.append(row)

    with open(os.path.join(results, "summary.json"), "w", encoding="utf-8") as f:
        json.dump({"hardware": sorted(hardware), "models": summary}, f, indent=2)

    out = []
    out.append("Runner hardware: " + (" / ".join(sorted(hardware)) or "not recorded"))
    out.append("")
    out.append(
        "| Model | Tiers | Size | Licence | Fixtures | Facts found | Invented numbers | Citation errors | Word flags | Chart recall | Chart precision | Invented teeth | s per note (mean / median) | tokens/s | Peak RSS | Runs |"
    )
    out.append("|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|")
    for r in summary:
        runs = f"{r['shards_run']}/{r['shards']}" + (" (" + "; ".join(r["failed"]) + ")" if r["failed"] else "")
        secs = f"{r['secs_mean']:.0f} / {r['secs_median']:.0f}" if r["secs_mean"] is not None else "-"
        tps = f"{r['tokens_per_sec']:.1f}" if r["tokens_per_sec"] else "-"
        rss = f"{r['peak_rss_mb'] / 1024:.1f} GB" if r["peak_rss_mb"] else "-"
        out.append(
            f"| {r['name']} | {', '.join(str(t) for t in r['tiers'])} GB | {r['size'] / 1e9:.2f} GB | {r['license']} | {r['fixtures']} "
            f"| {r['facts_passed']}/{r['facts_checked']} ({pct(r['facts_passed'], r['facts_checked'])}) "
            f"| {r['invented_numbers']} | {r['citation_errors']} | {r['word_flags']} "
            f"| {pct(r['chart_found'], r['chart_expected'])} | {pct(r['chart_matched'], r['chart_charted'])} | {r['chart_invented_teeth']} "
            f"| {secs} | {tps} | {rss} | {runs} |"
        )
    out.append("")
    for r in summary:
        if r["invented"]:
            out.append(f"Invented numbers, {r['name']}: " + "; ".join(r["invented"]))
    print("\n".join(out))


if __name__ == "__main__":
    main()
