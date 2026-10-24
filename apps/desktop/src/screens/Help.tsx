import { Button } from "@openbark/ui";
import { ArrowLeft, Maximize, Pause, Play } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useApp } from "../context";
import { useT } from "../i18n";

/**
 * Whether the app is showing its dark theme: the explicit choice on <html>
 * (data-theme), else the system's prefers-color-scheme. Follows changes to either.
 */
function useDarkTheme(): boolean {
  const read = () => {
    const chosen = document.documentElement.getAttribute("data-theme");
    if (chosen === "dark") return true;
    if (chosen === "light") return false;
    return typeof window.matchMedia === "function" && window.matchMedia("(prefers-color-scheme: dark)").matches;
  };
  const [dark, setDark] = useState(read);
  useEffect(() => {
    const update = () => setDark(read());
    const attrs = new MutationObserver(update);
    attrs.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
    const media = typeof window.matchMedia === "function" ? window.matchMedia("(prefers-color-scheme: dark)") : null;
    media?.addEventListener?.("change", update);
    return () => {
      attrs.disconnect();
      media?.removeEventListener?.("change", update);
    };
  }, []);
  return dark;
}

/** 1:24 */
function clock(seconds: number): string {
  const s = Number.isFinite(seconds) ? Math.max(0, Math.floor(seconds)) : 0;
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

/**
 * A walkthrough video with a flat play and scrub bar. Chromium's built-in
 * controls draw a gradient over the frame (§3.5), so they're not used. Each
 * walkthrough is recorded in both themes (<id>.webm and <id>-dark.webm) and
 * plays in the one the app is showing (§3.4); if the dark recording is
 * missing, the light one plays. Until it's first played, a solid surface
 * covers the frame.
 */
function HelpVideo({ id, title }: { id: string; title: string }) {
  const t = useT();
  const video = useRef<HTMLVideoElement>(null);
  const dark = useDarkTheme();
  const [darkMissing, setDarkMissing] = useState(false);
  const src = dark && !darkMissing ? `help/${id}-dark.webm` : `help/${id}.webm`;
  const [started, setStarted] = useState(false);
  const [playing, setPlaying] = useState(false);
  const [time, setTime] = useState(0);
  const [duration, setDuration] = useState(0);

  const toggle = () => {
    const v = video.current;
    if (!v) return;
    if (v.paused || v.ended) void v.play().catch(() => {});
    else v.pause();
  };

  return (
    <figure className="help-player" data-started={started || undefined}>
      <div className="help-frame">
        <video
          ref={video}
          className="help-video"
          src={src}
          preload="metadata"
          playsInline
          aria-label={title}
          onClick={toggle}
          onPlay={() => {
            setStarted(true);
            setPlaying(true);
          }}
          onPause={() => setPlaying(false)}
          onEnded={() => setPlaying(false)}
          onError={() => {
            if (dark && !darkMissing) setDarkMissing(true);
          }}
          onLoadedMetadata={(e) => setDuration(e.currentTarget.duration)}
          onDurationChange={(e) => setDuration(e.currentTarget.duration)}
          onTimeUpdate={(e) => setTime(e.currentTarget.currentTime)}
        />
        {!started && (
          <div className="help-cover">
            <Button variant="primary" onClick={toggle}>
              <Play size={20} aria-hidden />
              {t("Play video")}
            </Button>
            {duration > 0 && <span className="mono faint">{clock(duration)}</span>}
          </div>
        )}
      </div>
      <div className="help-controls">
        <Button
          variant="ghost"
          size="compact"
          className="help-control"
          onClick={toggle}
          aria-label={playing ? t("Pause video") : t("Play video")}
        >
          {playing ? <Pause size={20} aria-hidden /> : <Play size={20} aria-hidden />}
        </Button>
        <input
          type="range"
          className="help-scrub"
          min={0}
          max={duration || 0}
          step={0.1}
          value={Math.min(time, duration || 0)}
          disabled={!duration}
          aria-label={t("Video position")}
          aria-valuetext={`${clock(time)} / ${clock(duration)}`}
          onChange={(e) => {
            const v = video.current;
            if (!v) return;
            v.currentTime = Number(e.target.value);
            setTime(v.currentTime);
          }}
        />
        <span className="mono faint help-time">{`${clock(time)} / ${clock(duration)}`}</span>
        <Button
          variant="ghost"
          size="compact"
          className="help-control"
          onClick={() => void video.current?.requestFullscreen?.().catch(() => {})}
          aria-label={t("Full screen")}
        >
          <Maximize size={20} aria-hidden />
        </Button>
      </div>
    </figure>
  );
}

/**
 * Help: short walkthrough videos of the real app (recorded by
 * e2e/help-videos.mjs) with the same steps written out. Videos ship with
 * the app (public/help), so help works offline.
 */
/** Steps that name the device, as the iPhone profile says them (ADR-0027). */
const IPHONE_STEP: Record<string, string> = {
  "If the models aren't on this computer yet, choose Download everything on the Record screen. It's a one-time download; after that OpenBark works offline.":
    "If the models aren't on this iPhone yet, choose Download everything on the Record screen. It's a one-time download; after that OpenBark works offline.",
  "Recordings, transcripts and notes are encrypted on this computer. Show the recovery key once and keep it somewhere safe.":
    "Recordings, transcripts and notes are encrypted on this iPhone. Show the recovery key once and keep it somewhere safe.",
};

export const HELP_TOPICS = [
  {
    id: "getting-started",
    title: "Getting started",
    summary: "The one-time consent notice, the models, and where everything is.",
    steps: [
      "Read the consent notice and accept it. You're responsible for getting each client's consent before recording.",
      "If the models aren't on this computer yet, choose Download everything on the Record screen. It's a one-time download; after that OpenBark works offline.",
      "Use the side menu: Record, Visits, Patients and Templates, then Models, Settings and Help. On a phone the tabs are Record, Visits, Patients and Settings, and Templates, Models and Help are in Settings.",
      "In Settings, add your name for signing, and choose the app language and the language notes are written in.",
      "In Settings, Privacy and storage, choose where visits and models are kept, such as another drive.",
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
      "Visits lists every recording: what still needs you first, then what is signed.",
      "Edit details corrects the name or species. Past visits keep what was entered at the time.",
    ],
  },
  {
    id: "record",
    title: "Recording a consult",
    summary: "Enter the patient, press Record, and watch the transcript appear.",
    steps: [
      "On Record, choose Find a patient to pick an animal you've seen before, or enter a new patient's name, the client's last name and the reason for the visit. They're optional and can be added later.",
      "Choose the template, then press Record or Space to start.",
      "Not sure the microphone is working? Choose Change under the Record button, press Test microphone and speak: the bar should move. If the microphone is unplugged while you record, recording moves to another one or pauses until one is back, and the screen says so.",
      "Each sentence appears in the live transcript once it's finished. The audio is saved, encrypted, as you go.",
      "Press the button again to stop. A progress bar shows the transcript being finished, then the note is drafted and opens in Review.",
      "Client on a phone on speaker? Tick Phone on speaker before you start, and put the phone between you and the computer with the volume up. Calling through OpenBark's own phone records the client more clearly still.",
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
  const topics = useRef<HTMLOListElement>(null);
  // Below 1024 px the topics are one scrolling strip: keep the open one in view.
  useEffect(() => {
    topics.current?.querySelector('[aria-current="true"]')?.scrollIntoView?.({ inline: "nearest", block: "nearest" });
  }, [open]);

  return (
    <div className="screen help">
      <header className="screen-head">
        <div className="head-title">
          {/* Phones reach Help from Settings: the way back (§9). */}
          <Button variant="ghost" size="compact" className="back-to-settings" onClick={() => app.navigate({ screen: "settings" })}>
            <ArrowLeft size={16} aria-hidden />
            {t("Settings")}
          </Button>
          <h1>{t("Help")}</h1>
        </div>
      </header>
      <div className="help-grid">
        <nav className="help-topics" aria-label={t("Help topics")}>
          <ol ref={topics}>
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
          <HelpVideo key={topic.id} id={topic.id} title={t(topic.title)} />
          {app.settings.uiLanguage !== "en" && (
            <p className="small faint">{t("The videos are in English; the steps below are in your language.")}</p>
          )}
          <ol className="help-steps">
            {topic.steps.map((s) => (
              <li key={s}>{t((app.status?.profile === "ios" && IPHONE_STEP[s]) || s)}</li>
            ))}
          </ol>
        </article>
      </div>
    </div>
  );
}
