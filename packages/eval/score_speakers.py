"""Per-speaker capture of a transcript against a fixture's ground truth.

Usage: python -I score_speakers.py transcript.jsonl fixture.ref.json [--json]

Written for the speakerphone fixtures (a client on a phone on speaker, much
quieter than the vet), where one speaker can be lost while the other is fine.
For each script speaker it reports:

- wer: word error rate of that speaker's words. The whole transcript is
  aligned to the whole script (word level, edit distance), so a word lost at
  a segment boundary counts against the speaker who said it. Insertions count
  against the speaker of the nearest script word before them.
- covered: share of that speaker's speech time inside a transcript segment.
  Low coverage with a high WER means voice detection dropped the speech;
  high coverage with a high WER means the recogniser misheard it.
- segments per line: how many transcript segments fell mainly in that
  speaker's lines, per script line (above 1: turns cut into fragments), and
  the median segment length in words.
- labelled right: share of that speaker's transcript words whose segment
  carries the speaker label that the majority of the speaker's speech got,
  with each label mapped to one script speaker (so two speakers sharing one
  label can't both score). Plus the number of labels found.

Numbers are compared as words ("4.8" and "four point eight" alike).
"""

import json
import re
import sys
from collections import Counter, defaultdict

ONES = "zero one two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen seventeen eighteen nineteen".split()
TENS = "_ _ twenty thirty forty fifty sixty seventy eighty ninety".split()


def int_words(n):
    if n < 20:
        return [ONES[n]]
    if n < 100:
        return [TENS[n // 10]] + ([ONES[n % 10]] if n % 10 else [])
    if n < 1000:
        rest = int_words(n % 100) if n % 100 else []
        return [ONES[n // 100], "hundred"] + (["and"] + rest if rest else [])
    if n < 1_000_000:
        rest = int_words(n % 1000) if n % 1000 else []
        return int_words(n // 1000) + ["thousand"] + rest
    return [str(n)]


def number_words(tok):
    m = re.fullmatch(r"(\d+)(?:\.(\d+))?", tok)
    if not m:
        return [tok]
    out = int_words(int(m.group(1)))
    if m.group(2):
        out += ["point"] + [ONES[int(d)] for d in m.group(2)]
    return out


def words(text):
    text = text.lower().replace("%", " percent ").replace("-", " ")
    toks = re.findall(r"[a-z']+|\d+(?:\.\d+)?", text)
    out = []
    for t in toks:
        out.extend(number_words(t.strip("'")))
    # "and" is said inside numbers but often not written ("three hundred ten").
    return [w for w in out if w and w != "and"]


def align(ref, hyp):
    """Edit-distance alignment; returns a list of (op, ref_index, hyp_index)."""
    n, m = len(ref), len(hyp)
    d = [[0] * (m + 1) for _ in range(n + 1)]
    for i in range(n + 1):
        d[i][0] = i
    for j in range(m + 1):
        d[0][j] = j
    for i in range(1, n + 1):
        ri = ref[i - 1]
        row, prev = d[i], d[i - 1]
        for j in range(1, m + 1):
            row[j] = min(prev[j - 1] + (ri != hyp[j - 1]), prev[j] + 1, row[j - 1] + 1)
    ops, i, j = [], n, m
    while i > 0 or j > 0:
        if i > 0 and j > 0 and d[i][j] == d[i - 1][j - 1] + (ref[i - 1] != hyp[j - 1]):
            ops.append(("ok" if ref[i - 1] == hyp[j - 1] else "sub", i - 1, j - 1))
            i, j = i - 1, j - 1
        elif i > 0 and d[i][j] == d[i - 1][j] + 1:
            ops.append(("del", i - 1, None))
            i -= 1
        else:
            ops.append(("ins", i - 1 if i > 0 else None, j - 1))
            j -= 1
    return ops[::-1]


def overlap(a0, a1, b0, b1):
    return max(0.0, min(a1, b1) - max(a0, b0))


def score(segs, ref):
    roles = sorted({r["speaker"] for r in ref})
    ref_words, ref_role = [], []
    for r in ref:
        w = words(r["text"])
        ref_words += w
        ref_role += [r["speaker"]] * len(w)
    segs = sorted(segs, key=lambda s: s["start"])
    hyp_words, hyp_seg = [], []
    for k, s in enumerate(segs):
        w = words(s["text"])
        hyp_words += w
        hyp_seg += [k] * len(w)

    errors, hits = Counter(), Counter()
    for op, i, _ in align(ref_words, hyp_words):
        if op == "ins":
            role = ref_role[i] if i is not None else ref_role[0]
            errors[role] += 1
        elif op == "ok":
            hits[ref_role[i]] += 1
        else:
            errors[ref_role[i]] += 1
    total = Counter(ref_role)

    # Which script speaker each segment mostly overlaps.
    seg_role = []
    for s in segs:
        by = Counter()
        for r in ref:
            by[r["speaker"]] += overlap(s["start"], s["end"], r["start"], r["end"])
        seg_role.append(by.most_common(1)[0][0] if by and by.most_common(1)[0][1] > 0 else None)

    # Speaker labels: each label goes to the script speaker it covers most
    # (by time); a speaker scores the words in segments with its own label.
    label_time = defaultdict(Counter)
    for s, role in zip(segs, seg_role):
        if role is not None:
            label_time[s.get("speaker")][role] += s["end"] - s["start"]
    # Greedy one-to-one matching, largest shared time first.
    pairs = sorted(
        ((c[role], str(lab), lab, role) for lab, c in label_time.items() for role in roles if c[role] > 0),
        key=lambda x: (-x[0], x[1], x[3]),
    )
    owner = {}
    for _, _, lab, role in pairs:
        if role not in owner and lab not in owner.values():
            owner[role] = lab

    out = {"labels": len({s.get("speaker") for s in segs}), "segments": len(segs), "speakers": {}}
    for role in roles:
        lines = [r for r in ref if r["speaker"] == role]
        speech = sum(r["end"] - r["start"] for r in lines)
        covered = sum(
            overlap(s["start"], s["end"], r["start"], r["end"]) for r in lines for s in segs
        )
        mine = [k for k, sr in enumerate(seg_role) if sr == role]
        seg_lens = sorted(len(words(segs[k]["text"])) for k in mine)
        n_words = sum(seg_lens)
        right = sum(len(words(segs[k]["text"])) for k in mine if owner.get(role, object()) == segs[k].get("speaker"))
        out["speakers"][role] = {
            "words": total[role],
            "wer": round(errors[role] / total[role], 3) if total[role] else 0.0,
            "hits": hits[role],
            "covered": round(min(1.0, covered / speech), 3) if speech else 0.0,
            "lines": len(lines),
            "segments_per_line": round(len(mine) / len(lines), 2) if lines else 0.0,
            "median_segment_words": seg_lens[len(seg_lens) // 2] if seg_lens else 0,
            "labelled_right": round(right / n_words, 3) if n_words else 0.0,
        }
    return out


def main(argv):
    as_json = "--json" in argv
    args = [a for a in argv if a != "--json"]
    segs = [json.loads(l) for l in open(args[0], encoding="utf-8") if l.strip()]
    ref = json.load(open(args[1], encoding="utf-8"))
    result = score(segs, ref)
    if as_json:
        print(json.dumps(result))
        return
    print(f"{result['segments']} segments, {result['labels']} speaker labels")
    for role, s in result["speakers"].items():
        print(
            f"  {role:<8} WER {s['wer']:.1%}  covered {s['covered']:.0%}  "
            f"segments/line {s['segments_per_line']:.2f}  median words {s['median_segment_words']}  "
            f"labelled right {s['labelled_right']:.0%}  ({s['words']} words, {s['lines']} lines)"
        )


if __name__ == "__main__":
    main(sys.argv[1:])
