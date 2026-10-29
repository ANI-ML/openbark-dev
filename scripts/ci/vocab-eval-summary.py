#!/usr/bin/env python3
"""Summarise the vocabulary eval (.github/workflows/vocab-eval.yml, ADR-0031).

Reads every job's folder under <results dir> (meta.json with arm, set and
shard; report.json from `openbark eval --json`; transcripts/<id>.jsonl, the
transcripts the eval drafted from, hints included) and prints Markdown tables
for docs/eval/vocabulary.md. Also writes summary.json next to the folders.

Per arm (main, noreg, branch):
- facts found and invented numbers, per set;
- hints: how many, how many correct (the likely term was said: it is in the
  consult script for the consult set, or one of the snippet's drugs for the
  vocabulary set) and how many false; second-recogniser hints separately;
- drug recall on the vocabulary set: each drug a snippet names is either
  written right in the transcript, hinted, or missed;
- segments the second recogniser re-heard (its cost on a weak computer).

    python3 scripts/ci/vocab-eval-summary.py <results dir>
"""
import json
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SCRIPTS = {
    "main": os.path.join(ROOT, "packages", "eval", "fixtures", "scripts"),
    "vocab": os.path.join(ROOT, "packages", "eval", "fixtures", "vocab", "scripts"),
}
ARMS = ["main", "noreg", "branch"]


def load(path, default=None):
    try:
        with open(path, encoding="utf-8") as f:
            return json.load(f)
    except (OSError, ValueError):
        return default


def script_lines(path):
    """(the lines' text, lower case; the snippet's drugs) from a script, without a YAML parser."""
    text, drugs = [], []
    with open(path, encoding="utf-8") as f:
        src = f.read()
    for m in re.finditer(r'^\s*- \[\w+, "(.*)"\]\s*$', src, re.M):
        text.append(m.group(1).lower())
    d = re.search(r"^\s*drugs:\s*\[(.*)\]\s*$", src, re.M)
    if d:
        drugs = [x.strip().strip("'\"").lower() for x in d.group(1).split(",") if x.strip()]
    return " ".join(text), drugs


def scripts(set_name):
    out = {}
    folder = SCRIPTS[set_name]
    for name in sorted(os.listdir(folder)):
        if name.endswith(".yaml"):
            path = os.path.join(folder, name)
            m = re.search(r"^id:\s*(\S+)", open(path, encoding="utf-8").read(), re.M)
            if m:
                out[m.group(1)] = script_lines(path)
    return out


def words(s):
    return re.sub(r"[^a-z0-9' ]+", " ", s.lower())


def segments(path):
    out = []
    try:
        with open(path, encoding="utf-8") as f:
            for line in f:
                if line.strip():
                    out.append(json.loads(line))
    except OSError:
        pass
    return out


def main():
    results = sys.argv[1]
    said = {s: scripts(s) for s in SCRIPTS}
    stats = {}
    per_fixture = []
    for job in sorted(os.listdir(results)):
        folder = os.path.join(results, job)
        meta = load(os.path.join(folder, "meta.json"))
        if not meta:
            continue
        arm, set_name = meta["arm"], meta["set"]
        st = stats.setdefault((arm, set_name), {
            "facts": 0, "checks": 0, "invented": 0, "failed": 0, "fixtures": 0,
            "hints": 0, "correct": 0, "false": 0, "second": 0, "second_correct": 0,
            "rechecked": 0, "segments": 0, "right": 0, "hinted": 0, "missed": 0, "false_list": [],
        })
        for score in load(os.path.join(folder, "report.json"), []) or []:
            st["facts"] += score.get("passed", 0)
            st["checks"] += score.get("checks", 0)
            st["invented"] += score.get("invented_numbers", 0)
            st["failed"] += 1 if score.get("error") else 0
            st["fixtures"] += 1
            per_fixture.append((arm, set_name, score["fixture"], score.get("passed", 0), score.get("checks", 0)))
        tdir = os.path.join(folder, "transcripts")
        if not os.path.isdir(tdir):
            continue
        for name in sorted(os.listdir(tdir)):
            fixture = name[: -len(".jsonl")]
            if fixture not in said[set_name]:
                continue
            text, drugs = said[set_name][fixture]
            segs = segments(os.path.join(tdir, name))
            st["segments"] += len(segs)
            st["rechecked"] += sum(1 for s in segs if s.get("check"))
            for s in segs:
                for h in s.get("hints", []):
                    if h.get("kind") in ("number", "route", "name"):
                        continue
                    likely = [x.lower() for x in h.get("likely", [])]
                    ok = any(x in drugs for x in likely) if set_name == "vocab" else any(
                        f" {x} " in f" {words(text)} " for x in likely
                    )
                    second = h.get("kind") == "second recogniser"
                    st["hints"] += 1
                    st["second"] += 1 if second else 0
                    st["second_correct"] += 1 if second and ok else 0
                    st["correct" if ok else "false"] += 1
                    if not ok:
                        st["false_list"].append(f"{fixture}: \"{h.get('heard')}\" -> {', '.join(likely)}")
            if set_name == "vocab":
                heard = " " + words(" ".join(s.get("text", "") for s in segs)) + " "
                hinted = {x.lower() for s in segs for h in s.get("hints", []) for x in h.get("likely", [])}
                for d in drugs:
                    if f" {d} " in heard:
                        st["right"] += 1
                    elif d in hinted:
                        st["hinted"] += 1
                    else:
                        st["missed"] += 1

    lines = ["## Consult scripts (facts and hints)", ""]
    lines.append("| Arm | Fixtures | Facts found | Invented numbers | Failed drafts | Hints | Correct | False | Second-recogniser hints (correct) | Segments re-heard |")
    lines.append("|---|---|---|---|---|---|---|---|---|---|")
    for arm in ARMS:
        st = stats.get((arm, "main"))
        if not st:
            continue
        pct = 100 * st["facts"] / max(st["checks"], 1)
        lines.append(
            f"| {arm} | {st['fixtures']} | {st['facts']}/{st['checks']} ({pct:.0f}%) | {st['invented']} | {st['failed']} | "
            f"{st['hints']} | {st['correct']} | {st['false']} | {st['second']} ({st['second_correct']}) | {st['rechecked']}/{st['segments']} |"
        )
    lines += ["", "## Brand-name snippets (recall and false hints)", ""]
    lines.append("| Arm | Snippets | Drug names | Written right | Hinted | Missed | Recall | Hints | False hints | Facts found | Segments re-heard |")
    lines.append("|---|---|---|---|---|---|---|---|---|---|---|")
    for arm in ARMS:
        st = stats.get((arm, "vocab"))
        if not st:
            continue
        n = st["right"] + st["hinted"] + st["missed"]
        recall = 100 * (st["right"] + st["hinted"]) / max(n, 1)
        lines.append(
            f"| {arm} | {st['fixtures']} | {n} | {st['right']} | {st['hinted']} | {st['missed']} | {recall:.0f}% | "
            f"{st['hints']} | {st['false']} | {st['facts']}/{st['checks']} | {st['rechecked']}/{st['segments']} |"
        )
    for arm in ARMS:
        for set_name in SCRIPTS:
            st = stats.get((arm, set_name))
            if st and st["false_list"]:
                lines += ["", f"False hints, {arm}, {set_name}:", ""] + [f"- {x}" for x in st["false_list"]]
    lines += ["", "## Facts per fixture", "", "| Set | Fixture | " + " | ".join(ARMS) + " |", "|---|---|" + "---|" * len(ARMS)]
    table = {}
    for arm, set_name, fixture, p, c in per_fixture:
        table.setdefault((set_name, fixture), {})[arm] = f"{p}/{c}"
    for (set_name, fixture), row in sorted(table.items()):
        lines.append(f"| {set_name} | {fixture} | " + " | ".join(row.get(a, "") for a in ARMS) + " |")
    print("\n".join(lines))
    with open(os.path.join(results, "summary.json"), "w", encoding="utf-8") as f:
        json.dump({f"{a}/{s}": v for (a, s), v in stats.items()}, f, indent=2)


if __name__ == "__main__":
    main()
