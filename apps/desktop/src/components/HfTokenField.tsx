import { Button } from "@openbark/ui";
import { useEffect, useId, useState } from "react";
import * as engine from "../engine";
import { useT } from "../i18n";

/**
 * The Hugging Face token, for gated models (ADR-0025 §8). Optional, kept in
 * this computer's keychain, sent only to huggingface.co and only for gated
 * repos. The token is never shown again once saved.
 */
export function HfTokenField({ onChange }: { onChange?: (s: engine.HfTokenStatus) => void }) {
  const t = useT();
  const id = useId();
  const [status, setStatus] = useState<engine.HfTokenStatus | null>(null);
  const [token, setToken] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    engine.hfTokenStatus().then(setStatus, () => setStatus({ saved: false, source: null, user: null }));
  }, []);

  async function run(call: () => Promise<engine.HfTokenStatus>) {
    setBusy(true);
    setError(null);
    try {
      const s = await call();
      setStatus(s);
      setToken("");
      onChange?.(s);
    } catch (e) {
      setError(engine.modelMessage(t, e));
    } finally {
      setBusy(false);
    }
  }

  const sourceLine: Record<NonNullable<engine.HfTokenStatus["source"]>, string> = {
    keychain: t("Saved in this computer's keychain."),
    vault_file: t("Saved, encrypted, on this computer."),
    session: t("Kept until OpenBark closes."),
    server: t("Set by the server."),
  };
  const server = status?.source === "server";

  return (
    <div className="hf-token">
      {status?.saved && status.source && (
        <p className="small muted" role="status">
          {sourceLine[status.source]}
          {status.user && ` ${t("Signed in as {name}.", { name: status.user })}`}
        </p>
      )}
      {!server && (
        <form
          className="hf-token-form"
          onSubmit={(e) => {
            e.preventDefault();
            const value = token.trim();
            if (value) void run(() => engine.setHfToken(value));
          }}
        >
          <label className="ob-field" htmlFor={id}>
            <span className="label">{t("Hugging Face token")}</span>
            <input
              id={id}
              className="ob-input mono"
              type="password"
              autoComplete="off"
              spellCheck={false}
              value={token}
              disabled={busy}
              placeholder={status?.saved ? "••••••••" : "hf_…"}
              onChange={(e) => setToken(e.target.value)}
            />
          </label>
          <div className="ask-actions">
            <Button type="submit" variant="primary" disabled={busy || !token.trim()}>
              {t("Save token")}
            </Button>
            {status?.saved && (
              <Button variant="ghost" disabled={busy} onClick={() => void run(engine.clearHfToken)}>
                {t("Remove token")}
              </Button>
            )}
          </div>
        </form>
      )}
      {!server && <p className="small faint">{t("A Read token is enough.")}</p>}
      {error && (
        <p className="ob-notice" data-tone="danger" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
