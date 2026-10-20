"""Check the desktop self-test's results (scripts/ci/desktop-selftest.sh).

Usage: python3 check-selftest.py results.json
Exits 1 when the model didn't load, or a recording didn't play to the end
from the app's own audio URL. The blob fallback is reported, not required.
"""

import json
import sys


def main(path: str) -> int:
    with open(path, encoding="utf-8") as f:
        r = json.load(f)
    problems = []
    llm = r.get("llm")
    if llm is not None:
        print(f"- model: {llm.get('model')} ran in {llm.get('secs', 0):.1f}s; "
              f"loaded at quit: {llm.get('live_models_before_quit')}")
        if llm.get("live_models_before_quit") != 1:
            problems.append("the model wasn't loaded in the app when it quit")
    audio = r.get("audio")
    if audio is None:
        problems.append("no audio results")
    else:
        if audio.get("fatal"):
            problems.append(f"audio: {audio['fatal']}")
        print(f"- webview: {audio.get('user_agent', '?')}")
        for case in audio.get("cases", []):
            ok = case.get("ended") and not case.get("error") and \
                abs((case.get("duration") or 0) - case.get("expected_secs", 0)) < 0.5
            print(f"- {case.get('label')}: {'played to the end' if ok else 'FAILED'} "
                  f"(duration {case.get('duration')}, error {case.get('media_error')}, "
                  f"events {' '.join(case.get('events', []))})")
            if not ok and "(app URL)" in case.get("label", ""):
                problems.append(f"{case.get('label')} didn't play to the end")
        if not audio.get("cases"):
            problems.append("no recordings were played")
    for p in problems:
        print(f"FAIL: {p}")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1]))
