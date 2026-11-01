import { Button } from "@openbark/ui";
import { useApp } from "../context";
import { useT } from "../i18n";
import { checkCardDue } from "../setup";

/**
 * For a computer set up before this version (ADR-0034): a one-time card on
 * Record offering "Check my setup" (this computer, the note model and the
 * speed check). It suggests and never changes a model by itself; Not now
 * dismisses it for good. Settings, Models and Help still run setup again.
 */
export function SetupCheckCard() {
  const app = useApp();
  const t = useT();
  if (!checkCardDue(app.status, app.settings)) return null;
  return (
    <div className="ob-notice setup-check-card" role="status">
      <p>
        {t(
          "New in this version: OpenBark can check this computer, recommend the note model that suits it, and measure how long a note takes here. Nothing changes unless you choose.",
        )}
      </p>
      <div className="ask-actions">
        <Button size="compact" onClick={() => app.setSettings({ showSetup: "check" })}>
          {t("Check my setup")}
        </Button>
        <Button
          size="compact"
          variant="ghost"
          onClick={() => app.setSettings({ setupCardDismissedAt: new Date().toISOString() })}
        >
          {t("Not now")}
        </Button>
      </div>
    </div>
  );
}

/**
 * Record on a computer with models still to download (setup skipped or
 * closed): what's missing, and a way back into setup.
 */
export function SetupPrompt() {
  const app = useApp();
  const t = useT();
  return (
    <section className="setup" aria-label={t("Set up OpenBark")}>
      <h2>{t("Download the models to start")}</h2>
      <p className="small muted measure">
        {t(
          "OpenBark needs its speech and note models once. Setup checks this computer, recommends the note model that suits it and downloads them.",
        )}
      </p>
      <div className="ask-actions">
        <Button variant="primary" onClick={() => app.setSettings({ showSetup: "full" })}>
          {t("Set up OpenBark")}
        </Button>
      </div>
    </section>
  );
}

/** "A long visit's note takes about 4 minutes here", from the last speed check of the chosen note model. */
export function useSpeedLine(): string | null {
  const app = useApp();
  const t = useT();
  const s = app.settings.speedCheck;
  if (!s || s.model !== app.settings.llm || s.noteSecs == null) return null;
  return s.noteSecs < 60
    ? t("On this computer, a long visit's note takes less than a minute (speed check).")
    : t("On this computer, a long visit's note takes about {n} minutes (speed check).", {
        n: Math.max(1, Math.round(s.noteSecs / 60)),
      });
}
