#!/usr/bin/env python3
"""Summarise the phone models eval (.github/workflows/phone-models-eval.yml).

Reads every job's folder under <results dir> (meta.json, report.json from
`openbark eval --json`, time.txt from `/usr/bin/time -v`, status.txt with the
eval's exit code, hardware.txt) and prints one Markdown table per tier group
plus a per-fixture table, for docs/eval/phone-models.md. Also writes
summary.json next to the folders.

Invented numbers are shown twice: as the faithfulness check counted them
(the gate), and with three known gaps of the check on reference transcripts
set aside (`checker_gap`), each applied the same way to every model and
every flagged number listed with its verdict. The gaps were found after the
first run, when the check flagged every model, the computers' default
included.

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


TENS = {"twenty": 20, "thirty": 30, "forty": 40, "fifty": 50, "sixty": 60, "seventy": 70, "eighty": 80, "ninety": 90}
UNITS = {"one": 1, "two": 2, "three": 3, "four": 4, "five": 5, "six": 6, "seven": 7, "eight": 8, "nine": 9}
_refs = {}


def reference_text(fixture):
    """The fixture's reference transcript, lower case, one string."""
    if fixture not in _refs:
        path = os.path.join(ROOT, "packages", "eval", "fixtures", "audio", f"{fixture}.ref.json")
        lines = load(path, [])
        _refs[fixture] = " ".join(x["text"] for x in lines).lower().replace("-", " ")
    return _refs[fixture]


def checker_gap(fixture, number, note_text):
    """Which known gap of the number check explains a flagged number, if one does:
    - "hundreds": a vital said the clinical way, "one sixty" for 160;
    - "suture": the 0 of a suture size said "two oh" and written 2-0;
    - "quarter": "two and a quarter" for 2.25.
    None means the number counts as invented."""
    ref = reference_text(fixture)
    try:
        value = float(number)
    except ValueError:
        return None
    for m in re.finditer(r"\b(one|two)\s+(" + "|".join(TENS) + r")(?:\s+(" + "|".join(UNITS) + r"))?\b", ref):
        said = 100 * UNITS[m.group(1)] + TENS[m.group(2)] + (UNITS[m.group(3)] if m.group(3) else 0)
        if said == value:
            return "hundreds"
    if value == 0 and re.search(r"\b(two|three|four|five|2|3|4|5)[\s-]0\b", note_text.lower()) and re.search(
        r"\b(two|three|four|five) oh (pds|monocryl|nylon|vicryl|prolene|suture)", ref
    ):
        return "suture"
    whole, frac = divmod(value, 1)
    if abs(frac - 0.25) < 1e-9:
        names = [k for k, v in UNITS.items() if v == int(whole)]
        if any(f"{n} and a quarter" in ref for n in names):
            return "quarter"
    return None


def note_text_at(d, fixture, pet, path):
    name = f"{fixture}.{pet.lower()}.note.json" if pet else f"{fixture}.note.json"
    cur = load(os.path.join(d, "notes", name))
    try:
        for part in path.strip("/").split("/"):
            cur = cur[int(part)] if isinstance(cur, list) else cur[part]
    except (KeyError, IndexError, TypeError, ValueError):
        return ""
    return json.dumps(cur, ensure_ascii=False)


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
        for x in report:
            verdicts = []
            for inv in x.get("invented", []):
                path, number = inv.rsplit(": ", 1)
                pet = None
                if ": " in path:
                    pet, path = path.split(": ", 1)
                fixture = x["fixture"].split("/")[0]
                gap = checker_gap(fixture, number, note_text_at(d, fixture, pet, path))
                verdicts.append({"fixture": x["fixture"], "path": (f"{pet}: " if pet else "") + path, "number": number, "gap": gap})
            x["verdicts"] = verdicts
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
            "drafts_failed": [f"{x['fixture']}: {x['error']}" for x in s if x.get("error")],
            "facts_passed": passed,
            "facts_checked": checks,
            "invented_numbers": invented,
            "invented_after_gaps": sum(1 for x in s for v in x["verdicts"] if not v["gap"]),
            "verdicts": [v for x in s for v in x["verdicts"]],
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
        "| Model | Tiers | Size | Licence | Fixtures (drafts failed) | Facts found | Invented numbers (check) | Invented after the check's gaps | Citation errors | Word flags | Chart recall | Chart precision | Invented teeth | s per note (mean / median) | tokens/s | Peak RSS | Runs |"
    )
    out.append("|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|")
    for r in summary:
        runs = f"{r['shards_run']}/{r['shards']}" + (" (" + "; ".join(r["failed"]) + ")" if r["failed"] else "")
        secs = f"{r['secs_mean']:.0f} / {r['secs_median']:.0f}" if r["secs_mean"] is not None else "-"
        tps = f"{r['tokens_per_sec']:.1f}" if r["tokens_per_sec"] else "-"
        rss = f"{r['peak_rss_mb'] / 1024:.1f} GB" if r["peak_rss_mb"] else "-"
        out.append(
            f"| {r['name']} | {', '.join(str(t) for t in r['tiers'])} GB | {r['size'] / 1e9:.2f} GB | {r['license']} | {r['fixtures']} ({len(r['drafts_failed'])}) "
            f"| {r['facts_passed']}/{r['facts_checked']} ({pct(r['facts_passed'], r['facts_checked'])}) "
            f"| {r['invented_numbers']} | {r['invented_after_gaps']} | {r['citation_errors']} | {r['word_flags']} "
            f"| {pct(r['chart_found'], r['chart_expected'])} | {pct(r['chart_matched'], r['chart_charted'])} | {r['chart_invented_teeth']} "
            f"| {secs} | {tps} | {rss} | {runs} |"
        )
    out.append("")
    for r in summary:
        if r["drafts_failed"]:
            out.append(f"Drafts that failed, {r['name']}: " + "; ".join(d[:160] for d in r["drafts_failed"]))
        if r["verdicts"]:
            out.append(
                f"Flagged numbers, {r['name']}: "
                + "; ".join(f"{v['fixture']} {v['path']} {v['number']} ({v['gap'] or 'invented'})" for v in r["verdicts"])
            )
    print("\n".join(out))


if __name__ == "__main__":
    main()
