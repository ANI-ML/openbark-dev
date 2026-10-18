import { Button } from "@openbark/ui";
import { useEffect, useState } from "react";
import * as engine from "../engine";
import { useT } from "../i18n";

export const ROLE_LABEL: Record<engine.Role, string> = {
  dvm: "Veterinarian",
  rvt: "Technician",
  reception: "Reception",
};

export interface UserPickerProps {
  onChosen: (user: engine.User) => void;
}

/** "Who's using OpenBark?": pick a team member, then their PIN (M1 item 11). */
export function UserPicker({ onChosen }: UserPickerProps) {
  const t = useT();
  const [users, setUsers] = useState<engine.User[] | null>(null);
  const [picked, setPicked] = useState<engine.User | null>(null);
  const [pin, setPin] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    engine.listUsers().then(setUsers, (e) => setError(String(e?.message ?? e)));
  }, []);

  async function submit() {
    if (!picked) return;
    setBusy(true);
    setError(null);
    try {
      onChosen(await engine.verifyUser(picked.id, pin));
    } catch {
      setError(t("That PIN isn't right."));
      setPin("");
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <div className="ob-backdrop" />
      <div className="dialog user-picker" role="dialog" aria-modal="true" aria-labelledby="who-title">
        <h2 id="who-title">{t("Who's using OpenBark?")}</h2>
        {!picked ? (
          <ul className="who-list">
            {(users ?? []).map((u) => (
              <li key={u.id}>
                <button type="button" className="who" onClick={() => setPicked(u)}>
                  <span className="who-name">{u.name}</span>
                  <span className="small muted">{t(ROLE_LABEL[u.role])}</span>
                </button>
              </li>
            ))}
          </ul>
        ) : (
          <form
            className="who-pin"
            onSubmit={(e) => {
              e.preventDefault();
              void submit();
            }}
          >
            <p>
              <strong>{picked.name}</strong> <span className="small muted">{t(ROLE_LABEL[picked.role])}</span>
            </p>
            <label className="ob-field narrow">
              <span className="label">{t("PIN")}</span>
              <input
                className="ob-input mono"
                type="password"
                inputMode="numeric"
                autoComplete="off"
                autoFocus
                maxLength={8}
                value={pin}
                onChange={(e) => setPin(e.target.value.replace(/\D/g, ""))}
              />
            </label>
            <div className="ask-actions">
              <Button type="submit" variant="primary" disabled={busy || pin.length < 4}>
                {t("Continue")}
              </Button>
              <Button
                variant="ghost"
                onClick={() => {
                  setPicked(null);
                  setPin("");
                  setError(null);
                }}
              >
                {t("Someone else")}
              </Button>
            </div>
          </form>
        )}
        {error && (
          <p className="ob-error" role="alert">
            {error}
          </p>
        )}
      </div>
    </>
  );
}
