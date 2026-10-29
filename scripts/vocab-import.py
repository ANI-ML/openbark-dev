#!/usr/bin/env python3
"""Build packages/vocab/regulatory.tsv from public veterinary drug registers.

OpenBark hints likely mis-heard drug names (engine/os-engine/src/vocab.rs).
The hand-written list (packages/vocab/veterinary.tsv) has a few hundred
terms; this script adds the brand and ingredient names of veterinary
medicines registered in the US, UK, Canada and Australia, plus the brand
names of EU centrally authorised veterinary medicines:

  us-fda-gb  US FDA, Approved Animal Drug Products (Green Book), via Animal
             Drugs @ FDA. US government work, public domain.
  uk-vmd     Veterinary Medicines Directorate, Product Information Database.
             Open Government Licence v3.0.
  ca-dpd     Health Canada, Drug Product Database (CLASS "Veterinary").
             Open Government Licence - Canada.
  au-apvma   APVMA PubCRIS dataset on data.gov.au (veterinary products).
             CC BY 3.0 AU.
  eu-ema     European Medicines Agency, medicines table (veterinary rows,
             brand names only). Source: EMA.

Each product name is reduced to its spoken brand word (strengths, forms,
species and company prefixes removed) and each active ingredient to its bare
name (salts dropped). Ordinary English words are left out (SCOWL word lists,
size 50; downloaded for the build only, never shipped), as are words that
the hint matcher would confuse with an ordinary word or with something said
in the test consults. A term must be one or two words.

Usage (Python 3.11+; xlrd 2.x for the FDA's legacy .xls files):

  python -I scripts/vocab-import.py --cache <download folder> [--xlrd <folder with xlrd>]

Downloads go into --cache (an empty folder of their own) and are reused when
present; downloaded files are treated as data only. Output is written with
"\\n" line endings and no byte-order mark. `--report` prints what each filter
removed.
"""

from __future__ import annotations

import argparse
import collections
import csv
import datetime
import hashlib
import html
import io
import json
import os
import re
import sys
import tarfile
import unicodedata
import urllib.request
import zipfile

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.normpath(os.path.join(HERE, ".."))
OUT = os.path.join(ROOT, "packages", "vocab", "regulatory.tsv")
BUILTIN = os.path.join(ROOT, "packages", "vocab", "veterinary.tsv")
MISHEARD = os.path.join(ROOT, "packages", "vocab", "mishearings.tsv")
EVERYDAY = os.path.join(ROOT, "packages", "vocab", "everyday.tsv")
VOCAB_RS = os.path.join(ROOT, "engine", "os-engine", "src", "vocab.rs")
REFS = os.path.join(ROOT, "packages", "eval", "fixtures", "audio")

FDA_BASE = "https://animaldrugsatfda.fda.gov/adafda/app/search/public"
DOWNLOADS = {
    "fda-trade.xls": f"{FDA_BASE}/tradeSponsorExcelByTradeName/Section11ByTradeName",
    "fda-ingredients.xls": f"{FDA_BASE}/ingredientsInformationExcel/Section2ActiveIngredients",
    "vmd.xlsx": "https://www.vmd.defra.gov.uk/ProductInformationDatabase/downloads/VMD_ProductInformationDatabase.xlsx",
    "dpd-allfiles.zip": "https://www.canada.ca/content/dam/hc-sc/documents/services/drug-product-database/allfiles.zip",
    "ema-medicines.xlsx": "https://www.ema.europa.eu/en/documents/report/medicines-output-medicines-report_en.xlsx",
    "scowl.tar.gz": "https://downloads.sourceforge.net/wordlist/scowl-2020.12.07.tar.gz",
}
# The APVMA files are found through data.gov.au's catalogue (their resource
# ids change when the dataset is republished).
APVMA_PACKAGE = (
    "https://data.gov.au/data/api/3/action/package_show"
    "?id=apvma-pubcris-dataset-for-registered-agricultural-and-veterinary-chemical-products-and-approved-acti"
)
APVMA_FILES = ["product.csv", "prodcon.csv", "constit.csv"]
SCOWL_SHA256 = "5587667caa20c4891390c2d42dbb4d5c4c3f41bee77af1457ece3ba23fb859cc"
SCOWL_MAX_LEVEL = 50
# Words common enough to turn up in a consult; a term too close to one of
# these would be hinted on ordinary speech.
COMMON_LEVEL = 35

# ------------------------------------------------------------ word lists

# Words that end a brand name: forms, strengths, species, qualifiers.
STOP = set(
    """
    a an and with for of in on to by the plus or from without per vet vets veterinary
    tablet tablets tab tabs caplet caplets capsule capsules chewable chewables chew chews
    injection injectable injectables inj solution solutions suspension suspensions oral
    orally powder powders premix granules granule paste gel cream ointment ont lotion
    spray drops drop ear eye ophthalmic otic topical spot pour pouron spoton collar
    shampoo wash concentrate soluble feed medicated medication bolus boluses implant
    implants liquid liq syrup elixir emulsion pellets pellet block blocks drench
    pipette pipettes sterile intramammary infusion aerosol foam film coated tube tubes
    tincture tct dust wipes wipe cleanser cleaner rinse dip dips additive supplement
    mixture combination combo kit pack vial vials bottle bottles bag bags lc la sr xr
    dog dogs canine canines cat cats feline felines puppy puppies kitten kittens
    cattle calf calves cow cows heifer heifers beef dairy horse horses equine pony foal
    pig pigs swine porcine piglet piglets sheep ovine lamb lambs goat goats
    chicken chickens poultry turkey turkeys bird birds avian fish salmon rabbit rabbits
    ferret ferrets small medium large extra giant mini toy xl xxl xs
    lb lbs kg mg mcg ml g iu ui units unit dose doses
    type ii iii iv injectable forte retard depot long acting la ready use rtu
    new improved original formula strength regular double triple
    anti antibiotic antibacterial antiinflammatory antifungal antiseptic disinfectant
    insecticide insecticidal wormer dewormer drench tapewormer
    """.split()
)
# Leading words that are company names, not the brand.
COMPANIES = set(
    """
    zoetis elanco bayer merial intervet msd boehringer ingelheim virbac ceva dechra norbrook
    bimeda vetoquinol krka chanelle huvepharma alfasan kela coopers troy ilium jurox apex
    parnell animalcare ecuphar emdoka vetpharma genera aurora pfizer schering novartis
    fort dodge wyeth phibro alstoe pharmachem ausrichter sykes nature vet vetsense
    bomac abbey ranvet randlab mavlab nexgen ancare agrimin bioproperties zagro
    dopharma eurovet livisto bela pharma vetcare vetplus vetuk animal health
    """.split()
)
# Salts, esters and forms dropped from ingredient names.
SALTS = set(
    """
    maleate sulfate sulphate hydrochloride hcl dihydrochloride sodium potassium
    acetate diacetate phosphate citrate tartrate mesylate besylate succinate
    propionate dipropionate valerate hydrobromide fumarate tosylate oxime
    trihydrate monohydrate dihydrate hemihydrate sesquihydrate hydrate anhydrous
    base usp bp eur ph micronized micronised equivalent as salt magnesium zinc
    calcium chloride bromide iodide nitrate lactate gluconate palmitate pamoate
    embonate stearate benzoate hyclate disodium trisodium dipotassium edetate
    sodium-free free ester esters methylsulfate isethionate napsylate meglumine
    benzathine procaine dicyclohexylamine decanoate cypionate enanthate pivalate
    hydroxide oxide carbonate bicarbonate sulfide sulphide silicate peroxide dioxide
    sulfite sulphite nitrite thiosulfate aspartate glutamate glycinate orotate
    """.split()
)
# Organisms, antigens and things that are not names anyone says as a drug.
BIOLOGICAL = re.compile(
    r"virus|vaccine|strain|toxoid|antigen|bacterin|inactivated|attenuated|\blive\b|cells?\b|"
    r"protein|serovar|serotype|subsp|culture|lactobacill|bacill|coccus|cocci|bacter|"
    r"mycoplasma|clostridi|salmonella|leptospira|pasteurella|escherichia|\bcoli\b|"
    r"bordetella|chlamyd|herpes|parvo|corona|influenza|rabies|distemper|calici|"
    r"rhinotracheitis|circo|immune|immuno|globulin|serum|plasma|extract|\boil\b|"
    r"\bwater\b|homeopath|\bmother tincture\b|\bpollen\b|\ballergen",
    re.I,
)
VACCINE_GROUP = re.compile(r"vaccin|immunolog|bacterin|toxoid", re.I)

# Kept although an English word list has them (well-known veterinary brands).
ALLOW: set[str] = set()
# Left out although they pass the filters: everyday words, or names too close
# to an everyday word or a common clinical word to hint safely.
DENY = set(
    """
    advantage advocate revolution sentinel interceptor glusosamine
    """.split()
)


def ascii_fold(s: str) -> str:
    s = s.replace("®", " ").replace("™", " ").replace("©", " ")
    return unicodedata.normalize("NFKD", s).encode("ascii", "ignore").decode("ascii")


def tokens(s: str) -> list[str]:
    s = ascii_fold(s).lower()
    s = re.sub(r"\([^)]*\)", " ", s)
    return re.findall(r"[a-z0-9]+(?:'[a-z]+)?|[^a-z0-9\s]", s)


# ------------------------------------------------------------- downloads


def fetch(cache: str, name: str, url: str) -> str:
    path = os.path.join(cache, name)
    if os.path.isfile(path) and os.path.getsize(path) > 0:
        return path
    print(f"downloading {url}", file=sys.stderr)
    req = urllib.request.Request(url, headers={"User-Agent": "OpenBark vocabulary import (research)"})
    with urllib.request.urlopen(req, timeout=300) as r:
        data = r.read()
    with open(path, "wb") as f:
        f.write(data)
    return path


def fetch_apvma(cache: str) -> dict[str, str]:
    paths = {n: os.path.join(cache, f"apvma-{n}") for n in APVMA_FILES}
    if all(os.path.isfile(p) and os.path.getsize(p) > 0 for p in paths.values()):
        return paths
    with urllib.request.urlopen(APVMA_PACKAGE, timeout=120) as r:
        package = json.load(r)
    urls = {res["name"]: res["url"] for res in package["result"]["resources"]}
    for n in APVMA_FILES:
        fetch(cache, f"apvma-{n}", urls[n])
    return paths


# ---------------------------------------------------------------- readers


def xlsx_sheets(path: str) -> dict[str, list[dict[str, str]]]:
    """Every sheet of an .xlsx file as rows of {column letter: text}. Standard library only."""
    z = zipfile.ZipFile(path)
    names = z.namelist()
    ss_name = next((n for n in names if n.lower() == "xl/sharedstrings.xml"), None)
    strings: list[str] = []
    if ss_name:
        ss = z.read(ss_name).decode("utf-8-sig")
        for si in re.findall(r"<(?:\w+:)?si>(.*?)</(?:\w+:)?si>", ss, re.S):
            strings.append(html.unescape("".join(re.findall(r"<(?:\w+:)?t[^>]*>([^<]*)</(?:\w+:)?t>", si))))
    wb = z.read("xl/workbook.xml").decode("utf-8-sig")
    rels = z.read("xl/_rels/workbook.xml.rels").decode("utf-8-sig")
    targets = dict(re.findall(r'<(?:\w+:)?Relationship [^>]*Id="([^"]+)"[^>]*Target="([^"]+)"', rels))
    targets.update({k: v for v, k in re.findall(r'<(?:\w+:)?Relationship [^>]*Target="([^"]+)"[^>]*Id="([^"]+)"', rels)})
    out = {}
    for attrs in re.findall(r"<(?:\w+:)?sheet ([^>]*)/?>", wb):
        name = re.search(r'name="([^"]+)"', attrs).group(1)
        rid = re.search(r'r:id="([^"]+)"', attrs).group(1)
        target = targets[rid].lstrip("/")
        member = target if target.startswith("xl/") else "xl/" + target
        data = z.read(member).decode("utf-8-sig")
        rows = []
        for row in re.findall(r"<(?:\w+:)?row[^>]*>(.*?)</(?:\w+:)?row>", data, re.S):
            cells = {}
            for ref, cattrs, body in re.findall(
                r'<(?:\w+:)?c r="([A-Z]+)\d+"([^>]*?)(?:/>|>(.*?)</(?:\w+:)?c>)', row, re.S
            ):
                v = re.search(r"<(?:\w+:)?v>([^<]*)</(?:\w+:)?v>", body or "")
                if 't="s"' in cattrs and v:
                    cells[ref] = strings[int(v.group(1))]
                elif 't="inlineStr"' in cattrs:
                    cells[ref] = html.unescape("".join(re.findall(r"<(?:\w+:)?t[^>]*>([^<]*)<", body or "")))
                elif v:
                    cells[ref] = html.unescape(v.group(1))
            rows.append(cells)
        out[name] = rows
    return out


def with_header(rows: list[dict[str, str]], must: str) -> list[dict[str, str]]:
    """Rows keyed by header text, starting after the first row that has `must`."""
    for i, r in enumerate(rows):
        if must in r.values():
            header = {k: v.strip() for k, v in r.items()}
            return [{header[k]: v for k, v in row.items() if k in header} for row in rows[i + 1 :]]
    raise SystemExit(f"no header row with {must!r}")


# A product: (source, name, holder, [ingredients], vaccine?)
Product = tuple[str, str, str, list[str], bool]


def read_fda(trade: str, ingr: str, xlrd) -> list[Product]:
    t = xlrd.open_workbook(trade).sheet_by_index(0)
    i = xlrd.open_workbook(ingr).sheet_by_index(0)
    th = [str(x).strip() for x in t.row_values(0)]
    ih = [str(x).strip() for x in i.row_values(0)]
    ingredients: dict[str, list[str]] = collections.defaultdict(list)
    for r in range(1, i.nrows):
        row = dict(zip(ih, i.row_values(r)))
        ingredients[str(row["Application Number"]).strip()].append(str(row["Ingredient"]))
    out = []
    for r in range(1, t.nrows):
        row = dict(zip(th, t.row_values(r)))
        app = str(row["Application Number"]).strip()
        for name in str(row["Trade Name"]).split(","):
            out.append(("us-fda-gb", name, str(row["Sponsor Name"]), ingredients.get(app, []), False))
    return out


def read_vmd(path: str) -> list[Product]:
    rows = with_header(xlsx_sheets(path)["CurrentAuthorisedProducts"], "ActiveSubstances")
    out = []
    for r in rows:
        if not r.get("Name"):
            continue
        actives = [a for a in re.split(r",|;| and ", r.get("ActiveSubstances", "")) if a.strip()]
        vaccine = bool(VACCINE_GROUP.search(r.get("TherapeuticGroup", "")))
        out.append(("uk-vmd", r["Name"], r.get("MAHolder", ""), actives, vaccine))
    return out


def read_dpd(path: str) -> list[Product]:
    z = zipfile.ZipFile(path)

    def rows(name: str) -> list[list[str]]:
        return list(csv.reader(io.TextIOWrapper(z.open(name), encoding="utf-8", errors="replace")))

    vet = {r[0]: r for r in rows("drug.txt") if len(r) > 4 and r[2].strip() == "Veterinary"}
    holders = {r[0]: r[3] for r in rows("comp.txt") if len(r) > 3}
    ingredients: dict[str, list[str]] = collections.defaultdict(list)
    for r in rows("ingred.txt"):
        if r[0] in vet:
            ingredients[r[0]].append(r[2])
    vaccines = {r[0] for r in rows("ther.txt") if len(r) > 2 and VACCINE_GROUP.search(" ".join(r[1:]))}
    return [("ca-dpd", r[4], holders.get(code, ""), ingredients[code], code in vaccines) for code, r in vet.items()]


def read_apvma(paths: dict[str, str]) -> list[Product]:
    def rows(name: str) -> list[dict[str, str]]:
        with open(paths[name], encoding="utf-8-sig", errors="replace", newline="") as f:
            return list(csv.DictReader(f))

    constit = {r["ccode"].strip(): (r["cname"], r["clevel1"]) for r in rows("constit.csv")}
    ingredients: dict[str, list[str]] = collections.defaultdict(list)
    vaccine: set[str] = set()
    for r in rows("prodcon.csv"):
        if r["ctype"].strip() != "A":
            continue
        name, level = constit.get(r["ccode"].strip(), ("", ""))
        ingredients[r["pcode"].strip()].append(name)
        if VACCINE_GROUP.search(level) or level.upper() == "BIOLOGICAL":
            vaccine.add(r["pcode"].strip())
    out = []
    for r in rows("product.csv"):
        if not r["typedesc"].startswith("V"):
            continue
        code = r["pcode"].strip()
        out.append(("au-apvma", r["fpname"], r["sname"], ingredients[code], code in vaccine))
    return out


def read_ema(path: str) -> list[Product]:
    sheets = xlsx_sheets(path)
    rows = with_header(next(iter(sheets.values())), "Name of medicine")
    out = []
    for r in rows:
        if r.get("Category") != "Veterinary" or r.get("Medicine status") != "Authorised":
            continue
        group = next((v for k, v in r.items() if k.startswith("Pharmacotherapeutic group")), "")
        holder = next((v for k, v in r.items() if k.startswith("Marketing authorisation")), "")
        # Brand names only (decision of 2026-10-28): no ingredients from this table.
        out.append(("eu-ema", r["Name of medicine"], holder, [], bool(VACCINE_GROUP.search(group))))
    return out


def read_scowl(path: str) -> tuple[set[str], set[str]]:
    """(every word up to SCOWL_MAX_LEVEL, names included; everyday lowercase words up to COMMON_LEVEL)."""
    with open(path, "rb") as f:
        digest = hashlib.sha256(f.read()).hexdigest()
    if digest != SCOWL_SHA256:
        raise SystemExit(f"SCOWL archive checksum {digest} is not the pinned {SCOWL_SHA256}")
    words: set[str] = set()
    common: set[str] = set()
    pattern = re.compile(
        r"/final/(english|american|british|british_z|canadian|australian)(_variant_\d)?"
        r"-(words|upper|proper-names|contractions)\.(\d+)$"
    )
    with tarfile.open(path) as t:
        for m in t.getmembers():
            hit = pattern.search(m.name)
            if not hit or int(hit.group(4)) > SCOWL_MAX_LEVEL:
                continue
            everyday = hit.group(3) == "words" and int(hit.group(4)) <= COMMON_LEVEL
            for line in t.extractfile(m).read().decode("latin-1").splitlines():
                w = ascii_fold(line.strip()).lower()
                if w:
                    words.add(w)
                    if everyday and line.strip().islower():
                        common.add(w)
    return words, common


# ------------------------------------------------------------ normalising


def brand(name: str, holder: str, english: set[str]) -> str | None:
    """The spoken brand word of a product name, or None."""
    toks = tokens(name)
    head: list[str] = []
    for tok in toks:
        if not tok.isalpha() or tok in STOP:
            break
        head.append(tok)
    holder_words = set(tokens(holder))
    while head and (head[0] in COMPANIES or head[0] in holder_words):
        head.pop(0)
    # The first word that can stand as a brand, within the first two.
    for tok in head[:2]:
        if len(tok) >= 5 and tok not in english and tok not in STOP:
            return tok
        if tok not in english and len(tok) < 5:
            return None
    return None


def ingredient(text: str, english: set[str]) -> str | None:
    """The bare ingredient name, or None for salts-only, organisms and the like."""
    if BIOLOGICAL.search(text):
        return None
    words = [w for w in tokens(text) if w.isalpha() and w not in SALTS]
    if words and words[-1] == "acid" and len(words) == 2:
        return None if words[0] in english else " ".join(words)
    if len(words) != 1:
        return None
    w = words[0]
    return w if len(w) >= 5 and w not in english else None


# ------------------------------------------------- hint matcher (mirror)
# A copy of engine/os-engine/src/vocab.rs's scoring, kept in step with it, so
# terms the engine would confuse with ordinary speech are left out here.


def sound_key(s: str) -> str:
    t = "".join(c for c in s.lower() if "a" <= c <= "z")
    for a, b in [("ph", "f"), ("ck", "k"), ("qu", "kw"), ("x", "ks"), ("z", "s"), ("y", "i"), ("c", "k")]:
        t = t.replace(a, b)
    out = ""
    for i, c in enumerate(t):
        if c in "aeiou" and i > 0:
            c = "a"
        if out.endswith(c):
            continue
        out += c
    return out


def jaro_winkler(a: str, b: str) -> float:
    if not a or not b:
        return 0.0
    window = max(max(len(a), len(b)) // 2 - 1, 0)
    am = [False] * len(a)
    bm = [False] * len(b)
    m = 0
    for i, ca in enumerate(a):
        lo, hi = max(0, i - window), min(i + window + 1, len(b))
        for j in range(lo, hi):
            if not bm[j] and ca == b[j]:
                am[i] = bm[j] = True
                m += 1
                break
    if m == 0:
        return 0.0
    k = t = 0
    for i, ca in enumerate(a):
        if am[i]:
            while not bm[k]:
                k += 1
            if ca != b[k]:
                t += 1
            k += 1
    jaro = (m / len(a) + m / len(b) + (m - t / 2) / m) / 3
    prefix = 0
    for x, y in zip(a[:4], b[:4]):
        if x != y:
            break
        prefix += 1
    return jaro + prefix * 0.1 * (1 - jaro)


# Must match REGULATORY_THRESHOLD and REGULATORY_SPELLING in vocab.rs.
REG_THRESHOLD = 0.90
REG_SPELLING = 0.80


def score(heard: str, term: str) -> tuple[float, float]:
    spelling = jaro_winkler(heard, term)
    return (spelling + jaro_winkler(sound_key(heard), sound_key(term))) / 2, spelling


def confusables(term: str, words_by_key: dict[str, list[str]]) -> list[str]:
    """Words the engine would hint as the register name `term` (in drug context)."""
    out = []
    for w in words_by_key.get(sound_key(term)[:1], []):
        if abs(len(w) - len(term)) > max(2, len(term) // 3) or w == term:
            continue
        s, spelling = score(w, term)
        if spelling >= REG_SPELLING and s >= REG_THRESHOLD:
            out.append(w)
    return out


def confusable(term: str, words_by_key: dict[str, list[str]]) -> str | None:
    return next(iter(confusables(term, words_by_key)), None)


def ref_words() -> set[str]:
    out: set[str] = set()
    for name in os.listdir(REFS):
        if name.endswith(".ref.json"):
            with open(os.path.join(REFS, name), encoding="utf-8") as f:
                for line in json.load(f):
                    out.update(re.findall(r"[a-z']+", line["text"].lower()))
    return out


def read_tsv_terms(path: str) -> set[str]:
    return set(read_tsv_kinds(path))


def read_tsv_kinds(path: str) -> dict[str, str]:
    out = {}
    with open(path, encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if line and not line.startswith("#"):
                cols = line.split("\t")
                out[cols[0].strip().lower()] = cols[1].strip() if len(cols) > 1 else "term"
    return out


def engine_common() -> set[str]:
    """The engine's COMMON list (words never hinted), read from vocab.rs."""
    with open(VOCAB_RS, encoding="utf-8") as f:
        block = re.search(r"const COMMON: &\[&str\] = &\[(.*?)\];", f.read(), re.S)
    return set(re.findall(r'"([^"]+)"', block.group(1)))


# Must match THRESHOLD_DRUG, THRESHOLD_OTHER and SPELLING_FLOOR in vocab.rs.
THRESHOLD_DRUG = 0.87
THRESHOLD_OTHER = 0.95
SPELLING_FLOOR = 0.75


def everyday_lookalikes(common: set[str], builtin: dict[str, str], known: set[str]) -> list[tuple[str, str]]:
    """(everyday word, hand-written term) pairs the engine would hint by
    spelling (it compares every hand-written term, whatever its length)."""
    never = engine_common()
    words = [
        w
        for w in common
        if w.isalpha() and len(w) >= 5 and w not in never and w not in known and w.rstrip("s") not in known
    ]
    keys = {w: sound_key(w) for w in words}
    out = []
    for term, kind in builtin.items():
        if " " in term or "-" in term or "'" in term:
            continue
        bar = THRESHOLD_DRUG if kind in ("drug", "vaccine") else THRESHOLD_OTHER
        term_key = sound_key(term)
        for w in words:
            spelling = jaro_winkler(w, term)
            # The score is the mean of spelling and sound, each at most 1.
            if spelling < SPELLING_FLOOR or (spelling + 1) / 2 < bar:
                continue
            s = (spelling + jaro_winkler(keys[w], term_key)) / 2
            if spelling >= SPELLING_FLOOR and s >= bar:
                out.append((w, term))
    return sorted(out)


# ------------------------------------------------------------------- main


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--cache", required=True, help="folder for the downloads (reused when present)")
    ap.add_argument("--xlrd", help="folder that contains the xlrd package, if it isn't installed")
    ap.add_argument("--out", default=OUT)
    ap.add_argument("--no-ema", action="store_true", help="leave out the EMA brand names")
    ap.add_argument("--report", action="store_true")
    args = ap.parse_args()
    if args.xlrd:
        sys.path.insert(0, os.path.abspath(args.xlrd))
    import xlrd  # noqa: E402  (legacy .xls reader, FDA files only)

    cache = os.path.abspath(args.cache)
    os.makedirs(cache, exist_ok=True)
    if os.path.commonpath([cache, ROOT]) == ROOT:
        raise SystemExit("--cache must be outside the repository")
    files = {name: fetch(cache, name, url) for name, url in DOWNLOADS.items() if not (args.no_ema and "ema" in name)}
    apvma = fetch_apvma(cache)

    english, common = read_scowl(files["scowl.tar.gz"])
    products: list[Product] = []
    products += read_fda(files["fda-trade.xls"], files["fda-ingredients.xls"], xlrd)
    products += read_vmd(files["vmd.xlsx"])
    products += read_dpd(files["dpd-allfiles.zip"])
    products += read_apvma(apvma)
    if not args.no_ema:
        products += read_ema(files["ema-medicines.xlsx"])

    terms: dict[str, dict] = {}
    seen = collections.Counter()
    for source, name, holder, actives, vaccine in products:
        seen[source] += 1
        b = brand(name, holder, english)
        if b:
            t = terms.setdefault(b, {"kind": "vaccine" if vaccine else "drug", "sources": set(), "brand": True})
            t["sources"].add(source)
            if not vaccine:
                t["kind"] = "drug"
        if vaccine:
            continue
        for a in actives:
            ing = ingredient(a, english)
            if ing:
                t = terms.setdefault(ing, {"kind": "drug", "sources": set(), "brand": False})
                t["sources"].add(source)
                t["kind"] = "drug"

    removed: dict[str, list[str]] = collections.defaultdict(list)
    builtin = read_tsv_terms(BUILTIN)

    def index(words, known) -> dict[str, list[str]]:
        # Words the engine could hint: five letters or more and not a known
        # term (a heard term is never hinted), by the first letter of their
        # sound key, as the engine looks register names up.
        out: dict[str, list[str]] = collections.defaultdict(list)
        for w in words:
            if w.isalpha() and len(w) >= 5 and w not in known and w.rstrip("s") not in known:
                out[sound_key(w)[:1]].append(w)
        return out

    said_by_key = index(ref_words(), builtin | set(terms))
    for term in sorted(terms):
        if term in builtin:
            removed["already in veterinary.tsv"].append(term)
        elif term in DENY and term not in ALLOW:
            removed["deny list"].append(term)
        elif (w := confusable(term, said_by_key)) is not None:
            # A word said in a test consult would be hinted as this name.
            removed["confusable with a test-consult word"].append(f"{term}~{w}")
        else:
            continue
        del terms[term]

    today = datetime.date.today().isoformat()
    lines = [
        "# OpenBark regulatory drug names: term<TAB>kind<TAB>sources. Generated by scripts/vocab-import.py; do not edit by hand.",
        f"# Retrieved {today}. Brand words and bare ingredient names only, deduplicated; see THIRD_PARTY_NOTICES.",
        "# us-fda-gb: US FDA Green Book via Animal Drugs @ FDA, public domain (US government work).",
        "# uk-vmd: VMD Product Information Database, Open Government Licence v3.0",
        "#   (https://www.nationalarchives.gov.uk/doc/open-government-licence/version/3/).",
        "# ca-dpd: Health Canada Drug Product Database, Open Government Licence - Canada",
        "#   (https://open.canada.ca/en/open-government-licence-canada).",
        "# au-apvma: APVMA PubCRIS dataset (data.gov.au), CC BY 3.0 AU (https://creativecommons.org/licenses/by/3.0/au/); changed: names extracted.",
    ]
    if not args.no_ema:
        lines.append("# eu-ema: Source: European Medicines Agency (EMA), medicines table, veterinary brand names only.")
    lines.append("# None of these organisations endorses OpenBark. Brand names are trade marks of their owners.")
    order = ["us-fda-gb", "uk-vmd", "ca-dpd", "au-apvma", "eu-ema"]
    for term in sorted(terms):
        t = terms[term]
        lines.append(f"{term}\t{t['kind']}\t{','.join(s for s in order if s in t['sources'])}")
    with open(args.out, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(lines) + "\n")

    # Everyday words that look like a term are never hinted by spelling
    # alone (the engine reads this list): "certainly" is not Cerenia, and
    # "rabbits" is not rabies. Real confusions go in mishearings.tsv.
    known = builtin | set(terms)
    builtin_kinds = read_tsv_kinds(BUILTIN)
    lookalikes = set(everyday_lookalikes(common, builtin_kinds, known))
    by_key = index(common, known)
    for term in terms:
        if " " not in term:
            lookalikes.update((w, term) for w in confusables(term, by_key))
    first: dict[str, str] = {}
    for w, t in sorted(lookalikes):
        first.setdefault(w, t)
    with open(EVERYDAY, "w", encoding="utf-8", newline="\n") as f:
        f.write(
            "# Everyday words that look like a drug or term name: word<TAB>term. Generated by scripts/vocab-import.py.\n"
            "# They are never hinted by spelling alone (\"certainly\" is not Cerenia); a real confusion\n"
            "# (\"apple\" for Apoquel) is listed in mishearings.tsv instead. Words: SCOWL size 35.\n"
        )
        f.write("".join(f"{w}\t{t}\n" for w, t in sorted(first.items())))
    print(f"everyday look-alikes: {len(first)} words")
    if args.report:
        by_term = collections.defaultdict(list)
        for w, t in lookalikes:
            by_term[t].append(w)
        for t, ws in sorted(by_term.items(), key=lambda x: -len(x[1])):
            print(f"  {t}: {' '.join(ws)}")

    per_source = collections.Counter(s for t in terms.values() for s in t["sources"])
    kinds = collections.Counter(t["kind"] for t in terms.values())
    brands = sum(1 for t in terms.values() if t["brand"])
    print(f"products read: {dict(seen)}")
    print(f"terms written: {len(terms)} ({brands} brand words, {len(terms) - brands} ingredients); kinds {dict(kinds)}")
    print(f"terms per source: {dict(per_source)}")
    for why, items in removed.items():
        print(f"removed ({why}): {len(items)}")
        if args.report:
            print("  " + " ".join(items))
    return 0


if __name__ == "__main__":
    sys.exit(main())
