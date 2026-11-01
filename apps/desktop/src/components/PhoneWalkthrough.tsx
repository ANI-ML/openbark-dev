import { Button, Mark } from "@openbark/ui";
import { Check, Lock, MemoryStick, Mic, RefreshCw } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useApp, type Settings } from "../context";
import * as engine from "../engine";
import { useT } from "../i18n";
import { isIos, noPhoneDrafting, phoneCanDraft } from "../visits";
import { ConsentCheck, ConsentText } from "./ConsentNotice";
import { PhoneDrafting } from "./PhoneDrafting";

const STEPS = 5;

/**
 * Whether the iPhone's first-run walkthrough shows: on the iPhone profile,
 * on a true first run (nothing accepted yet, never finished), or when the
 * person asked to see it again from Settings. Never on a computer.
 */
export function walkthroughDue(status: engine.Status | null, s: Settings): boolean {
  if (!isIos(status)) return false;
  return s.showWalkthrough || (!s.walkthroughDoneAt && !s.consentAcknowledgedAt);
}

type Mic = "idle" | "asking" | "ready" | "blocked";

/**
 * Ask for the microphone, just in time (Apple's guidance): open it briefly
 * and close it. In the app the engine opens it, which is when iOS asks; in a
 * browser demo, the browser asks.
 */
async function askForMicrophone(): Promise<void> {
  if (engine.isDesktop) {
    const started = await engine.micTestStart(null);
    await new Promise((r) => setTimeout(r, 800));
    await engine.micTestStop(started.test);
    return;
  }
  const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
  stream.getTracks().forEach((track) => track.stop());
}

/**
 * The iPhone's first-run walkthrough (owner request, 2026-10-25; Apple's
 * onboarding guidance): four short screens, skippable from every one, each
 * teaching by doing. 1. What OpenBark does here and its privacy promise.
 * 2. Recording and consent, with the microphone asked for here and only
 * here. 3. What this iPhone can do: write notes (8 GB or more) or record and
 * transcribe while a computer writes the note, and why (ADR-0034). 4. Notes
 * on this iPhone: the speech models are built in, and the drafting model is
 * offered. 5. The clinic computer: where encrypted sync is set up later.
 * Then Record. Shown once; Settings, Help shows it again.
 */
export function PhoneWalkthrough({ onDone }: { onDone?: () => void } = {}) {
  const app = useApp();
  const t = useT();
  const [step, setStep] = useState(0);
  const [agreed, setAgreed] = useState(Boolean(app.settings.consentAcknowledgedAt));
  const [mic, setMic] = useState<Mic>("idle");
  const heading = useRef<HTMLHeadingElement>(null);

  // Once started, it stays until finished or skipped, even after the consent
  // step is accepted (which alone would end a first run), and after a restart.
  useEffect(() => {
    if (!app.settings.showWalkthrough) app.setSettings({ showWalkthrough: true });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Each step's title takes focus, so VoiceOver reads the new screen.
  useEffect(() => {
    heading.current?.focus();
  }, [step]);

  const speech = app.status?.models ?? [];
  // The engine says the phone's memory in its summary (ADR-0018 section 10).
  const ramGb =
    app.status?.hardware?.summary?.match(/(\d+)\s*GB/)?.[1] ?? (app.status?.mobile_tier ? String(app.status.mobile_tier) : null);
  const tierOk = (app.status?.mobile_tier ?? 0) >= 8;
  const speechBuiltIn = speech.length > 0 && speech.every((m) => m.installed);

  function finish(to: "record" | "sync" = "record") {
    app.setSettings({ walkthroughDoneAt: new Date().toISOString(), showWalkthrough: false });
    onDone?.();
    app.navigate(to === "sync" ? { screen: "settings", section: "sync" } : { screen: "record" });
  }

  function next() {
    if (step === 1 && agreed && !app.settings.consentAcknowledgedAt) {
      app.setSettings({ consentAcknowledgedAt: new Date().toISOString() });
    }
    if (step + 1 >= STEPS) finish();
    else setStep(step + 1);
  }

  async function allowMicrophone() {
    setMic("asking");
    try {
      await askForMicrophone();
      setMic("ready");
    } catch {
      setMic("blocked");
    }
  }

  const titles = [
    t("Welcome to OpenBark"),
    t("Recording and consent"),
    t("What this iPhone can do"),
    t("Notes on this iPhone"),
    t("Use it with your clinic computer"),
  ];

  return (
    <div className="walkthrough" role="dialog" aria-modal="true" aria-labelledby="walkthrough-title">
      <header className="walkthrough-top">
        <span className="small muted" aria-live="polite">
          {t("Step {n} of {total}", { n: step + 1, total: STEPS })}
        </span>
        {step < STEPS - 1 && (
          <Button variant="ghost" className="walkthrough-skip" onClick={() => finish()}>
            {t("Skip")}
          </Button>
        )}
      </header>

      <div className="walkthrough-body" data-step={step + 1}>
        {step === 0 && <Mark size={72} className="walkthrough-mark" />}
        <h1 id="walkthrough-title" ref={heading} tabIndex={-1}>
          {titles[step]}
        </h1>

        {step === 0 && (
          <>
            <p>
              {/* What this iPhone does: drafting only on 8 GB and up, in a build with llama.cpp. */}
              {phoneCanDraft(app.status)
                ? t("OpenBark records the consult and writes the note, here on this iPhone.")
                : t("OpenBark records and transcribes the consult here on this iPhone. Your computer writes the note.")}
            </p>
            <p className="walkthrough-promise">
              <Lock size={20} aria-hidden />
              <span>{t("Recordings stay on this iPhone.")}</span>
            </p>
          </>
        )}

        {step === 1 && (
          <>
            <ConsentText />
            <ConsentCheck checked={agreed} onChange={setAgreed} />
            <div className="walkthrough-mic">
              {mic === "ready" ? (
                <p className="walkthrough-done" role="status">
                  <Check size={20} aria-hidden />
                  <span>{t("Microphone ready")}</span>
                </p>
              ) : (
                <Button onClick={() => void allowMicrophone()} disabled={mic === "asking"}>
                  <Mic size={20} aria-hidden />
                  {t("Allow microphone")}
                </Button>
              )}
              {mic === "blocked" && (
                <p className="small" role="alert">
                  {t("Microphone access is off. Turn it on in the iPhone's Settings, OpenBark, Microphone.")}
                </p>
              )}
            </div>
          </>
        )}

        {step === 2 && (
          <>
            <p className="walkthrough-promise" data-testid="phone-can-do">
              <MemoryStick size={20} aria-hidden />
              <span>
                {phoneCanDraft(app.status)
                  ? ramGb
                    ? t("This iPhone has {gb} GB of memory, so it can write notes as well as record and transcribe.", { gb: ramGb })
                    : t("This iPhone can write notes as well as record and transcribe.")
                  : tierOk
                    ? noPhoneDrafting(app.status, t)
                    : ramGb
                      ? t("This iPhone has {gb} GB of memory. It records and transcribes here, and your computer writes the note.", {
                          gb: ramGb,
                        })
                      : t("This iPhone records and transcribes here, and your computer writes the note.")}
              </span>
            </p>
            <p className="small muted">
              {phoneCanDraft(app.status)
                ? t(
                    "Writing a note needs a note model in memory alongside the speech models, so OpenBark offers it only on iPhones with 8 GB of memory or more. It's optional: your computer can write the notes instead.",
                  )
                : tierOk
                  ? t("Recording and transcribing work here now.")
                  : t(
                      "Writing a note needs a note model in memory alongside the speech models. On iPhones with less than 8 GB, every model OpenBark tested wrote doses that weren't said, so it isn't offered.",
                    )}
            </p>
          </>
        )}

        {step === 3 && (
          <>
            <p className="walkthrough-done">
              <Check size={20} aria-hidden />
              <span>
                {speechBuiltIn
                  ? t("The speech models are built in, so recording and transcribing work offline now.")
                  : t("The speech models aren't on this iPhone yet. Download them in Models.")}
              </span>
            </p>
            {phoneCanDraft(app.status) && (
              <p className="small muted">
                {t("Writing the note on this iPhone is optional. It needs one more model, downloaded once.")}
              </p>
            )}
            <ul className="inset-list">
              <PhoneDrafting walkthrough />
            </ul>
          </>
        )}

        {step === 4 && (
          <>
            <p className="walkthrough-promise">
              <RefreshCw size={20} aria-hidden />
              <span>
                {t(
                  "When your clinic sets up encrypted sync, transcripts and notes reach the clinic computer, encrypted. Recordings stay on this iPhone. It's in Settings, Encrypted sync.",
                )}
              </span>
            </p>
            {/* How to set it up, in three steps (Help, Set up encrypted sync, has the rest). */}
            <ol className="help-steps walkthrough-sync-steps">
              <li>{t("On the clinic computer: Settings, Clinic hub. Turn it on and choose New join code.")}</li>
              <li>{t("On this iPhone, on the clinic's Wi-Fi: Settings, Encrypted sync, Join your clinic hub. Type the code, the address and the certificate check.")}</li>
              <li>{t("Keep the 24-word Recovery Kit it shows somewhere safe.")}</li>
            </ol>
          </>
        )}
      </div>

      <footer className="walkthrough-foot">
        {step === 0 && (
          <Button variant="primary" size="prominent" onClick={next}>
            {t("Get started")}
          </Button>
        )}
        {step === 1 && (
          <Button variant="primary" size="prominent" onClick={next} disabled={!agreed}>
            {t("Continue")}
          </Button>
        )}
        {step === 2 && (
          <Button variant="primary" size="prominent" onClick={next}>
            {t("Continue")}
          </Button>
        )}
        {step === 3 && (
          <>
            <Button variant="primary" size="prominent" onClick={next}>
              {t("Continue")}
            </Button>
            {phoneCanDraft(app.status) && (
              <Button
                variant="ghost"
                size="prominent"
                onClick={() => {
                  app.setSettings({ writeNotesHere: false });
                  next();
                }}
              >
                {t("Not now")}
              </Button>
            )}
          </>
        )}
        {step === 4 && (
          <>
            <Button variant="primary" size="prominent" onClick={() => finish()}>
              {t("Later")}
            </Button>
            <Button variant="ghost" size="prominent" onClick={() => finish("sync")}>
              {t("Open Encrypted sync")}
            </Button>
          </>
        )}
      </footer>
    </div>
  );
}
