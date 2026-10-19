// Phone numbers and call lengths as the screens show them (ADR-0023).

/** "+15555550142" → "+1 555 555 0142". Other countries' numbers are left as stored. */
export function formatPhone(e164: string | null | undefined): string {
  if (!e164) return "";
  const m = /^\+1(\d{3})(\d{3})(\d{4})$/.exec(e164);
  return m ? `+1 ${m[1]} ${m[2]} ${m[3]}` : e164;
}

/**
 * What was typed on the dial pad, as E.164: "+1 (555) 555-0142",
 * "555-555-0142" and "15555550142" all become "+15555550142". Ten digits
 * without a country code are taken as North American. `null` when it can't
 * be a full number. The engine checks again; this is only for the screen.
 */
export function toE164(typed: string): string | null {
  const t = typed.trim();
  if (!t || /[^\d\s+\-().]/.test(t) || t.slice(1).includes("+")) return null;
  const digits = t.replace(/\D/g, "");
  let e164: string;
  if (t.startsWith("+")) e164 = `+${digits}`;
  else if (digits.length === 10) e164 = `+1${digits}`;
  else if (digits.length === 11 && digits.startsWith("1")) e164 = `+${digits}`;
  else return null;
  const rest = e164.slice(1);
  return rest.length >= 8 && rest.length <= 15 && !rest.startsWith("0") ? e164 : null;
}

/** "02:14" for a call timer; hours when it runs that long. */
export function callTimer(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  const pad = (n: number) => String(n).padStart(2, "0");
  const h = Math.floor(s / 3600);
  const mm = pad(Math.floor((s % 3600) / 60));
  return h > 0 ? `${h}:${mm}:${pad(s % 60)}` : `${mm}:${pad(s % 60)}`;
}

/** "4:12" for a finished call's length. */
export function callLength(seconds: number): string {
  const s = Math.max(0, Math.round(seconds));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}
