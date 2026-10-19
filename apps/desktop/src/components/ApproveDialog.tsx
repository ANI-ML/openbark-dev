import { Button, Dialog } from "@openbark/ui";
import { useEffect, useRef, useState } from "react";
import { useApp } from "../context";
import * as engine from "../engine";
import { useT } from "../i18n";

/**
 * An admin approves a change to the hospital's templates (ADR-0024 D2):
 * every such change needs an admin's PIN, checked by the engine in the same
 * call. The person using the app is picked when they're an admin; otherwise
 * they choose one, who enters their PIN here. The PIN lives only in this
 * dialog and the request body. A wrong PIN keeps the dialog open.
 */
export function ApproveDialog({
  title,
  body,
  action,
  onApprove,
  onCancel,
}: {
  title: string;
  body: string;
  /** The primary button's label, the outcome ("Save for the hospital"). */
  action: string;
  /** Runs the change with the approval; a rejection shows its message here. */
  onApprove: (approver: engine.Approver) => Promise<void>;
  onCancel: () => void;
}) {
  const app = useApp();
  const t = useT();
  const [admins, setAdmins] = useState<engine.User[]>([]);
  const [who, setWho] = useState<string>(app.user?.admin ? app.user.id : "");
  const [pin, setPin] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const pinRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    engine.listUsers().then(
      (users) => {
        const list = users.filter((u) => u.admin);
        setAdmins(list);
        setWho((w) => w || (list.length === 1 ? (list[0]?.id ?? "") : ""));
      },
      (e) => setError(String(e?.message ?? e)),
    );
    pinRef.current?.focus();
  }, []);

  async function submit() {
    if (!who || pin.length < 4) return;
    setBusy(true);
    setError(null);
    try {
      await onApprove({ id: who, pin });
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setPin("");
      pinRef.current?.focus();
    } finally {
      setBusy(false);
    }
  }

  const me = app.user?.admin ? app.user : null;
  return (
    <Dialog labelledBy="approve-title" onClose={onCancel} dismissible={!busy} className="approve-dialog">
      <h2 id="approve-title">{title}</h2>
      <p className="small muted">{body}</p>
      <form
        className="approve-form"
        onSubmit={(e) => {
          e.preventDefault();
          void submit();
        }}
      >
        {me ? (
          <p className="small">{t("{name}, enter your PIN to approve.", { name: me.name })}</p>
        ) : (
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
            ref={pinRef}
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
            {action}
          </Button>
          <Button variant="ghost" onClick={onCancel} disabled={busy}>
            {t("Cancel")}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
