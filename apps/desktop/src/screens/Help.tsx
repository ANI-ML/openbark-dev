import { useState } from "react";
import { useApp } from "../context";
import { useT } from "../i18n";

/**
 * Help: short walkthrough videos of the real app (recorded by
 * e2e/help-videos.mjs) with the same steps written out. Videos ship with
 * the app (public/help), so help works offline.
 */
export const HELP_TOPICS = [
  {
    id: "getting-started",
    title: "Getting started",
    summary: "The one-time consent notice, the models, and where everything is.",
    steps: [
      "Read the consent notice and accept it. You're responsible for getting each client's consent before recording.",
      "If the models aren't on this computer yet, choose Download everything on the Record screen. It's a one-time download; after that OpenBark works offline.",
      "Use the sidebar: Record, Patients, Templates, Models, Settings and Help.",
      "In Settings, add your name for signing, and choose the app language and the language notes are written in.",
      "In Settings, Storage, choose where visits and models are kept, such as another drive.",
    ],
  },
  {
    id: "patients",
    title: "Patients and visits",
    summary: "Find an animal, see every visit, and start a new one from its page.",
    steps: [
      "Patients lists every animal, most recently seen first. Type a name to find one.",
      "Open a patient to see every visit, newest first, with its type and whether it's signed.",
      "New visit on a patient's page opens Record with the patient filled in.",
      "Recent visits shows the day's recordings as a list.",
      "Edit details corrects the name or species. Past visits keep what was entered at the time.",
    ],
  },
  {
    id: "record",
    title: "Recording a consult",
    summary: "Enter the patient, press Record, and watch the transcript appear.",
    steps: [
      "On Record, choose Find a patient to pick an animal you've seen before, or enter a new patient's name, the client's last name and the reason for the visit. They're optional and can be added later.",
      "Choose the kind of visit, then press the red button or Space to start.",
      "Each sentence appears in the live transcript once it's finished. The audio is saved, encrypted, as you go.",
      "Press the button again to stop. A progress bar shows the transcript being finished, then the note is drafted and opens in Review.",
      "You can also import a recording made elsewhere, such as a phone's call recording. A progress bar shows how far it has got and the time left.",
    ],
  },
  {
    id: "review",
    title: "Reviewing and signing",
    summary: "Check the draft against the recording, ask for changes, edit, and sign.",
    steps: [
      "Rest the pointer on any line of the note to light up the transcript lines it came from. Click it to hear that part of the recording.",
      "A red callout means a number isn't in the transcript: check the recording and correct it before signing. Amber underlines need a second look.",
      "Ask on a heading redrafts just that heading from the recording, following your request. Tick Save as a standard to make it the rule for that heading.",
      "Edit on a heading lets you type your own version. Changed lines are marked as yours.",
      "Sign note locks the note. After signing, add an addendum for anything that comes later.",
    ],
  },
  {
    id: "templates",
    title: "Templates",
    summary: "Pick one of the built-in templates for your kind of visit, or build your own from headings.",
    steps: [
      "Open Templates and choose Start from SOAP, or New template.",
      "Give each heading a title and choose what goes under it: bullet points, measurements, medications, or subheadings.",
      "Add plain-language instructions where it helps, or choose one of the Examples under the heading, such as Triadan numbers for teeth.",
      "Check the preview and What the model is told, then Save template.",
      "Use Try on a visit to draft a saved consult with the new template.",
    ],
  },
  {
    id: "documents",
    title: "Client documents, PDF and copying",
    summary: "Discharge instructions and summaries for the owner, PDFs and pasting into your PIMS.",
    steps: [
      "Under the note, choose Discharge instructions or Client summary. They're written in plain language and follow your corrected note.",
      "Read the document through, then Copy it or Save PDF.",
      "Copy note pastes the note into your PIMS. In Settings, choose plain text, formatted text or Markdown.",
      "Save PDF uses your clinic's letterhead from Settings.",
    ],
  },
  {
    id: "team",
    title: "Team and security",
    summary: "Who's using OpenBark, roles, and keeping recordings safe.",
    steps: [
      "In Settings, add your team: the first member is a veterinarian. Each person gets a PIN.",
      "Each person picks their name and enters their PIN. Veterinarians sign; technicians draft and edit; reception records.",
      "Every revision records who recorded, edited and signed it.",
      "Recordings, transcripts and notes are encrypted on this computer. Show the recovery key once and keep it somewhere safe.",
    ],
  },
] as const;

export function Help() {
  const app = useApp();
  const t = useT();
  const [open, setOpen] = useState<string>(HELP_TOPICS[0].id);
  const topic = HELP_TOPICS.find((x) => x.id === open) ?? HELP_TOPICS[0];

  return (
    <div className="screen help">
      <header className="screen-head">
        <h1>{t("Help")}</h1>
      </header>
      <div className="help-grid">
        <nav className="help-topics" aria-label={t("Help topics")}>
          <ol>
            {HELP_TOPICS.map((x, i) => (
              <li key={x.id}>
                <button
                  type="button"
                  className="help-topic"
                  aria-current={x.id === open ? "true" : undefined}
                  onClick={() => setOpen(x.id)}
                >
                  <span className="mono faint">{i + 1}</span>
                  <span>{t(x.title)}</span>
                </button>
              </li>
            ))}
          </ol>
        </nav>
        <article className="help-body">
          <h2>{t(topic.title)}</h2>
          <p className="muted">{t(topic.summary)}</p>
          <video
            key={topic.id}
            className="help-video"
            src={`help/${topic.id}.webm`}
            controls
            preload="metadata"
            playsInline
          />
          {app.settings.uiLanguage !== "en" && (
            <p className="small faint">{t("The videos are in English; the steps below are in your language.")}</p>
          )}
          <ol className="help-steps">
            {topic.steps.map((s) => (
              <li key={s}>{t(s)}</li>
            ))}
          </ol>
        </article>
      </div>
    </div>
  );
}
