"""List every interface string passed to t() in apps/desktop/src, for translation.

Usage: python scripts/i18n-keys.py > target/i18n-keys.json
Strings translated indirectly (labels kept in constants and rendered with
t(x.label)) are listed in INDIRECT; keep it in sync with the code.
"""
import io
import json
import os
import re
import sys

ROOT = os.path.join(os.path.dirname(__file__), "..", "apps", "desktop", "src")
CALL = re.compile(r'\bt\(\s*("(?:[^"\\]|\\.)*")')
INDIRECT = [
    # ConsentStep METHODS, RefinePanel SUGGESTIONS, HeadingEditor MED_LABELS
    "Agreed out loud just now", "Signed a consent form", "Consent on file for this client",
    "Only I consent (one-party rule)", "Add any details that were said but left out",
    "Give every value with its unit", "One finding per line",
    "Drug", "Dose", "Route", "Frequency", "Duration",
    # Models SLOT_LABEL / FIT, Templates KINDS / KIND_HINT, App NAV
    "Notes", "Transcription", "Speech detection", "Speakers", "Fits", "Tight", "Won't fit",
    "Bullet points", "Measurements", "Medications", "Subheadings",
    "Bullet points, each linked to where it was said", "Drug, dose, route, frequency, duration",
    "Record", "Encounters", "Templates", "Models", "Settings", "This device", "Web, this computer only",
    # @openbark/ui ThemeToggle (translated through its `translate` prop)
    "Theme", "System", "Light", "Dark",
    # UserPicker ROLE_LABEL
    "Veterinarian", "Technician", "Reception",
    # Built-in template names (templateName() in src/i18n.ts)
    "SOAP note", "Callback note", "Discharge instructions", "Client summary",
    "Wellness visit", "Sick visit", "Emergency and triage", "Dental procedure", "Surgery and anaesthesia", "Recheck visit", "Dermatology consult", "Oncology visit", "Exotics consult", "Equine visit", "Production animal visit", "Euthanasia and quality of life", "Estimate explanation", "Euthanasia aftercare",
    # species.ts SPECIES (one, many) and SHOW_FOR
    "Dog", "Cat", "Horse", "Cattle", "Sheep or goat", "Pig", "Bird", "Reptile", "Rabbit", "Small rodent", "Ferret", "dogs", "cats", "horses", "cattle", "sheep and goats", "pigs", "birds", "reptiles", "rabbits", "small rodents", "ferrets", "for every species", "only for dogs", "only for cats", "only for horses", "only for cattle", "only for sheep and goats", "only for pigs", "only for birds", "only for reptiles", "only for rabbits", "only for small rodents", "only for ferrets", "only for dogs and cats", "only for farm animals", "only for exotics",
    # promptExamples NOTE_EXAMPLES use cases
    "Small-animal general practice", "Emergency and critical care", "Feline-only clinic", "Equine ambulatory", "Exotics", "Farm and production animals", "Shelter medicine", "Referral specialty",
]


STRING = re.compile(r'"((?:[^"\\]|\\.)*)"')


def help_topics() -> set[str]:
    """Titles, summaries and steps of the Help screen (rendered with t())."""
    path = os.path.join(ROOT, "screens", "Help.tsx")
    with io.open(path, encoding="utf-8") as f:
        src = f.read()
    block = src[src.index("export const HELP_TOPICS"):src.index("] as const;")]
    out = set()
    for line in block.splitlines():
        if line.strip().startswith("id:"):
            continue
        for m in STRING.finditer(line):
            out.add(json.loads(f'"{m.group(1)}"'))
    return out


def main() -> None:
    keys = set(INDIRECT) | help_topics()
    for folder, _, files in os.walk(ROOT):
        for name in files:
            if not name.endswith((".ts", ".tsx")) or ".test." in name:
                continue
            with io.open(os.path.join(folder, name), encoding="utf-8") as f:
                for m in CALL.finditer(f.read()):
                    keys.add(json.loads(m.group(1)))
    keys.discard("")
    json.dump(sorted(keys), sys.stdout, ensure_ascii=False, indent=0)


if __name__ == "__main__":
    main()
