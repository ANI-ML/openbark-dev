import { Button, StatusDot } from "@openbark/ui";
import { useCallback, useEffect, useState } from "react";
import { useApp } from "../context";
import * as engine from "../engine";
import { useT } from "../i18n";
import { ApproveDialog } from "./ApproveDialog";
import { ROLE_LABEL } from "./UserPicker";

/** An admin change waiting for a PIN. */
type Pending = { kind: "mode"; templates: "personal" | "hospital" } | { kind: "admin"; user: engine.User; admin: boolean };

/**
 * Settings, Team: who can record, write notes and sign (M1 item 11); who is
 * an admin, and whose templates the team drafts with (ADR-0024 D2).
 */
export function TeamSettings() {
  const app = useApp();
  const t = useT();
  const [users, setUsers] = useState<engine.User[]>([]);
  const [settings, setSettings] = useState<engine.TeamSettingsView | null>(null);
  const [name, setName] = useState("");
  const [role, setRole] = useState<engine.Role>("dvm");
  const [pin, setPin] = useState("");
  const [myPin, setMyPin] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState<Pending | null>(null);
  /** Removing someone: what their profile holds, for the confirmation. */
  const [removing, setRemoving] = useState<{ user: engine.User; summary: engine.ProfileSummary } | null>(null);
  /** Resetting someone's PIN in place (so nobody loses their versions to remove-and-re-add). */
  const [resetting, setResetting] = useState<{ user: engine.User; pin: string } | null>(null);
  const team = users.length > 0;

  const load = useCallback(() => {
    engine.listUsers().then(setUsers, (e) => setError(String(e?.message ?? e)));
    engine.teamSettings().then(setSettings, () => setSettings(null));
  }, []);
  useEffect(() => void load(), [load]);

  const fail = (e: unknown) => setError(e instanceof Error ? e.message : String(e));

  async function add() {
    setError(null);
    try {
      const u = await engine.addUser(name, team ? role : "dvm", pin, app.user?.id ?? null);
      setName("");
      setPin("");
      app.toast(t("{name} added to the team", { name: u.name }));
      load();
      app.refreshStatus();
    } catch (e) {
      fail(e);
    }
  }

  async function askRemove(u: engine.User) {
    setError(null);
    try {
      setRemoving({ user: u, summary: await engine.profileSummary(u.id) });
    } catch (e) {
      fail(e);
    }
  }

  async function remove(u: engine.User) {
    setError(null);
    try {
      await engine.removeUser(u.id, app.user?.id ?? null);
      setRemoving(null);
      app.toast(t("{name} removed from the team", { name: u.name }));
      load();
      app.refreshStatus();
    } catch (e) {
      fail(e);
    }
  }

  async function resetPin() {
    if (!resetting) return;
    setError(null);
    try {
      await engine.setUserPin(resetting.user.id, resetting.pin, app.user?.id ?? null);
      app.toast(t("{name}'s PIN changed", { name: resetting.user.name }));
      setResetting(null);
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

  const versions = (s: engine.ProfileSummary) => s.versions + s.own_templates;

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
          {t(error)}
        </p>
      )}
      {team && settings && (
        <div className="ob-field template-mode">
          <span className="label">{t("Templates")}</span>
          <div className="ob-segmented" role="radiogroup" aria-label={t("Templates")}>
            {(["personal", "hospital"] as const).map((m) => (
              <button
                key={m}
                type="button"
                role="radio"
                className="ob-segment"
                aria-checked={settings.templates === m}
                disabled={!app.can.admin}
                onClick={() => settings.templates !== m && setPending({ kind: "mode", templates: m })}
              >
                {m === "personal" ? t("Each person's own") : t("Shared by the hospital")}
              </button>
            ))}
          </div>
          <span className="ob-help">
            {settings.templates === "personal"
              ? t("Everyone can keep their own version of any template. The hospital's templates are everyone's defaults.")
              : t("Everyone drafts with the hospital's templates. Personal versions are kept and come back if this changes.")}{" "}
            {app.can.admin ? t("Changing this needs your PIN.") : t("An admin changes this, with their PIN.")}
          </span>
        </div>
      )}
      {team && (
        <ul className="team-list">
          {users.map((u) => (
            <li key={u.id}>
              <span>{u.name}</span>
              <span className="small muted">{t(ROLE_LABEL[u.role])}</span>
              {u.admin && <StatusDot>{t("Admin")}</StatusDot>}
              <span className="team-actions">
                {app.can.admin && (
                  <Button variant="ghost" size="compact" onClick={() => setPending({ kind: "admin", user: u, admin: !u.admin })}>
                    {u.admin ? t("Remove admin") : t("Make admin")}
                  </Button>
                )}
                {app.can.team && u.id !== app.user?.id && (
                  <Button variant="ghost" size="compact" onClick={() => setResetting({ user: u, pin: "" })}>
                    {t("Reset PIN")}
                  </Button>
                )}
                {app.can.team && u.id !== app.user?.id && (
                  <Button
                    variant="danger"
                    size="compact"
                    aria-label={t("Remove {name}", { name: u.name })}
                    onClick={() => void askRemove(u)}
                  >
                    {t("Remove")}
                  </Button>
                )}
              </span>
              {resetting?.user.id === u.id && (
                <form
                  className="team-inline"
                  onSubmit={(e) => {
                    e.preventDefault();
                    void resetPin();
                  }}
                >
                  <label className="ob-field narrow">
                    <span className="label">{t("New PIN for {name}", { name: u.name })}</span>
                    <input
                      className="ob-input mono"
                      type="password"
                      inputMode="numeric"
                      autoComplete="new-password"
                      maxLength={8}
                      autoFocus
                      value={resetting.pin}
                      onChange={(e) => setResetting({ user: u, pin: e.target.value.replace(/\D/g, "") })}
                    />
                  </label>
                  <Button type="submit" disabled={resetting.pin.length < 4}>
                    {t("Save PIN")}
                  </Button>
                  <Button variant="ghost" onClick={() => setResetting(null)}>
                    {t("Cancel")}
                  </Button>
                </form>
              )}
              {removing?.user.id === u.id && (
                <div className="team-inline" role="alert">
                  <p className="small">
                    {versions(removing.summary) > 0
                      ? t("{person} has their own version of {n} templates. They're removed with {person}. Notes keep the version that drafted them.", {
                          person: u.name,
                          n: versions(removing.summary),
                        })
                      : t("Remove {person} from the team? Notes keep who wrote them.", { person: u.name })}
                  </p>
                  <Button variant="danger" onClick={() => void remove(u)}>
                    {t("Remove {name}", { name: u.name })}
                  </Button>
                  <Button variant="ghost" onClick={() => setRemoving(null)}>
                    {t("Keep")}
                  </Button>
                </div>
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
      {!team && <p className="ob-help">{t("The first person added is the team's admin.")}</p>}
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
      {pending && (
        <ApproveDialog
          title={
            pending.kind === "mode"
              ? pending.templates === "hospital"
                ? t("Share templates by the hospital")
                : t("Let each person keep their own templates")
              : pending.admin
                ? t("Make {name} an admin", { name: pending.user.name })
                : t("Remove {name} as an admin", { name: pending.user.name })
          }
          body={
            pending.kind === "mode"
              ? pending.templates === "hospital"
                ? t("Everyone will draft with the hospital's templates. Personal versions are kept, not used, and come back if this is switched back.")
                : t("Everyone can keep their own version of any template again. Personal versions saved before come back.")
              : t("Admins change the hospital's templates and this setting, each time with their PIN. There is always at least one admin.")
          }
          action={pending.kind === "mode" ? t("Change templates setting") : pending.admin ? t("Make admin") : t("Remove admin")}
          onCancel={() => setPending(null)}
          onApprove={async (approver) => {
            if (pending.kind === "mode") {
              setSettings(await engine.setTeamSettings(pending.templates, approver));
              app.refreshStatus();
              app.toast(pending.templates === "hospital" ? t("Templates are now shared by the hospital") : t("Templates are now each person's own"));
            } else {
              const u = await engine.setUserAdmin(pending.user.id, pending.admin, approver);
              if (u.id === app.user?.id) app.setUser(u);
              app.toast(u.admin ? t("{name} is now an admin", { name: u.name }) : t("{name} is no longer an admin", { name: u.name }));
              load();
            }
            setPending(null);
          }}
        />
      )}
    </section>
  );
}
