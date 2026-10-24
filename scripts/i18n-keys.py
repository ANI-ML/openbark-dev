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
    "Bullet points", "Text box", "Sentences that read as a paragraph, each linked to where it was said", "Measurements", "Medications", "Subheadings",
    "Bullet points, each linked to where it was said", "Drug, dose, route, frequency, duration",
    "Record", "Patients", "Templates", "Models", "Settings", "This device", "Web, this computer only",
    # Redesign (ADR-0027): App NAV, Visits WHEN, Settings TITLE
    "Visits", "Help", "Today", "This week", "Earlier",
    "You and your team", "Recording and consent", "Language and appearance", "Phone calls", "PDFs and copying",
    "Encrypted sync", "Privacy and storage", "Deleted visits", "About",
    # @openbark/ui ThemeToggle (translated through its `translate` prop)
    "Theme", "System", "Light", "Dark",
    # UserPicker ROLE_LABEL
    "Veterinarian", "Technician", "Reception",
    # Built-in template names (templateName() in src/i18n.ts)
    "SOAP note", "Phone call", "Annual exam", "New consult", "Recheck", "Results", "Clinician rounds", "Discharge instructions", "Client summary",
    "Emergency and triage", "Dental procedure", "Surgery and anaesthesia", "Dermatology consult", "Oncology visit", "Exotics consult", "Equine visit", "Production animal visit", "Euthanasia and quality of life", "Estimate explanation", "Euthanasia aftercare",
    # species.ts SPECIES (one, many) and SHOW_FOR
    "Dog", "Cat", "Horse", "Cattle", "Sheep or goat", "Pig", "Bird", "Reptile", "Rabbit", "Small rodent", "Ferret", "dogs", "cats", "horses", "cattle", "sheep and goats", "pigs", "birds", "reptiles", "rabbits", "small rodents", "ferrets", "for every species", "only for dogs", "only for cats", "only for horses", "only for cattle", "only for sheep and goats", "only for pigs", "only for birds", "only for reptiles", "only for rabbits", "only for small rodents", "only for ferrets", "only for dogs and cats", "only for farm animals", "only for exotics",
    # Templates screen groups
    "Your team's templates", "Common visits", "Specialty and other notes", "For the client",
    # promptExamples NOTE_EXAMPLES use cases
    "Small-animal general practice", "Emergency and critical care", "Feline-only clinic", "Equine ambulatory", "Exotics", "Farm and production animals", "Shelter medicine", "Referral specialty",
    # Engine messages about deleting (os-app, os-engine trash; ADR-0022), shown
    # through t(error) in Review and Settings. Messages with a name or a date
    # in them stay in English.
    "Add your name in Settings, then restore the visit.", "Add your name in Settings, then delete the recording.",
    "This visit is being drafted. Delete it when that's finished.",
    "This visit is still being transcribed. Delete it when that's finished.",
    "This visit is still recording. Stop the recording first.",
    "This visit is being saved in another window. Try again in a moment.", "This visit is being deleted.",
    "Another program is using a file in this visit. Close any window showing it, then try again.",
    "This visit was deleted. Restore it from Deleted visits in Settings.",
    "That deleted visit is no longer there. It may have been restored or removed for good.",
    "This visit has no recording.",
    # Template versions (ADR-0024): the core rules in plain words and the checks
    # after drafting (os-engine template.rs, the Always applied panel), Templates
    # LINT_REASON, LANGUAGE_NAME and group titles, CompareSheet FIELD_LABEL, and
    # engine messages shown through t(error).
    "Only what was said. No findings, diagnoses, vitals, doses or plans that weren't said.",
    "Anything not discussed shows as not discussed. Never \"normal\" unless it was said.",
    "Numbers are copied exactly as said, with their units.",
    "Patient details only if someone said them. The species is never guessed.",
    "Every fact points to the line where it was said.",
    "Speech-recognition hints never add a drug on their own.",
    "Every line links to the recording.",
    "One clinical fact or action per line.",
    "After drafting, every number and every line is checked against the recording.",
    "A number that isn't in the recording blocks signing until it's resolved.",
    "A note stays a draft until a veterinarian signs it.",
    "Findings that weren't said", "Numbers that weren't said", "Doses that weren't said",
    "It would take away \"Not discussed\"", "Inventing", "Every line keeps its link to the recording",
    "French", "Spanish", "German", "Portuguese",
    "Only you", "Hospital templates", "Your templates",
    "Name", "Description", "Icon", "Level of detail", "Who it's for", "Instructions", "Extra rules", "Rules that always apply",
    "Templates are shared by the hospital, so only an admin can change them.",
    "Changing the hospital's templates needs an admin. Ask one to approve with their PIN.",
    "Only an admin can approve this.",
    "This template changed while you were editing. Your changes are still here. Compare, then save again.",
    "Who it's for is fixed for this visit type. Duplicate the template to make one for the client.",
    "Fixed rules can't be changed. They come from the original template.",
    "You're already using the original.",
    "Keep at least one admin on the team. Make someone else an admin first.",
    "Set up a team first. Working alone, your templates are all yours.",
    "Standards for yourself are off while templates are shared by the hospital.",
    "Standards can be turned off for yourself when templates are each person's own.",
    "That reset can't be undone any more.",
    "That reset was already undone.",
    "This template changed after it was reset, so the reset can't be undone.",
    "Only you use this template, so there's no original to go back to. Delete it instead.",
    "This template has no original to go back to. Delete it instead.",
    "This is your version of a template. Reset it to the original instead.",
    "This is an edited built-in template. Reset it to the built-in instead.",
    "There's no original to compare with.",
    "There's no copy of the original from when this version was made.",
    "Only you use this template. Share it with the hospital instead.",
    "That PIN isn't right.", "Choose who you are first.",
    # Tooth chart confirmation (ADR-0021 decision 10): engine messages shown through t(error).
    "Add your name in Settings, then confirm the chart.",
    "This note is signed, so its chart can't change. Add an addendum instead.",
    # Phone calls (ADR-0023): the phone window's errors and outcomes
    # (src/phone/errors.ts, callState.ts), Review's call line, and engine
    # messages (os-app phone.rs, os-connect) shown through t(error) or t(message).
    "Outgoing", "Incoming", "Vet", "Tech", "Clinic", "Call saved",
    "No answer.", "The call dropped. What was recorded is saved.",
    "Your Twilio account is a trial, and trials can't connect calls through OpenBark. Upgrade it in your Twilio console.",
    "The call connected but no sound is getting through. Your network may be blocking Twilio's call audio. Ask your IT provider to allow UDP to Twilio (see the setup guide).",
    "Twilio blocked a call to this country. Turn it on in your Twilio console under Voice > Settings > Geo permissions.",
    "OpenBark can't use the microphone. Allow it in your computer's privacy settings, then try again.",
    "Can't reach Twilio. Check this computer's internet connection.",
    "Twilio refused the call: your account needs attention. Check billing in your Twilio console.",
    "This browser can't make calls. Use a current version of Chrome, Edge, Firefox or Safari.",
    "Twilio didn't accept this key. Check the SID and secret, or make a new key in your Twilio console.",
    "Twilio refused the request: your account needs attention. Check billing in your Twilio console.",
    "Twilio accepts this computer's key.", "The OpenBark TwiML App uses your outgoing Bin.",
    "The phone is on on this computer.", "The phone is off on this computer. Turn it on to make calls.",
    "Real calls need an upgraded Twilio account. Trials can't connect calls through OpenBark.",
    "Also delete this key in your Twilio console, so it can't be used anywhere else.",
    "Only a veterinarian can connect Twilio or change the phone settings.",
    "Connect Twilio before turning the phone on.",
    "The phone is off on this computer. Turn it on in Settings > Phone calls.",
    "Twilio isn't connected on this computer. A veterinarian can connect it in Settings > Phone calls.",
    "Write the announcement. Recorded calls always start with it.", "Paste the API key secret.",
    "The OpenBark TwiML App was deleted in your Twilio console. Choose Connect again to make it.",
    "The OpenBark TwiML App no longer uses your outgoing Bin. Choose Connect again to fix it.",
    "The phone's sign-in with Twilio ran out. Close this window and call again.",
    "Twilio didn't allow this call. In Settings > Phone calls, choose Check setup to see why.",
    "Twilio couldn't find OpenBark's TwiML App. In Settings > Phone calls, choose Change, then Connect.",
    "Twilio is getting too many requests from this account. Wait a minute and try again.",
    "The line is busy.", "The call was declined.", "That number couldn't be reached. Check it and try again.",
    "Read the announcement to the client first.", "The recording of this call was already deleted.",
]


STRING = re.compile(r'"((?:[^"\\]|\\.)*)"')


def model_guide() -> set[str]:
    """Plain-language model help (meanings, legend, starting points), rendered with t()."""
    path = os.path.join(ROOT, "modelGuide.ts")
    with io.open(path, encoding="utf-8") as f:
        src = f.read()
    # Every sentence-like string literal: meanings, legend text, starting points.
    out = set()
    for m in STRING.finditer(src):
        text = json.loads(f'"{m.group(1)}"')
        sentence = text[:1].isupper() or text.startswith("{")
        acronym = len(text) > 1 and text[0].isupper() and text[1].isupper()
        if " " in text and sentence and not acronym:
            out.add(text)
    return out


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


CHART = os.path.join(os.path.dirname(__file__), "..", "packages", "dental", "chart.json")


def dental_chart() -> set[str]:
    """The tooth chart's words (ADR-0021), rendered with t() from packages/dental/chart.json
    (finding labels, words and legend lines, detail words) and src/dental.ts (tooth names),
    plus the chart editor's GROUPS, DETAIL_LABEL and GRADE_LABEL and the builder's kind."""
    with io.open(CHART, encoding="utf-8") as f:
        chart = json.load(f)
    out = set()
    for finding in chart["findings"]:
        for key in ("label", "words", "words_grade", "words_mm", "legend"):
            if finding.get(key):
                out.add(finding[key])
        for detail in finding["details"]:
            out.add(detail["words"])
    for side in ("right", "left"):
        for jaw in ("maxillary", "mandibular"):
            out.add(f"{side} {jaw} {{tooth}}")
    out |= {
        "deciduous {tooth}", "canine",
        "first incisor", "second incisor", "third incisor",
        "first premolar", "second premolar", "third premolar", "fourth premolar",
        "first molar", "second molar", "third molar",
        "Normal or missing", "Fracture", "Periodontal", "Resorption", "Eruption", "Tooth condition", "Treatment", "Other",
        "Type", "Version", "Technique", "Stage", "Grade",
        "Tooth chart", "Teeth by Triadan number, for dogs and cats. Filled from what was said; teeth not mentioned stay blank.",
        "A template can have one tooth chart.",
    }
    return out


ENGINE = os.path.join(os.path.dirname(__file__), "..", "engine")
RUST_ERROR = re.compile(r'#\[error\(\s*"((?:[^"\\]|\\.)*)"\s*\)\]')
APP_ERROR = re.compile(r'AppError\(\s*"((?:[^"\\]|\\.)*)"\.into\(\)\s*\)')


def microphone() -> set[str]:
    """Microphone errors shown through t(error) on the Record screen: the
    browser's (MIC_ERRORS in src/mic/errors.ts), the desktop capture's
    (os-asr capture.rs) and the microphone test's (os-app mic.rs)."""
    out = set()
    with io.open(os.path.join(ROOT, "mic", "errors.ts"), encoding="utf-8") as f:
        src = f.read()
    block = src[src.index("export const MIC_ERRORS"):src.index("} as const;")]
    for m in STRING.finditer(block):
        out.add(json.loads(f'"{m.group(1)}"'))
    with io.open(os.path.join(ENGINE, "os-asr", "src", "capture.rs"), encoding="utf-8") as f:
        out |= {json.loads(f'"{m.group(1)}"') for m in RUST_ERROR.finditer(f.read())}
    with io.open(os.path.join(ENGINE, "os-app", "src", "mic.rs"), encoding="utf-8") as f:
        out |= {json.loads(f'"{m.group(1)}"') for m in APP_ERROR.finditer(f.read()) if "poisoned" not in m.group(1)}
    out.add("This browser reported a microphone rate OpenBark can't use. Reload the page and try again, or use another browser.")
    return out


def main() -> None:
    keys = set(INDIRECT) | help_topics() | model_guide() | dental_chart() | microphone()
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
