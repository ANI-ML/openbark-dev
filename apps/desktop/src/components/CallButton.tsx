import { Button } from "@openbark/ui";
import { Phone } from "lucide-react";
import { useEffect, useState } from "react";
import { useApp } from "../context";
import * as engine from "../engine";
import { useT } from "../i18n";

/** Whether calls can be placed here: Twilio connected, the phone on, and not the Linux desktop app. */
export function usePhoneReady(): boolean {
  const [ready, setReady] = useState(false);
  useEffect(() => {
    let live = true;
    engine.phone().then(
      (p) => live && setReady(p.ready && !(engine.isDesktop && p.linux)),
      () => live && setReady(false),
    );
    return () => {
      live = false;
    };
  }, []);
  return ready;
}

/**
 * **Call** (ADR-0023 section 5): opens the phone window for this patient's
 * owner, or with the dial pad. Shown only once a veterinarian has connected
 * the clinic's Twilio account and turned the phone on. Anyone can call.
 */
export function CallButton({
  patientId,
  label,
  variant = "default",
}: {
  patientId?: string | null;
  label?: string;
  variant?: "default" | "ghost" | "primary";
}) {
  const app = useApp();
  const t = useT();
  const ready = usePhoneReady();
  const [busy, setBusy] = useState(false);
  if (!ready) return null;
  async function call() {
    setBusy(true);
    try {
      const opened = await engine.openPhone({ patient_id: patientId ?? null });
      if (!opened) app.toast(t("Allow pop-ups for this page to make calls."));
    } catch (e) {
      app.toast(t(e instanceof Error ? e.message : String(e)));
    } finally {
      setBusy(false);
    }
  }
  return (
    <Button variant={variant} onClick={() => void call()} disabled={busy}>
      <Phone size={18} aria-hidden />
      {label ?? t("Call")}
    </Button>
  );
}
