"""Check the desktop self-test's results (scripts/ci/desktop-selftest.sh).

Usage: python3 check-selftest.py results.json
Exits 1 when the model wasn't loaded at quit, or a recording didn't play to
the end the way the app plays it (engine.ts audioUrl): from the app's own
audio URL on macOS and Windows, and from the blob: URL on Linux, where
WebKitGTK won't play an app-defined scheme.
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
        audio = {}
    if audio.get("fatal"):
        problems.append(f"audio: {audio['fatal']}")
    if audio:
        print(f"- webview: {audio.get('user_agent', '?')}")
    played = {}
    for case in audio.get("cases", []):
        ok = bool(case.get("ended") and not case.get("error")
                  and abs((case.get("duration") or 0) - case.get("expected_secs", 0)) < 0.5)
        played[case.get("label", "")] = ok
        print(f"- {case.get('label')}: {'played to the end' if ok else 'FAILED'} "
              f"(duration {case.get('duration')}, error {case.get('media_error')}, "
              f"events {' '.join(case.get('events', []))})")
    if audio and not played:
        problems.append("no recordings were played")
    used = "(blob fallback)" if r.get("platform") == "linux" else "(app URL)"
    for label, ok in played.items():
        if label.endswith(used) and not ok:
            problems.append(f"{label}, the way the app plays it here, didn't play to the end")
    for p in problems:
        print(f"FAIL: {p}")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1]))
