import { Button, Dialog } from "@openbark/ui";
import { useEffect, useState } from "react";
import { useApp } from "../context";
import * as engine from "../engine";
import { useT } from "../i18n";

/**
 * Share with the hospital (ADR-0024 section 14.4): a dialog, because it
 * changes other people's drafts. It says who is affected, offers making it
 * the hospital's template or adding it as a new one, and an admin approves
 * with their PIN (D2, D6). A wrong PIN keeps the dialog open.
 */
export function ShareDialog({
  name,
  hasOriginal,
  onShare,
  onCancel,
}: {
  name: string;
  /** A version of a built-in or hospital template (not one only you use). */
  hasOriginal: boolean;
  onShare: (mode: "replace" | "add", newName: string | null, approver: engine.Approver) => Promise<void>;
  onCancel: () => void;
}) {
  const app = useApp();
  const t = useT();
  const [mode, setMode] = useState<"replace" | "add">("replace");
  const [newName, setNewName] = useState(t("{name} (shared)", { name }));
  const [admins, setAdmins] = useState<engine.User[]>([]);
  const [who, setWho] = useState(app.user?.admin ? app.user.id : "");
  const [pin, setPin] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    engine.listUsers().then(
      (users) => setAdmins(users.filter((u) => u.admin)),
      (e) => setError(String(e?.message ?? e)),
    );
  }, []);

  async function submit() {
    setBusy(true);
    setError(null);
    try {
      await onShare(mode, mode === "add" ? newName.trim() || null : null, { id: who, pin });
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setPin("");
    } finally {
      setBusy(false);
    }
  }

  const me = app.user?.admin ? app.user : null;
  return (
    <Dialog labelledBy="share-title" onClose={onCancel} dismissible={!busy} className="share-dialog">
      <h2 id="share-title">{t("Share with the hospital")}</h2>
      <p className="small muted">
        {t("Everyone without their own version will draft with it. People with their own version keep it and are told the hospital's changed.")}
      </p>
      <form
        className="approve-form"
        onSubmit={(e) => {
          e.preventDefault();
          void submit();
        }}
      >
        <fieldset className="share-choices">
          <legend className="sr-only">{t("How to share")}</legend>
          <label className="check">
            <input type="radio" name="share-mode" checked={mode === "replace"} onChange={() => setMode("replace")} />
            <span>{hasOriginal ? t("Make it the hospital's {name}", { name }) : t("Share {name} with the hospital", { name })}</span>
          </label>
          <label className="check">
            <input type="radio" name="share-mode" checked={mode === "add"} onChange={() => setMode("add")} />
            <span>{t("Add it as a new template")}</span>
          </label>
        </fieldset>
        {mode === "add" && (
          <label className="ob-field">
            <span className="label">{t("Name of the new template")}</span>
            <input className="ob-input" value={newName} onChange={(e) => setNewName(e.target.value)} />
          </label>
        )}
        <p className="small">{me ? t("{name}, enter your PIN to approve.", { name: me.name }) : t("An admin approves sharing.")}</p>
        {!me && (
          <label className="ob-field">
            <span className="label">{t("Admin approving")}</span>
            <select className="ob-select" value={who} onChange={(e) => setWho(e.target.value)} disabled={busy}>
              <option value="">{t("Choose an admin")}</option>
              {admins.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.name}
                </option>
              ))}
            </select>
          </label>
        )}
        <label className="ob-field narrow">
          <span className="label">{t("Admin's PIN")}</span>
          <input
            className="ob-input mono"
            type="password"
            inputMode="numeric"
            autoComplete="off"
            maxLength={8}
            value={pin}
            disabled={busy}
            onChange={(e) => setPin(e.target.value.replace(/\D/g, ""))}
          />
        </label>
        {error && (
          <p className="ob-notice" data-tone="danger" role="alert">
            {t(error)}
          </p>
        )}
        <div className="dialog-actions">
          <Button type="submit" variant="primary" disabled={busy || !who || pin.length < 4}>
            {t("Share with the hospital")}
          </Button>
          <Button variant="ghost" onClick={onCancel} disabled={busy}>
            {t("Cancel")}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
