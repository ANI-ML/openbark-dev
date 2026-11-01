"""Transcription accuracy by language, for the French eval (docs/eval/french.md).

Usage:
  python -I score_language.py --lang fr --refs DIR [--lexicon LANG=DIR ...]
      [--baseline NAME] NAME=DIR [NAME=DIR ...] [--json OUT]
  python -I score_language.py --self-test

--refs holds the fixture set's ground truth (`<fixture>.ref.json`, as
tools/fixture-tts writes it). Each NAME=DIR is one recogniser: DIR holds
`<fixture>.jsonl` (what `openbark transcribe` printed, one segment per line)
and `<fixture>.log` (its standard error, whose last JSON line holds the
timings). --lexicon LANG=DIR adds more reference transcripts, whose untagged
lines are in LANG, to the word lists that tell the languages apart (the
committed English fixtures, for one).

For each recogniser it reports, pooled over the fixtures (word-weighted):

- WER: word errors (substitutions, deletions, insertions) over the script's
  words. Both sides are normalised the same way: lower case, punctuation,
  hyphens and apostrophes out, "œ" as "oe", and numbers written with digits
  said as words in the language around them ("38,6" is "trente huit virgule
  six" in French, "thirty eight point six" in English), with "mg", "ml",
  "kg", "cm" and "%" spelled out. Accents are kept: a missing accent is an
  error.
- wrong language: the share of the recogniser's words that are words of the
  other language (English in a French consult, French in an English one)
  and are not in the script. A word is English when it is in the English
  word list and not in the French one, and the other way round; a word in
  both lists (or neither) never counts. The lists are built from the
  scripts themselves (the English fixtures, and the French fixtures' French
  and English lines) plus a few hundred common words of each language. This
  is a heuristic: a French word that happens to be in an English script and
  in no French one counts as English.
- real-time factor: recognition time over audio time (the recogniser alone,
  without voice detection), on the machine the run was on.

With --baseline, each other recogniser is held to the bar set in
docs/eval/french.md before any result was seen. It can become the main
recogniser for this language only if, against the baseline:
- its WER is at least 20% lower (relative);
- it has no more wrong-language words, and strictly fewer when the baseline
  has any;
- its real-time factor is at most 1.5 times the baseline's.

Prints Markdown; --json writes the numbers.
"""

import json
import os
import re
import sys
import unicodedata
from collections import Counter

# ---- numbers as words --------------------------------------------------------

FR_UNITS = (
    "zéro un deux trois quatre cinq six sept huit neuf dix onze douze treize quatorze quinze seize".split()
)
FR_TENS = {2: "vingt", 3: "trente", 4: "quarante", 5: "cinquante", 6: "soixante"}
EN_ONES = (
    "zero one two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen "
    "sixteen seventeen eighteen nineteen"
).split()
EN_TENS = "_ _ twenty thirty forty fifty sixty seventy eighty ninety".split()


def fr_int(n):
    """French words for an integer, hyphens as spaces ("quatre vingt dix")."""
    if n < 17:
        return [FR_UNITS[n]]
    if n < 20:
        return ["dix", FR_UNITS[n - 10]]
    if n < 70:
        t, u = divmod(n, 10)
        return [FR_TENS[t]] + (["et", "un"] if u == 1 else fr_int(u) if u else [])
    if n < 80:
        return ["soixante", "et", "onze"] if n == 71 else ["soixante"] + fr_int(n - 60)
    if n < 100:
        return ["quatre", "vingts"] if n == 80 else ["quatre", "vingt"] + fr_int(n - 80)
    if n < 1000:
        h, r = divmod(n, 100)
        head = ["cent"] if h == 1 else fr_int(h) + ["cents" if r == 0 else "cent"]
        return head + (fr_int(r) if r else [])
    if n < 1_000_000:
        th, r = divmod(n, 1000)
        head = ["mille"] if th == 1 else fr_int(th) + ["mille"]
        return head + (fr_int(r) if r else [])
    return [str(n)]


def en_int(n):
    """English words for an integer, without "and" ("one hundred twenty")."""
    if n < 20:
        return [EN_ONES[n]]
    if n < 100:
        return [EN_TENS[n // 10]] + ([EN_ONES[n % 10]] if n % 10 else [])
    if n < 1000:
        return [EN_ONES[n // 100], "hundred"] + (en_int(n % 100) if n % 100 else [])
    if n < 1_000_000:
        return en_int(n // 1000) + ["thousand"] + (en_int(n % 1000) if n % 1000 else [])
    return [str(n)]


def number_words(tok, lang):
    """Words for a number written with digits: "38,6" or "38.6"."""
    m = re.fullmatch(r"(\d+)(?:[.,](\d+))?", tok)
    if not m:
        return [tok]
    whole, frac = int(m.group(1)), m.group(2)
    if lang == "fr":
        out = fr_int(whole)
        if frac:
            if frac.startswith("0"):
                out += ["virgule"] + [FR_UNITS[int(d)] for d in frac]
            else:
                out += ["virgule"] + fr_int(int(frac))
        return out
    out = en_int(whole)
    if frac:
        out += ["point"] + [EN_ONES[int(d)] for d in frac]
    return out


UNITS = {
    "fr": {"mg": ["milligrammes"], "ml": ["millilitres"], "kg": ["kilos"], "cm": ["centimètre"],
           "zzpct": ["pour", "cent"], "zzsur": ["sur"]},
    "en": {"mg": ["milligrams"], "ml": ["milliliters"], "kg": ["kilos"], "cm": ["centimeter"],
           "zzpct": ["percent"], "zzsur": ["out", "of"]},
}

# ---- word lists that tell French and English apart --------------------------

COMMON_EN = set("""
a about after again all also always am an and any are around as ask at away back bad be because been
before being best better big bit both but by call came can come could day did do does doing done down
each eat even ever every eye feel few find first for from get give go going good got great had has have
he her here him his how i if in into is it its just keep kind know last left let like little long look
lot made make many may me might more morning most much must my need never new next night no not now of
off oh ok okay old on once one only or other our out over own people please put quite really right said
same say see she should show since so some something soon still such sure take tell than thank thanks
that the their them then there these they thing think this those though through time to today too try
um uh up us use very want was way we week well were what when where which while who why will with
without would yeah yes yet you your strong being see that blood gums tablet tablets once twice day days
hour hours dog cat she's he's it's that's don't i'm you're we'll weird garbage belly soft pain mass
dehydrated percent sunday tomorrow starting call right away still vomiting vomits food bit tacky give
""".split())

COMMON_FR = set("""
à a ai alors après as au aujourd aussi autre aux avant avec avez avoir bien bon bonjour ça car ce cela
celle ces cet cette chat chez chien comme comment d dans de des deux dire dit docteur donc dont du elle
elles en encore entre est et été être eu fait faire fois il ils j je jour jours jusqu l la le les leur
leurs lui m ma madame mais me même mes merci moi moins mon monsieur n ne ni nos notre nous on ont ou où
oui par parce pas peu peut plus pour pourquoi qu quand que quel quelle quelque qui quoi rien s sa sans se
ses si sinon son sont sous sur t ta te tes toi ton tous tout toute toutes très tu un une vous vos votre
y ok d'accord parfait super voilà bah ben euh semaine semaines mois matin soir nuit eau ventre yeux œil
oeil oreille oreilles patte pattes dents gencives poids température comprimé comprimés gouttes pommade
""".split())

TOKEN = re.compile(r"\d+(?:[.,]\d+)?|[^\W\d_]+")


def tokens(text, lang, en_words=None, fr_words=None):
    """Normalised words of `text`. In a French set (`lang` "fr"), a number or
    unit is said in the language of the words around it."""
    t = unicodedata.normalize("NFC", text).lower()
    t = t.replace("œ", "oe").replace("æ", "ae")
    t = re.sub(r"(\d),(\d{3})(?!\d)", r"\1\2", t) if lang == "en" else t
    t = re.sub(r"(\d)\s*%", r"\1 zzpct", t)
    t = re.sub(r"(\d)\s*/\s*(\d)", r"\1 zzsur \2", t)
    t = re.sub(r"°\s*c\b|°", " ", t)
    t = re.sub(r"[-'’‘`]", " ", t)
    raw = TOKEN.findall(t)
    out = []
    for i, w in enumerate(raw):
        here = lang
        if lang == "fr" and en_words is not None and (w[0].isdigit() or w in UNITS["fr"]):
            window = [x for x in raw[max(0, i - 4) : i + 5] if not x[0].isdigit()]
            en = sum(1 for x in window if x in en_words and x not in fr_words)
            fr = sum(1 for x in window if x in fr_words and x not in en_words)
            here = "en" if en > fr else "fr"
        if w[0].isdigit():
            out.extend(number_words(w, here))
        elif w in UNITS[here]:
            out.extend(UNITS[here][w])
        else:
            out.append(w)
    if lang == "en":
        out = [w for w in out if w != "and"]
    return out


def edits(ref, hyp):
    """Word edit distance (substitutions, deletions, insertions)."""
    prev = list(range(len(hyp) + 1))
    for i in range(1, len(ref) + 1):
        row = [i] + [0] * len(hyp)
        for j in range(1, len(hyp) + 1):
            row[j] = min(prev[j - 1] + (ref[i - 1] != hyp[j - 1]), prev[j] + 1, row[j - 1] + 1)
        prev = row
    return prev[-1]


# ---- loading -----------------------------------------------------------------


def load_ref(path):
    return json.load(open(path, encoding="utf-8"))


def ref_text(lines, lang=None):
    """The script's words; with `lang`, only its lines in that language
    (lines without a tag are in the set's own language)."""
    return " ".join(l["text"] for l in lines if lang is None or l.get("lang", lang) == lang)


def hyp_text(path):
    out = []
    for line in open(path, encoding="utf-8"):
        line = line.strip()
        if line:
            out.append(json.loads(line).get("text", ""))
    return " ".join(out)


def timings(path):
    """The `{"transcribe": {...}}` line `openbark transcribe` prints last."""
    if not os.path.exists(path):
        return None
    found = None
    for line in open(path, encoding="utf-8", errors="replace"):
        line = line.strip()
        if line.startswith("{") and '"transcribe"' in line:
            try:
                found = json.loads(line)["transcribe"]
            except (ValueError, KeyError):
                pass
    return found


def lexicons(ref_dirs):
    """English and French word lists: the common words, plus every script
    word, by the language of its line. `ref_dirs`: (language of untagged
    lines, folder) pairs."""
    words = {"en": set(COMMON_EN), "fr": set(COMMON_FR)}
    for lang, d in ref_dirs:
        for f in sorted(os.listdir(d)):
            if not f.endswith(".ref.json"):
                continue
            for l in load_ref(os.path.join(d, f)):
                line_lang = l.get("lang", lang)
                if line_lang in words:
                    words[line_lang].update(tokens(l["text"], line_lang))
    return words["en"], words["fr"]


# ---- scoring -----------------------------------------------------------------


def score_arm(lang, refs_dir, arm_dir, en_words, fr_words):
    fixtures = {}
    for f in sorted(os.listdir(refs_dir)):
        if not f.endswith(".ref.json"):
            continue
        fixture = f[: -len(".ref.json")]
        hyp_path = os.path.join(arm_dir, fixture + ".jsonl")
        if not os.path.exists(hyp_path):
            continue
        lines = load_ref(os.path.join(refs_dir, f))
        ref = tokens(ref_text(lines), lang, en_words, fr_words)
        hyp = tokens(hyp_text(hyp_path), lang, en_words, fr_words)
        said = set(ref)
        other, own = (en_words, fr_words) if lang == "fr" else (fr_words, en_words)
        wrong = [w for w in hyp if w in other and w not in own and w not in said]
        t = timings(os.path.join(arm_dir, fixture + ".log")) or {}
        fixtures[fixture] = {
            "mixed": any("lang" in l for l in lines),
            "ref_words": len(ref),
            "hyp_words": len(hyp),
            "edits": edits(ref, hyp),
            "wrong": len(wrong),
            "wrong_words": dict(Counter(wrong).most_common(12)),
            "audio_secs": t.get("audio_secs"),
            "asr_ms": t.get("asr_ms"),
        }
    return fixtures


def pool(fixtures):
    fx = list(fixtures.values())
    n = sum(f["ref_words"] for f in fx)
    h = sum(f["hyp_words"] for f in fx)
    timed = [f for f in fx if f["audio_secs"] and f["asr_ms"] is not None]
    audio = sum(f["audio_secs"] for f in timed)
    wrong_words = Counter()
    for f in fx:
        wrong_words.update(f["wrong_words"])
    return {
        "fixtures": len(fx),
        "ref_words": n,
        "wer": sum(f["edits"] for f in fx) / n if n else None,
        "wrong": sum(f["wrong"] for f in fx),
        "wrong_share": sum(f["wrong"] for f in fx) / h if h else None,
        "rtf": sum(f["asr_ms"] for f in timed) / 1000.0 / audio if audio and len(timed) == len(fx) else None,
        "audio_secs": audio,
        "wrong_words": dict(wrong_words.most_common(12)),
    }


def verdict(cand, base):
    """The pre-set bar (docs/eval/french.md): a list of (test, passed, detail)."""
    out = []
    if cand["wer"] is None or base["wer"] is None or base["wer"] == 0:
        out.append(("WER at least 20% lower", False, "no WER"))
    else:
        rel = 1 - cand["wer"] / base["wer"]
        out.append(("WER at least 20% lower", rel >= 0.20, f"{rel:+.0%} relative"))
    fewer = cand["wrong"] <= base["wrong"] and (base["wrong"] == 0 or cand["wrong"] < base["wrong"])
    out.append(("fewer wrong-language words", fewer, f"{cand['wrong']} against {base['wrong']}"))
    if cand["rtf"] is None or base["rtf"] is None or base["rtf"] == 0:
        out.append(("real-time factor at most 1.5x", False, "no timing"))
    else:
        ratio = cand["rtf"] / base["rtf"]
        out.append(("real-time factor at most 1.5x", ratio <= 1.5, f"{ratio:.2f}x"))
    return out


def pct(x):
    return "-" if x is None else f"{100 * x:.1f}%"


def num(x, fmt="{:.3f}"):
    return "-" if x is None else fmt.format(x)


def report(lang, results, baseline):
    names = list(results)
    lines = []
    groups = [("all", lambda f: True)]
    if any(f["mixed"] for r in results.values() for f in r.values()):
        groups = [("all", lambda f: True), ("single-language", lambda f: not f["mixed"]),
                  ("mixed", lambda f: f["mixed"])]
    pooled = {}
    for gname, keep in groups:
        lines.append(f"**{lang} fixtures, {gname}**")
        lines.append("")
        lines.append("| recogniser | fixtures | script words | WER | wrong-language words | wrong-language share | real-time factor |")
        lines.append("|---|---|---|---|---|---|---|")
        for name in names:
            p = pool({k: v for k, v in results[name].items() if keep(v)})
            pooled.setdefault(name, {})[gname] = p
            lines.append(
                f"| {name} | {p['fixtures']} | {p['ref_words']} | {pct(p['wer'])} | {p['wrong']} | "
                f"{pct(p['wrong_share'])} | {num(p['rtf'])} |"
            )
        lines.append("")
    fixtures = sorted({k for r in results.values() for k in r})
    lines.append(f"**{lang} fixtures, WER and wrong-language words per fixture**")
    lines.append("")
    lines.append("| fixture | " + " | ".join(names) + " |")
    lines.append("|---|" + "---|" * len(names))
    for fx in fixtures:
        cells = []
        for name in names:
            f = results[name].get(fx)
            cells.append("-" if not f else f"{pct(f['edits'] / max(f['ref_words'], 1))}, {f['wrong']}")
        lines.append(f"| {fx} | " + " | ".join(cells) + " |")
    lines.append("")
    for name in names:
        ww = pooled[name]["all"]["wrong_words"]
        if ww:
            lines.append(f"Wrong-language words, {name}: " + ", ".join(f"{w} ({n})" for w, n in ww.items()))
    lines.append("")
    verdicts = {}
    if baseline and baseline in pooled:
        lines.append(f"**The pre-set bar, against {baseline} ({lang} fixtures, all)**")
        lines.append("")
        lines.append("| recogniser | WER at least 20% lower | fewer wrong-language words | real-time factor at most 1.5x | passes |")
        lines.append("|---|---|---|---|---|")
        for name in names:
            if name == baseline:
                continue
            v = verdict(pooled[name]["all"], pooled[baseline]["all"])
            ok = all(p for _, p, _ in v)
            verdicts[name] = {"passes": ok, "tests": [{"test": t, "passed": p, "detail": d} for t, p, d in v]}
            cells = " | ".join(("yes" if p else "no") + f" ({d})" for _, p, d in v)
            lines.append(f"| {name} | {cells} | {'**yes**' if ok else 'no'} |")
        lines.append("")
    return "\n".join(lines), {"pooled": pooled, "fixtures": results, "verdicts": verdicts}


def self_test():
    assert fr_int(180) == ["cent", "quatre", "vingts"], fr_int(180)
    assert fr_int(71) == ["soixante", "et", "onze"]
    assert fr_int(99) == ["quatre", "vingt", "dix", "neuf"]
    assert fr_int(21) == ["vingt", "et", "un"]
    assert fr_int(250) == ["deux", "cent", "cinquante"]
    assert fr_int(200) == ["deux", "cents"]
    assert fr_int(17) == ["dix", "sept"]
    assert number_words("38,6", "fr") == ["trente", "huit", "virgule", "six"]
    assert number_words("0.5", "fr") == ["zéro", "virgule", "cinq"]
    assert number_words("0,05", "fr") == ["zéro", "virgule", "zéro", "cinq"]
    assert number_words("32.4", "en") == ["thirty", "two", "point", "four"]
    assert tokens("Trente-huit virgule six.", "fr") == ["trente", "huit", "virgule", "six"]
    assert tokens("Température 38,6 °C, l'œil gauche", "fr") == [
        "température", "trente", "huit", "virgule", "six", "l", "oeil", "gauche"]
    assert tokens("0,5 cm trois fois par jour", "fr") == [
        "zéro", "virgule", "cinq", "centimètre", "trois", "fois", "par", "jour"]
    assert tokens("5 %", "fr") == ["cinq", "pour", "cent"]
    assert tokens("grade 3/5", "fr") == ["grade", "trois", "sur", "cinq"]
    en, fr = {"sixty", "tablets", "give", "her", "milligrams"}, {"le", "matin"}
    assert tokens("give her Cerenia tablets, 60 mg once", "fr", en, fr) == [
        "give", "her", "cerenia", "tablets", "sixty", "milligrams", "once"]
    assert tokens("le matin 60 mg", "fr", en, fr) == ["le", "matin", "soixante", "milligrammes"]
    assert tokens("She weighs 1,250 grams and 4.2 kg", "en") == [
        "she", "weighs", "one", "thousand", "two", "hundred", "fifty", "grams", "four", "point", "two",
        "kilos"]
    assert edits("a b c".split(), "a x c d".split()) == 2
    base = {"wer": 0.10, "wrong": 4, "rtf": 0.10}
    assert all(p for _, p, _ in verdict({"wer": 0.079, "wrong": 1, "rtf": 0.15}, base))
    assert not verdict({"wer": 0.081, "wrong": 1, "rtf": 0.1}, base)[0][1]
    assert not verdict({"wer": 0.05, "wrong": 4, "rtf": 0.1}, base)[1][1]
    assert not verdict({"wer": 0.05, "wrong": 0, "rtf": 0.16}, base)[2][1]
    assert verdict({"wer": 0.05, "wrong": 0, "rtf": 0.1}, {"wer": 0.1, "wrong": 0, "rtf": 0.1})[1][1]
    print("score_language self-test passed")


def main(argv):
    if argv == ["--self-test"]:
        self_test()
        return
    out_json, lang, refs, baseline, extra, arms = None, None, None, None, [], []
    i = 0
    while i < len(argv):
        a = argv[i]
        if a in ("--json", "--lang", "--refs", "--baseline", "--lexicon"):
            v = argv[i + 1]
            i += 2
            if a == "--json":
                out_json = v
            elif a == "--lang":
                lang = v
            elif a == "--refs":
                refs = v
            elif a == "--baseline":
                baseline = v
            else:
                extra.append(tuple(v.split("=", 1)))
            continue
        arms.append(a.split("=", 1))
        i += 1
    if lang not in ("fr", "en") or not refs or not arms:
        sys.exit(__doc__)
    en_words, fr_words = lexicons([(lang, refs)] + extra)
    results = {name: score_arm(lang, refs, folder, en_words, fr_words) for name, folder in arms}
    text, data = report(lang, results, baseline)
    print(text)
    if out_json:
        json.dump(data, open(out_json, "w", encoding="utf-8"), indent=2, ensure_ascii=False)


if __name__ == "__main__":
    main(sys.argv[1:])
