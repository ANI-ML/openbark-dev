import { Button } from "@openbark/ui";
import { useCallback, useEffect, useState } from "react";
import { useApp } from "../context";
import * as engine from "../engine";
import { useT } from "../i18n";
import { ROLE_LABEL } from "./UserPicker";

/** Settings, Team: who can record, write notes and sign (M1 item 11). */
export function TeamSettings() {
  const app = useApp();
  const t = useT();
  const [users, setUsers] = useState<engine.User[]>([]);
  const [name, setName] = useState("");
  const [role, setRole] = useState<engine.Role>("dvm");
  const [pin, setPin] = useState("");
  const [myPin, setMyPin] = useState("");
  const [error, setError] = useState<string | null>(null);
  const team = users.length > 0;

  const load = useCallback(() => engine.listUsers().then(setUsers, (e) => setError(String(e?.message ?? e))), []);
  useEffect(() => void load(), [load]);

  const fail = (e: unknown) => setError(e instanceof Error ? e.message : String(e));

  async function add() {
    setError(null);
    try {
      const u = await engine.addUser(name, team ? role : "dvm", pin, app.user?.id ?? null);
      setName("");
      setPin("");
      app.toast(t("{name} added to the team", { name: u.name }));
      await load();
      app.refreshStatus();
    } catch (e) {
      fail(e);
    }
  }

  async function remove(u: engine.User) {
    setError(null);
    try {
      await engine.removeUser(u.id, app.user?.id ?? null);
      app.toast(t("{name} removed from the team", { name: u.name }));
      await load();
      app.refreshStatus();
    } catch (e) {
      fail(e);
    }
  }

  async function changeMyPin() {
    if (!app.user) return;
    setError(null);
    try {
      await engine.setUserPin(app.user.id, myPin, app.user.id);
      setMyPin("");
      app.toast(t("PIN changed"));
    } catch (e) {
      fail(e);
    }
  }

  return (
    <section className="group">
      <h2>{t("Team")}</h2>
      <p className="small muted measure">
        {team
          ? t("Each person picks their name and enters their PIN. Veterinarians sign; technicians draft and edit; reception records and enters patient details. Every revision records who made it.")
          : t("Working alone? Leave this empty and use the name above. Add your team to record who recorded, edited and signed each note, and to keep signing to veterinarians.")}
      </p>
      {error && (
        <p className="ob-notice" data-tone="danger" role="alert">
          {error}
        </p>
      )}
      {team && (
        <ul className="team-list">
          {users.map((u) => (
            <li key={u.id}>
              <span>{u.name}</span>
              <span className="small muted">{t(ROLE_LABEL[u.role])}</span>
              {app.can.team && u.id !== app.user?.id && (
                <Button
                  variant="danger"
                  data-size="compact"
                  aria-label={t("Remove {name}", { name: u.name })}
                  onClick={() => void remove(u)}
                >
                  {t("Remove")}
                </Button>
              )}
            </li>
          ))}
        </ul>
      )}
      {(!team || app.can.team) && (
        <form
          className="team-add"
          onSubmit={(e) => {
            e.preventDefault();
            void add();
          }}
        >
          <label className="ob-field">
            <span className="label">{t("Name")}</span>
            <input className="ob-input" value={name} onChange={(e) => setName(e.target.value)} placeholder="Dr. A. Patel" />
          </label>
          <label className="ob-field">
            <span className="label">{t("Role")}</span>
            <select className="ob-select" value={team ? role : "dvm"} disabled={!team} onChange={(e) => setRole(e.target.value as engine.Role)}>
              {(Object.keys(ROLE_LABEL) as engine.Role[]).map((r) => (
                <option key={r} value={r}>
                  {t(ROLE_LABEL[r])}
                </option>
              ))}
            </select>
          </label>
          <label className="ob-field">
            <span className="label">{t("PIN (4 to 8 digits)")}</span>
            <input
              className="ob-input mono"
              type="password"
              inputMode="numeric"
              autoComplete="new-password"
              maxLength={8}
              value={pin}
              onChange={(e) => setPin(e.target.value.replace(/\D/g, ""))}
            />
          </label>
          <Button type="submit" disabled={!name.trim() || pin.length < 4}>
            {team ? t("Add to the team") : t("Add the first veterinarian")}
          </Button>
        </form>
      )}
      {app.user && (
        <form
          className="team-add"
          onSubmit={(e) => {
            e.preventDefault();
            void changeMyPin();
          }}
        >
          <label className="ob-field">
            <span className="label">{t("Your new PIN")}</span>
            <input
              className="ob-input mono"
              type="password"
              inputMode="numeric"
              autoComplete="new-password"
              maxLength={8}
              value={myPin}
              onChange={(e) => setMyPin(e.target.value.replace(/\D/g, ""))}
            />
          </label>
          <Button type="submit" variant="ghost" disabled={myPin.length < 4}>
            {t("Change my PIN")}
          </Button>
        </form>
      )}
    </section>
  );
}
