"""Compare speaker labelling settings over a set of synthetic consults.

Usage: python -I speaker_suite.py REFS_DIR NAME=DIR [NAME=DIR ...] [--json OUT]

REFS_DIR holds each fixture's ground truth (`<fixture>.ref.json`, as
tools/fixture-tts writes it). Each NAME=DIR is one run of the settings NAME:
DIR holds `<fixture>.jsonl` transcripts (and optionally `<fixture>.secs`, the
seconds the import took). Every transcript is scored with score_speakers.py.

Prints Markdown: per fixture, the WER and the speaker accuracy per word of
each run, the labels found against the speakers in the script; then totals
per group of fixtures (the part of the name after the script id, such as
"tight" or "room"), word-weighted.
"""

import json
import os
import sys
from collections import defaultdict

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from score_speakers import score  # noqa: E402

# Fixture name suffixes, in the order groups are reported.
GROUPS = ["tight", "room", "same", "phone", "orig"]


def group_of(name):
    for g in GROUPS:
        if name.endswith("-" + g):
            return g
    return "other"


def load(path):
    return [json.loads(l) for l in open(path, encoding="utf-8") if l.strip()]


def main(argv):
    out_json = None
    if "--json" in argv:
        i = argv.index("--json")
        out_json = argv[i + 1]
        argv = argv[:i] + argv[i + 2 :]
    refs, runs = argv[0], [a.split("=", 1) for a in argv[1:]]
    results = defaultdict(dict)  # fixture -> run -> score
    for name, folder in runs:
        for f in sorted(os.listdir(folder)):
            if not f.endswith(".jsonl"):
                continue
            fixture = f[: -len(".jsonl")]
            ref_path = os.path.join(refs, fixture + ".ref.json")
            if not os.path.exists(ref_path):
                continue
            s = score(load(os.path.join(folder, f)), json.load(open(ref_path, encoding="utf-8")))
            secs = os.path.join(folder, fixture + ".secs")
            s["secs"] = float(open(secs).read().strip()) if os.path.exists(secs) else None
            results[fixture][name] = s
    names = [n for n, _ in runs]

    print("| fixture | speakers | " + " | ".join(f"{n}: WER / speaker per word / labels" for n in names) + " |")
    print("|---|---|" + "---|" * len(names))
    for fixture in sorted(results, key=lambda f: (GROUPS.index(group_of(f)) if group_of(f) in GROUPS else 99, f)):
        row = results[fixture]
        true = next(iter(row.values()))["true_speakers"]
        cells = []
        for n in names:
            s = row.get(n)
            cells.append(f"{s['wer']:.1%} / {s['speaker_acc']:.1%} / {s['labels']}" if s else "-")
        print(f"| {fixture} | {true} | " + " | ".join(cells) + " |")

    print()
    print("| group | fixtures | " + " | ".join(f"{n}: WER / speaker per word / right speaker count / secs" for n in names) + " |")
    print("|---|---|" + "---|" * len(names))
    totals = {}
    for g in GROUPS + ["other", "all"]:
        fixtures = [f for f in results if g == "all" or group_of(f) == g]
        if not fixtures:
            continue
        cells = []
        for n in names:
            got = [results[f][n] for f in fixtures if n in results[f]]
            if not got:
                cells.append("-")
                continue
            words = sum(s["words"] for s in got)
            wer = sum(s["wer"] * s["words"] for s in got) / words
            acc = sum(s["speaker_acc"] * s["words"] for s in got) / words
            count = sum(1 for s in got if s["labels"] == s["true_speakers"])
            secs = sum(s["secs"] or 0 for s in got)
            totals.setdefault(n, {})[g] = {"wer": wer, "speaker_acc": acc, "count_right": count, "fixtures": len(got)}
            cells.append(f"{wer:.1%} / {acc:.1%} / {count} of {len(got)} / {secs:.0f}")
        print(f"| {g} | {len(fixtures)} | " + " | ".join(cells) + " |")

    if out_json:
        with open(out_json, "w", encoding="utf-8") as f:
            json.dump({"fixtures": results, "totals": totals}, f, indent=1)


if __name__ == "__main__":
    main(sys.argv[1:])
