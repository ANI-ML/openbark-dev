import { Button, Dialog } from "@openbark/ui";
import { Trash2 } from "lucide-react";
import { useState } from "react";
import { fmtBytes } from "../context";
import * as engine from "../engine";
import { useT } from "../i18n";

/** What is being removed: a catalog row, the MLX engine pack or the GPU speech pack. */
export type RemoveTarget =
  { kind: "model"; row: engine.ModelRow } | { kind: "mlx"; bytes: number } | { kind: "gpu"; bytes: number };

/** What the dialog did, for the caller's toast and reload. */
export interface Removed {
  /** The engine's account; null for the GPU pack (it answers with its status). */
  removal: engine.Removal | null;
  gpu: engine.GpuStatus | null;
  /** The toast already shown. */
  message: string;
}

/**
 * Bytes removing a row frees: its own files, or what a stopped download left.
 * A copy that is a hard link to the original frees nothing (its bytes are
 * shared), so it counts 0, not its size.
 */
export const rowBytes = (r: engine.ModelRow) =>
  (r.disk_bytes ?? 0) > 0 || (r.shared_bytes ?? 0) > 0
    ? (r.disk_bytes ?? 0)
    : r.installed
      ? r.size_bytes
      : r.partial_bytes;

/** Everything the row has on disk is shared with another file: removing it frees nothing. */
const onlyShared = (r: engine.ModelRow) => (r.shared_bytes ?? 0) > 0 && (r.disk_bytes ?? 0) === 0;

/**
 * Remove a model or an engine pack (ADR-0025 §10), with what it frees and
 * whether it can come back. The engine applies the guards (the note model, a
 * note being written, a download running); its refusal is shown here and the
 * dialog stays open.
 */
export function RemoveModelDialog({
  target,
  noteModel,
  onClose,
  onRemoved,
  toast,
}: {
  target: RemoveTarget;
  /** This device's note model (app.settings.llm): the engine refuses to remove it. */
  noteModel: string | null;
  onClose: () => void;
  onRemoved: (r: Removed) => void;
  toast: (message: string) => void;
}) {
  const t = useT();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [forget, setForget] = useState(false);

  const words = copy(t, target);

  async function confirm(close: () => void) {
    setBusy(true);
    setError(null);
    try {
      let removal: engine.Removal | null = null;
      let gpu: engine.GpuStatus | null = null;
      if (target.kind === "model") removal = await engine.removeModel(target.row.id, noteModel, forget);
      else if (target.kind === "mlx") removal = await engine.removeMlx(noteModel);
      else gpu = await engine.removeGpu();
      const freed = removal ? removal.freed_bytes : target.kind === "gpu" ? target.bytes : 0;
      const message = removal?.kept_in_place
        ? t("Forgotten. Nothing was deleted.")
        : freed === 0 && (removal?.shared_bytes ?? 0) > 0
          ? t("Removed. It was linked to another file, so no space was freed.")
          : removal?.forgotten && freed === 0
            ? t("Removed from the list.")
            : t("Removed. {size} freed.", { size: fmtBytes(freed) });
      toast(message);
      onRemoved({ removal, gpu, message });
      close();
    } catch (e) {
      setError(engine.modelMessage(t, e));
      setBusy(false);
    }
  }

  return (
    <Dialog labelledBy="remove-model-title" onClose={onClose} dismissible={!busy} className="delete-dialog">
      {(close) => (
        <>
          <h2 id="remove-model-title">{words.title}</h2>
          <p>{words.body}</p>
          {words.hub && (
            <label className="check small">
              <input type="checkbox" checked={forget} disabled={busy} onChange={(e) => setForget(e.target.checked)} />
              <span>{t("Also remove it from the list")}</span>
            </label>
          )}
          {error && (
            <p className="ob-notice" data-tone="danger" role="alert">
              {error}
            </p>
          )}
          <div className="ask-actions">
            <Button variant="danger" disabled={busy} onClick={() => void confirm(close)}>
              <Trash2 size={18} aria-hidden />
              {words.confirm}
            </Button>
            <Button variant="ghost" disabled={busy} onClick={close} autoFocus>
              {t("Cancel")}
            </Button>
          </div>
        </>
      )}
    </Dialog>
  );
}

type T = ReturnType<typeof useT>;

/** The plan's copy for each case (docs/plans/mlx-models.md, Remove dialog). */
function copy(t: T, target: RemoveTarget): { title: string; body: string; confirm: string; hub: boolean } {
  if (target.kind === "mlx") {
    return {
      title: t("Remove the MLX engine?"),
      body: t(
        "This frees {size}. MLX models stay on this computer, but can't write notes until you download the engine again.",
        { size: fmtBytes(target.bytes) },
      ),
      confirm: t("Remove"),
      hub: false,
    };
  }
  if (target.kind === "gpu") {
    return {
      title: t("Remove the GPU speech pack?"),
      body: `${t("Speech recognition goes back to the processor.")} ${t("This frees {size}.", { size: fmtBytes(target.bytes) })}`,
      confirm: t("Remove"),
      hub: false,
    };
  }
  const r = target.row;
  const name = r.name;
  const size = fmtBytes(rowBytes(r));
  if (r.remove === "forget_only" || r.origin === "local_in_place") {
    return {
      title: t("Forget {name}?", { name }),
      body: t("OpenBark will stop using this model. The file stays where it is, and nothing is deleted."),
      confirm: t("Forget"),
      hub: false,
    };
  }
  if (r.remove === "delete_and_forget" || r.origin === "local_copy") {
    return {
      title: t("Remove {name}?", { name }),
      body: onlyShared(r)
        ? t(
            "OpenBark's copy is linked to the original file, so removing it frees no space. The original file stays where it was.",
          )
        : t("This deletes OpenBark's copy and frees {size}. The original file stays where it was.", { size }),
      confirm: t("Remove"),
      hub: false,
    };
  }
  if (onlyShared(r)) {
    return {
      title: t("Remove {name}?", { name }),
      body: t(
        "This model's file is linked to another file on this computer, so removing it frees no space. You can download it again from the catalog.",
      ),
      confirm: t("Remove"),
      hub: r.origin === "hub",
    };
  }
  if (!r.installed && r.partial_bytes > 0) {
    return {
      title: t("Remove {name}?", { name }),
      body: t("This deletes the {size} downloaded so far.", { size }),
      confirm: t("Remove"),
      hub: r.origin === "hub",
    };
  }
  return {
    title: t("Remove {name}?", { name }),
    body: t("This frees {size} on this computer. You can download it again from the catalog.", { size }),
    confirm: t("Remove"),
    hub: r.origin === "hub",
  };
}
