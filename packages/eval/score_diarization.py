"""Score diarized transcript segments against a fixture's ground truth.

Usage: python -I score_diarization.py transcript.jsonl fixture.ref.json

Each predicted segment is matched to the reference line it overlaps most;
predicted speakers are mapped to their majority true speaker. Reports the
number of predicted speakers and the share of speech time labelled with the
right (mapped) speaker. A simple purity score, enough for M0 tuning; a proper
DER implementation arrives with the eval harness (M5).
"""

import json
import sys
from collections import Counter, defaultdict


def overlap(a0, a1, b0, b1):
    return max(0.0, min(a1, b1) - max(a0, b0))


def main(transcript_path, ref_path):
    segs = [json.loads(l) for l in open(transcript_path, encoding="utf-8") if l.strip()]
    ref = json.load(open(ref_path, encoding="utf-8"))
    votes = defaultdict(Counter)  # predicted speaker -> true speaker -> seconds
    for s in segs:
        best = max(ref, key=lambda r: overlap(s["start"], s["end"], r["start"], r["end"]))
        votes[s["speaker"]][best["speaker"]] += s["end"] - s["start"]
    total = sum(sum(c.values()) for c in votes.values())
    correct = sum(c.most_common(1)[0][1] for c in votes.values())
    true_speakers = len({r["speaker"] for r in ref})
    print(json.dumps({
        "predicted_speakers": len(votes),
        "true_speakers": true_speakers,
        "purity": round(correct / total, 3) if total else 0.0,
        "mapping": {str(k): v.most_common(1)[0][0] for k, v in votes.items()},
    }))


if __name__ == "__main__":
    main(*sys.argv[1:3])
