"""Compare speaker labelling settings over a set of synthetic consults.

Usage: python -I speaker_suite.py REFS_DIR NAME=DIR [NAME=DIR ...] [--json OUT]

REFS_DIR holds each fixture's ground truth (`<fixture>.ref.json`, as
tools/fixture-tts writes it). Each NAME=DIR is one run of the settings NAME:
DIR holds `<fixture>.jsonl` transcripts (and optionally `<fixture>.secs`, the
seconds the import took). Every transcript is scored with score_speakers.py.

Prints Markdown: for each run, the WER and the speaker accuracy per word per
group of fixtures (the end of the name, such as "tight" or "room"),
word-weighted, with how many fixtures got the right number of labels; then
the same per fixture.
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

    # Totals: a row per run, a column per group of fixtures.
    groups = [g for g in GROUPS + ["other"] if any(group_of(f) == g for f in results)] + ["all"]
    totals = {}
    for n in names:
        for g in groups:
            got = [results[f][n] for f in results if (g == "all" or group_of(f) == g) and n in results[f]]
            if not got:
                continue
            words = sum(s["words"] for s in got)
            totals.setdefault(n, {})[g] = {
                "wer": sum(s["wer"] * s["words"] for s in got) / words,
                "speaker_acc": sum(s["speaker_acc"] * s["words"] for s in got) / words,
                "count_right": sum(1 for s in got if s["labels"] == s["true_speakers"]),
                "fixtures": len(got),
                "secs": sum(s["secs"] or 0 for s in got),
            }
    print("Per group: WER / speaker per word / fixtures with the right number of labels (word-weighted).")
    print()
    print("| run | " + " | ".join(f"{g} ({sum(1 for f in results if g == 'all' or group_of(f) == g)})" for g in groups) + " | secs |")
    print("|---|" + "---|" * (len(groups) + 1))
    for n in names:
        t = totals.get(n, {})
        cells = [
            f"{t[g]['wer']:.1%} / {t[g]['speaker_acc']:.1%} / {t[g]['count_right']}" if g in t else "-" for g in groups
        ]
        secs = t.get("all", {}).get("secs", 0)
        print(f"| {n} | " + " | ".join(cells) + f" | {secs:.0f} |")

    print()
    print("Per fixture: WER / speaker per word / labels found.")
    print()
    print("| fixture | speakers | " + " | ".join(names) + " |")
    print("|---|---|" + "---|" * len(names))
    for fixture in sorted(results, key=lambda f: (GROUPS.index(group_of(f)) if group_of(f) in GROUPS else 99, f)):
        row = results[fixture]
        true = next(iter(row.values()))["true_speakers"]
        cells = []
        for n in names:
            s = row.get(n)
            cells.append(f"{s['wer']:.1%} / {s['speaker_acc']:.1%} / {s['labels']}" if s else "-")
        print(f"| {fixture} | {true} | " + " | ".join(cells) + " |")

    if out_json:
        with open(out_json, "w", encoding="utf-8") as f:
            json.dump({"fixtures": results, "totals": totals}, f, indent=1)


if __name__ == "__main__":
    main(sys.argv[1:])
