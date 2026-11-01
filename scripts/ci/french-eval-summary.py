"""Notes drafted in the French eval (.github/workflows/french-eval.yml), per arm.

Usage: python -I french-eval-summary.py RESULTS_DIR [--json OUT]

RESULTS_DIR holds one folder per eval job (`french-eval-notes-<arm>-<shard>`),
each with `meta.json` ({"arm", "shard"}) and `report.json` (what
`openbark eval --json` wrote). An arm is where the transcript came from:
`reference` (the script itself, no recognition errors) or a recogniser.

Prints Markdown: per arm, facts found (the checklist in each script's
`expected:`), invented numbers (numbers in the note that aren't in the
transcript), failed drafts (scored as every fact missed) and statements the
word check flagged; for all fixtures, then the French ones and the consult
that mixes French and English apart; then facts found per fixture.
"""

import json
import os
import sys
from collections import defaultdict


def main(argv):
    out_json = None
    if "--json" in argv:
        i = argv.index("--json")
        out_json = argv[i + 1]
        argv = argv[:i] + argv[i + 2 :]
    root = argv[0]
    rows = defaultdict(list)  # arm -> scores
    for d in sorted(os.listdir(root)):
        meta_path = os.path.join(root, d, "meta.json")
        report_path = os.path.join(root, d, "report.json")
        if not os.path.exists(meta_path):
            continue
        meta = json.load(open(meta_path, encoding="utf-8"))
        if os.path.exists(report_path):
            rows[meta["arm"]].extend(json.load(open(report_path, encoding="utf-8")))
        else:
            rows[meta["arm"]]  # an arm whose job left no report still shows
    arms = sorted(rows, key=lambda a: (a != "reference", a))
    lines = []
    summary = {}
    for title, keep in [
        ("all fixtures", lambda s: True),
        ("French only", lambda s: s.get("category") == "fr"),
        ("French and English mixed", lambda s: s.get("category") == "fr-en"),
    ]:
        lines.append(f"**Notes, {title}**")
        lines.append("")
        lines.append("| transcript | drafts | facts found | invented numbers | failed drafts | flagged statements | note s (mean) |")
        lines.append("|---|---|---|---|---|---|---|")
        for arm in arms:
            of = [s for s in rows[arm] if keep(s)]
            passed = sum(s["passed"] for s in of)
            checks = sum(s["checks"] for s in of)
            invented = sum(s["invented_numbers"] for s in of)
            failed = sum(1 for s in of if s.get("error"))
            flags = sum(len(s.get("word_flags", [])) for s in of)
            secs = [s["note_secs"] for s in of if not s.get("error")]
            mean = sum(secs) / len(secs) if secs else None
            share = f"{passed}/{checks} ({100 * passed / checks:.0f}%)" if checks else "-"
            lines.append(
                f"| {arm} | {len(of)} | {share} | {invented} | {failed} | {flags} | "
                f"{'-' if mean is None else f'{mean:.0f}'} |"
            )
            summary.setdefault(arm, {})[title] = {
                "drafts": len(of),
                "passed": passed,
                "checks": checks,
                "invented_numbers": invented,
                "failed_drafts": failed,
                "flagged_statements": flags,
                "mean_note_secs": mean,
            }
        lines.append("")
    fixtures = sorted({s["fixture"] for arm in arms for s in rows[arm]})
    lines.append("**Facts found per fixture**")
    lines.append("")
    lines.append("| fixture | " + " | ".join(arms) + " |")
    lines.append("|---|" + "---|" * len(arms))
    for fx in fixtures:
        cells = []
        for arm in arms:
            s = next((s for s in rows[arm] if s["fixture"] == fx), None)
            if s is None:
                cells.append("-")
            elif s.get("error"):
                cells.append("failed")
            else:
                cells.append(f"{s['passed']}/{s['checks']}, {s['invented_numbers']} invented")
        lines.append(f"| {fx} | " + " | ".join(cells) + " |")
    lines.append("")
    misses = []
    for arm in arms:
        for s in rows[arm]:
            if s.get("misses"):
                misses.append(f"- {arm} {s['fixture']}: " + "; ".join(s["misses"]))
    if misses:
        lines.append("<details><summary>Missed facts</summary>")
        lines.append("")
        lines.extend(misses)
        lines.append("")
        lines.append("</details>")
    print("\n".join(lines))
    if out_json:
        json.dump(summary, open(out_json, "w", encoding="utf-8"), indent=2, ensure_ascii=False)


if __name__ == "__main__":
    main(sys.argv[1:])
