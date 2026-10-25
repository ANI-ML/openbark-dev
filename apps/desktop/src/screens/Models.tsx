import { Button, Sheet, Skeleton, StatusDot } from "@openbark/ui";
import { ArrowLeft, Download, FilePlus, Search, Trash2, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { fmtBytes, useApp } from "../context";
import * as engine from "../engine";
import { hardwareLine, useT } from "../i18n";
import { AddLocalModelSheet } from "../components/AddLocalModelSheet";
import { GpuCard } from "../components/GpuCard";
import { HfTokenField } from "../components/HfTokenField";
import { MLX_CARD_ID, MlxCard } from "../components/MlxCard";
import { ModelGuide, ModelMeaning, ModelTags } from "../components/ModelGuide";
import { RemoveModelDialog, rowBytes } from "../components/RemoveModelDialog";
import { PHONE_STARTERS, STARTERS } from "../modelGuide";
import { PHONE, useMediaQuery } from "../useMediaQuery";
import { noPhoneDrafting, phoneCanDraft } from "../visits";
import "./Models.css";

const SLOT_LABEL: Record<engine.Slot, string> = {
  llm: "Notes",
  stt: "Transcription",
  vad: "Speech detection",
  diarization: "Speakers",
  speaker_embedding: "Speakers",
};

const FIT: Record<engine.Fit, { label: string; tone: "ok" | "warn" | "neutral" }> = {
  // Not downloaded yet, so no success dot: green means Ready only (§3.3, §7.6).
  fits: { label: "Fits", tone: "neutral" },
  tight: { label: "Tight", tone: "warn" },
  wont_fit: { label: "Won't fit", tone: "neutral" },
};

/** "1.2 / 3.4 GB": progress in one unit, as the brand's downloading chip shows it (§7.6). */
function fmtPair(done: number, total: number) {
  const [div, unit] = total >= 1e9 ? [1e9, "GB"] : [1e6, "MB"];
  const n = (b: number) => (b / div).toFixed(div === 1e9 ? 1 : 0);
  return `${n(Math.min(done, total))} / ${n(total)} ${unit}`;
}

/** The row has something on this computer to remove (files, a stopped download, or a model used where it is). */
const hasFiles = (r: engine.ModelRow) =>
  r.installed || (r.disk_bytes ?? 0) > 0 || (r.partial_bytes ?? 0) > 0 || r.origin === "local_in_place";

const runsMlx = (s: engine.MlxSupport | undefined) => s === "yes" || s === "test_mode";

/**
 * MLX models can write notes on this computer: MLX runs here and its engine
 * is installed (a check that is out of date after an update runs again by
 * itself on the first draft; a failed one needs Try again on the card).
 */
const mlxUsable = (s: engine.MlxStatus | null) =>
  s !== null && runsMlx(s.support) && s.pack_installed && s.check !== "failed";

/** DESIGN.md §5 Model manager: catalog rows, fit words, determinate progress. */
export function Models() {
  const app = useApp();
  const t = useT();
  const [rows, setRows] = useState<engine.ModelRow[]>([]);
  const [progress, setProgress] = useState<Record<string, { done: number; total: number }>>({});
  const [error, setError] = useState<string | null>(null);
  const [browsing, setBrowsing] = useState(false);
  const [adding, setAdding] = useState(false);
  const [removing, setRemoving] = useState<engine.ModelRow | null>(null);
  const [mlx, setMlx] = useState<engine.MlxStatus | null>(null);
  /** Open the browser straight on one repo (a starting point from the guide). */
  const [openRepo, setOpenRepo] = useState<string | null>(null);

  const load = () => engine.models().then(setRows, (e) => setError(engine.modelMessage(t, e)));

  useEffect(() => {
    void load();
    const off = engine.on("download", (d) => {
      // A copy from this computer shows its progress in the add sheet.
      if (d.model.startsWith("local/")) return;
      // The last event can arrive after the download call has returned:
      // a finished one clears the row's progress instead of setting it again.
      setProgress((p) => {
        if (d.total > 0 && d.done >= d.total) {
          const { [d.model]: _, ...rest } = p;
          return rest;
        }
        return { ...p, [d.model]: { done: d.done, total: d.total } };
      });
    });
    return () => void off.then((f) => f());
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function download(id: string) {
    setError(null);
    setProgress((p) => ({
      ...p,
      [id]: { done: 0, total: rows.find((r) => r.id === id)?.size_bytes ?? 1 },
    }));
    try {
      await engine.downloadModel(id);
      app.toast(t("Model downloaded"));
      app.refreshStatus();
    } catch (e) {
      setError(engine.modelMessage(t, e));
    } finally {
      setProgress((p) => {
        const { [id]: _, ...rest } = p;
        return rest;
      });
      void load();
    }
  }

  /** A Hugging Face model with nothing on this computer: drop it from the list (nothing to delete). */
  async function removeFromList(r: engine.ModelRow) {
    setError(null);
    try {
      await engine.removeModel(r.id, app.settings.llm, true);
      app.toast(t("Removed from the list."));
    } catch (e) {
      setError(engine.modelMessage(t, e));
    } finally {
      void load();
    }
  }

  function getMlxEngine() {
    setBrowsing(false);
    setOpenRepo(null);
    // After the sheet's exit motion, bring the card into view.
    setTimeout(() => {
      const card = document.getElementById(MLX_CARD_ID);
      card?.scrollIntoView?.({ behavior: "smooth", block: "start" });
      card?.focus({ preventScroll: true });
    }, 360);
  }

  const llms = rows.filter((r) => r.slot === "llm" && r.installed && !r.missing);
  // An MLX model can't write notes where the engine can't run or isn't installed.
  // On a phone, a model too large for it can't write notes there either.
  const canDraft = (r: engine.ModelRow) => (r.engine !== "mlx" || mlxUsable(mlx)) && !r.phone_note;
  const current = rows.find((r) => r.id === app.settings.llm);
  const currentProgress = progress[app.settings.llm];
  // The GPU recognisers come with the GPU speech pack, not one by one.
  const shown = rows.filter((r) => r.remove !== "with_gpu_pack");
  const mac = app.status?.platform === "macos";
  // A phone (ADR-0018 section 10): mobile models only, no MLX, no GPU pack,
  // and no files from the device itself.
  const phone = engine.isMobilePlatform(app.status?.platform) || app.status?.profile === "ios";
  const tier = app.status?.mobile_tier ?? null;
  /** Phone width (layout only): stacked catalog rows, plain names, a quieter Browse button. */
  const narrow = useMediaQuery(PHONE);
  const label = (name: string) => (narrow ? plainModelName(name).name : name);
  const rowProps = (r: engine.ModelRow) => ({
    row: r,
    progress: progress[r.id],
    noteModel: r.id === app.settings.llm,
    onDownload: () => void download(r.id),
    onRemove: () => setRemoving(r),
    onRemoveFromList: () => void removeFromList(r),
  });

  return (
    <div className="screen models">
      <header className="screen-head">
        <div className="head-title">
          {/* Phones reach Models from Settings: the way back (§9). */}
          <Button
            variant="ghost"
            size="compact"
            className="back-to-settings"
            onClick={() => app.navigate({ screen: "settings" })}
          >
            <ArrowLeft size={16} aria-hidden />
            {t("Settings")}
          </Button>
          <h1>{t("Models")}</h1>
        </div>
        <div className="head-actions">
          {!phone && (
            <Button onClick={() => setAdding(true)}>
              <FilePlus size={20} aria-hidden />
              {t("Add from this computer")}
            </Button>
          )}
          {/* On a phone the catalog's own Download buttons come first; browsing is secondary. */}
          <Button variant={narrow ? "default" : "primary"} onClick={() => setBrowsing(true)}>
            <Search size={20} aria-hidden />
            {t("Browse Hugging Face")}
          </Button>
        </div>
      </header>
      {rows.length > 0 && (
        <p>
          {currentProgress ? (
            <StatusDot tone="warn">
              {t("Downloading {progress}", { progress: fmtPair(currentProgress.done, currentProgress.total) })}
            </StatusDot>
          ) : current?.installed && !current.missing ? (
            // §7.6: always say where inference runs. Local-first is the promise.
            <StatusDot tone="ok">{`${label(current.name)} · ${t("local")}`}</StatusDot>
          ) : (
            <StatusDot tone="danger">{t("No model loaded")}</StatusDot>
          )}
        </p>
      )}
      <p className="small muted">
        {app.status?.profile === "ios" && app.status.hardware
          ? // The engine says the phone's own memory in its summary (ADR-0018 section 10), not a graphics card.
            t("This iPhone, {gb} GB memory", {
              gb:
                app.status.hardware.summary.match(/(\d+)\s*GB/)?.[1] ??
                String(Math.round(app.status.hardware.ram_bytes / 1e9)),
            })
          : hardwareLine(t, app.status?.hardware)}
      </p>
      {error && (
        <p className="ob-notice" data-tone="danger" role="alert">
          {error}
        </p>
      )}

      {phone ? (
        <p className="ob-notice" data-tone={phoneCanDraft(app.status) ? undefined : "danger"} data-testid="phone-tier">
          {phoneCanDraft(app.status)
            ? t(
                "Writing notes on this iPhone is optional. It uses a small model that fits this iPhone's memory ({ram} GB). Recording and transcribing need only the speech models.",
                { ram: tier ?? "" },
              )
            : noPhoneDrafting(app.status, t)}
        </p>
      ) : (
        <ModelGuide
          onOpenRepo={(repo) => {
            setOpenRepo(repo);
            setBrowsing(true);
          }}
        />
      )}

      <section className="group">
        <h2>{t("In use")}</h2>
        <dl className="slots">
          <div>
            <dt className="label muted">{t("Notes")}</dt>
            <dd>
              <select
                className="ob-select"
                value={app.settings.llm}
                onChange={(e) => app.setSettings({ llm: e.target.value })}
                aria-label={t("Model that writes notes")}
              >
                {llms.map((m) => (
                  <option key={m.id} value={m.id} disabled={!canDraft(m) && m.id !== app.settings.llm}>
                    {canDraft(m)
                      ? label(m.name)
                      : m.phone_note
                        ? t("{name} (too large for this iPhone)", { name: label(m.name) })
                        : t("{name} (needs the MLX engine)", { name: label(m.name) })}
                  </option>
                ))}
                {!llms.some((m) => m.id === app.settings.llm) && (
                  <option value={app.settings.llm}>{app.settings.llm}</option>
                )}
              </select>
            </dd>
          </div>
          <div>
            <dt className="label muted">{t("Transcription")}</dt>
            <dd>Parakeet TDT 0.6B v3</dd>
          </div>
          <div>
            <dt className="label muted">{t("Speakers")}</dt>
            <dd>WeSpeaker ResNet34</dd>
          </div>
        </dl>
      </section>

      {!phone && <GpuCard />}
      {!phone && <MlxCard onChange={setMlx} />}

      <section className="group">
        <h2>{t("Catalog")}</h2>
        {narrow ? (
          <ul className="model-items">
            {shown.map((r) => (
              <CatalogItem key={r.id} {...rowProps(r)} />
            ))}
          </ul>
        ) : (
          <div className="table-wrap">
            <table className="ob-table">
              <thead>
                <tr>
                  <th>{t("Model")}</th>
                  <th>{t("Use")}</th>
                  <th className="num">{t("Size")}</th>
                  <th>{t("Licence")}</th>
                  <th>{phone ? t("On this phone") : t("On this computer")}</th>
                  <th aria-label={t("Actions")} />
                </tr>
              </thead>
              <tbody>
                {shown.map((r) => (
                  <CatalogRow key={r.id} {...rowProps(r)} />
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>

      {browsing && (
        <HubBrowser
          mac={mac && !phone}
          phone={phone}
          initialRepo={openRepo}
          onClose={() => {
            setBrowsing(false);
            setOpenRepo(null);
          }}
          onAdded={(id) => {
            setBrowsing(false);
            void load().then(() => download(id));
          }}
          onGetMlxEngine={getMlxEngine}
        />
      )}
      {adding && (
        <AddLocalModelSheet
          mlx={runsMlx(mlx?.support)}
          onClose={() => setAdding(false)}
          onAdded={(row) => {
            setAdding(false);
            // The copy's progress came as download events for the new id.
            setProgress((p) => {
              const { [row.id]: _, ...rest } = p;
              return rest;
            });
            void load();
          }}
        />
      )}
      {removing && (
        <RemoveModelDialog
          target={{ kind: "model", row: removing }}
          noteModel={app.settings.llm}
          toast={app.toast}
          onClose={() => setRemoving(null)}
          onRemoved={() => {
            app.refreshStatus();
            void load();
          }}
        />
      )}
    </div>
  );
}

/** What a catalog row's state, chips and actions need. */
interface RowProps {
  row: engine.ModelRow;
  progress?: { done: number; total: number };
  /** This device's note model: it can't be removed. */
  noteModel: boolean;
  onDownload: () => void;
  onRemove: () => void;
  onRemoveFromList: () => void;
}

function RowChips({ row: r }: { row: engine.ModelRow }) {
  const t = useT();
  const inPlace = r.origin === "local_in_place";
  const parts = r.parts ?? 1;
  // ModelTags already labels MLX when the id says so (Hub ids end in "@mlx").
  const mlxChip = r.engine === "mlx" && !/mlx/i.test(r.id);
  if (!inPlace && !mlxChip && parts <= 1) return null;
  return (
    <div className="row-chips">
      {mlxChip && <span className="ob-chip mlx-chip">{t("MLX")}</span>}
      {parts > 1 &&
        (r.engine === "mlx" ? (
          <span className="ob-chip">{t("{n} files", { n: parts })}</span>
        ) : (
          <span className="ob-chip">{t("{n} parts", { n: parts })}</span>
        ))}
      {inPlace && (
        <span className="ob-chip" title={r.path ?? undefined}>
          {t("Used where it is")}
        </span>
      )}
    </div>
  );
}

function RowState({ row: r, progress: p }: { row: engine.ModelRow; progress?: { done: number; total: number } }) {
  const t = useT();
  const inPlace = r.origin === "local_in_place";
  return (
    <div className="row-state">
      {p ? (
        <StatusDot tone="warn">{t("Downloading")}</StatusDot>
      ) : r.bundled ? (
        // The iPhone app's speech models come inside the app (ADR-0018 section 10).
        <StatusDot tone="ok">{t("Built in")}</StatusDot>
      ) : r.missing ? (
        <StatusDot tone="danger">{t("Not found")}</StatusDot>
      ) : r.installed ? (
        <StatusDot tone="ok">{t("Ready")}</StatusDot>
      ) : r.slot === "llm" ? (
        <StatusDot tone={FIT[r.fit].tone}>{t(FIT[r.fit].label)}</StatusDot>
      ) : (
        <StatusDot>{t("Not downloaded")}</StatusDot>
      )}
      {!p && r.installed && !inPlace && (r.disk_bytes ?? 0) > 0 && (
        <span className="small faint">{t("{size} on disk", { size: fmtBytes(r.disk_bytes) })}</span>
      )}
      {!p && r.installed && !inPlace && (r.shared_bytes ?? 0) > 0 && (r.disk_bytes ?? 0) === 0 && (
        <span className="small faint">{t("Linked to another file, so it takes no extra space")}</span>
      )}
      {!p && !r.installed && (r.partial_bytes ?? 0) > 0 && (
        <span className="small faint">{t("{size} partly downloaded", { size: fmtBytes(r.partial_bytes) })}</span>
      )}
      {inPlace && r.path && <span className="small faint mono row-path">{r.path}</span>}
    </div>
  );
}

function RowActions({ row: r, progress: p, noteModel, onDownload, onRemove, onRemoveFromList }: RowProps) {
  const t = useT();
  const local = r.origin === "local_copy" || r.origin === "local_in_place";
  const inPlace = r.origin === "local_in_place";
  const files = hasFiles(r);
  const removable = files && r.remove !== "needed" && r.remove !== "with_gpu_pack";
  const noteHelpId = `note-model-${r.id}`;
  if (r.bundled) {
    return (
      <div className="row-actions">
        <span className="small faint row-help">{t("Comes with the app")}</span>
      </div>
    );
  }
  if (p) {
    return (
      <div className="dl">
        <div className="ob-progress" role="progressbar" aria-valuenow={Math.round((p.done / Math.max(1, p.total)) * 100)}>
          <span style={{ transform: `scaleX(${p.done / Math.max(1, p.total)})` }} />
        </div>
        <span className="mono faint">{t("{size} left", { size: fmtBytes(Math.max(0, p.total - p.done)) })}</span>
      </div>
    );
  }
  return (
    <div className="row-actions">
      {!r.installed && !local && (
        <Button onClick={onDownload} disabled={r.fit === "wont_fit" || Boolean(r.phone_note)}>
          <Download size={20} aria-hidden />
          {t("Download ({size})", { size: fmtBytes(r.size_bytes) })}
        </Button>
      )}
      {removable ? (
        <>
          <Button
            variant="ghost"
            onClick={onRemove}
            disabled={noteModel}
            aria-describedby={noteModel ? noteHelpId : undefined}
          >
            <Trash2 size={18} aria-hidden />
            {inPlace ? t("Forget") : t("Remove")}
          </Button>
          {noteModel && (
            <span id={noteHelpId} className="small faint row-help">
              {t("Writes your notes. Choose another model first.")}
            </span>
          )}
        </>
      ) : files && r.remove === "needed" ? (
        <span className="small faint row-help">{t("Needed to transcribe visits")}</span>
      ) : !files && r.origin === "hub" ? (
        <Button variant="ghost" onClick={onRemoveFromList} disabled={noteModel}>
          {t("Remove from list")}
        </Button>
      ) : null}
    </div>
  );
}

function CatalogRow(props: RowProps) {
  const t = useT();
  const r = props.row;
  const local = r.origin === "local_copy" || r.origin === "local_in_place";
  return (
    <tr>
      <td>
        <div>{r.name}</div>
        <div className="small faint">{local ? t("This computer") : r.publisher}</div>
        <RowChips row={r} />
        {r.slot === "llm" && <ModelTags name={r.id} />}
        {r.phone_note && <div className="small muted row-help">{r.phone_note}</div>}
        {r.attribution && <div className="small faint row-credit">{r.attribution}</div>}
      </td>
      <td>{t(SLOT_LABEL[r.slot])}</td>
      <td className="num">{fmtBytes(r.size_bytes)}</td>
      <td>
        <span className="ob-chip">{r.license}</span>
      </td>
      <td>
        <RowState row={r} progress={props.progress} />
      </td>
      <td className="cell-action">
        <RowActions {...props} />
      </td>
    </tr>
  );
}

/**
 * The same row stacked for a phone (ADR-0027): the plain name first, the
 * build details (quantization, publisher, licence) on a quiet line beneath,
 * then the size and state, then the actions. No columns that wrap.
 */
function CatalogItem(props: RowProps) {
  const t = useT();
  const r = props.row;
  const local = r.origin === "local_copy" || r.origin === "local_in_place";
  const { name, detail } = plainModelName(r.name);
  const origin = local ? t("This computer") : r.publisher;
  return (
    <li className="model-item">
      <div className="model-item-text">
        <span className="model-item-name">{name}</span>
        <span className="small muted model-item-detail">
          {[t(SLOT_LABEL[r.slot]), origin, r.license].filter(Boolean).join(", ")}
          {detail && (
            <>
              {", "}
              <span className="mono">{detail}</span>
            </>
          )}
        </span>
        <span className="model-item-state">
          <span className="small mono">{fmtBytes(r.size_bytes)}</span>
          <RowState row={r} progress={props.progress} />
        </span>
        <RowChips row={r} />
        {r.phone_note && <span className="small muted">{r.phone_note}</span>}
        {r.attribution && <span className="small faint row-credit">{r.attribution}</span>}
      </div>
      <div className="model-item-actions">
        <RowActions {...props} />
      </div>
    </li>
  );
}

/** Build and file-format words that don't belong in a model's plain name. */
const BUILD_TOKEN = /^(?:i?q\d\w*|qat|gguf|onnx|mlx|int\d+|fp\d+|bf16|f16|f32|\d+-?bit|instruct|it)$/i;

/**
 * "Gemma 4 E4B instruct (QAT Q4_0)" reads "Gemma 4 E4B" with "QAT Q4_0" as its
 * detail; "Qwen3 235B Q4_K_M" reads "Qwen3 235B" with "Q4_K_M". The name a
 * person picks a model by, and the build details kept for a second line.
 */
export function plainModelName(full: string): { name: string; detail: string } {
  const details: string[] = [];
  // Parenthesised build details: "(QAT Q4_0)", "(int8)", "(full precision)".
  let name = full.replace(/\s*\(([^)]*)\)/g, (_, inner: string) => {
    if (inner.trim()) details.push(inner.trim());
    return "";
  });
  // Trailing build words, separated by spaces or hyphens ("Q4_K_M" is one word).
  for (;;) {
    const m = name.match(/^(.*\S)[\s-]+([A-Za-z0-9_]+)$/);
    if (!m || !BUILD_TOKEN.test(m[2] ?? "")) break;
    const token = m[2] ?? "";
    if (!/^(?:instruct|it)$/i.test(token)) details.unshift(token);
    name = m[1] ?? name;
  }
  name = name.trim();
  if (!name) return { name: full, detail: "" };
  return { name, detail: details.join(", ") };
}

function HubBrowser({
  mac,
  phone = false,
  initialRepo,
  onClose,
  onAdded,
  onGetMlxEngine,
}: {
  mac: boolean;
  /** A phone: small GGUFs that fit it, the rest held back with the reason. */
  phone?: boolean;
  initialRepo?: string | null;
  onClose: () => void;
  onAdded: (id: string) => void;
  /** Close the sheet and bring the MLX engine card into view. */
  onGetMlxEngine: () => void;
}) {
  const t = useT();
  const [onlyMlx, setOnlyMlx] = useState(false);
  const [q, setQ] = useState(phone ? "qwen3 1.7b" : "gemma 4");
  const [results, setResults] = useState<engine.HubModel[] | null>(null);
  const [showAll, setShowAll] = useState(false);
  const [repo, setRepo] = useState<engine.HubRepoView | null>(null);
  const [alternatives, setAlternatives] = useState<engine.HubModel[]>([]);
  const [accepted, setAccepted] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [tokenOpen, setTokenOpen] = useState(false);
  /** The repo now shown (or being opened): answers for any other one are dropped. */
  const shownRepo = useRef<string | null>(null);

  async function search(e?: React.FormEvent) {
    e?.preventDefault();
    setError(null);
    setRepo(null);
    setBusy(true);
    try {
      setResults(await engine.hubSearch(q));
    } catch (err) {
      setError(engine.modelMessage(t, err));
    } finally {
      setBusy(false);
    }
  }

  useEffect(() => {
    void search().then(() => {
      if (initialRepo) void open(initialRepo);
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function open(id: string) {
    shownRepo.current = id;
    setError(null);
    setAccepted(false);
    setAlternatives([]);
    setBusy(true);
    try {
      const view = await engine.hubRepo(id);
      // Back, or another repo, while this one was loading: drop the answer.
      if (shownRepo.current !== id) return;
      setRepo(view);
      // An MLX build: suggest GGUF builds of the same model, which run
      // anywhere. A repo with nothing OpenBark can run (safetensors only, as
      // many gated originals are): suggest GGUF builds of it.
      // On a phone, files held back for size aren't "nothing to run".
      const base = view.repo.model.mlx
        ? view.repo.base_model
        : view.files.length === 0 && !view.mlx && !(view.held?.length ?? 0)
          ? view.repo.model.id
          : null;
      if (base) {
        engine.hubAlternatives(base).then(
          (alts) => {
            if (shownRepo.current === id) setAlternatives(alts.filter((a) => a.id !== id));
          },
          () => {},
        );
      }
    } catch (err) {
      if (shownRepo.current === id) setError(engine.modelMessage(t, err));
    } finally {
      if (shownRepo.current === id) setBusy(false);
    }
  }

  function back() {
    shownRepo.current = null;
    setRepo(null);
    setAlternatives([]);
    setBusy(false);
  }

  async function add(file: string) {
    if (!repo) return;
    setError(null);
    try {
      const row = await engine.hubAdd(repo.repo.model.id, file, accepted);
      onAdded(row.id);
    } catch (err) {
      setError(engine.modelMessage(t, err));
    }
  }

  const visible = (results ?? []).filter((m) => (showAll || m.license_class === "permissive") && (!onlyMlx || m.mlx));
  const mlxRepo = Boolean(repo?.repo.model.mlx);
  const restricted = repo && repo.repo.model.license_class !== "permissive";
  const access: engine.HubAccess = repo?.repo.access ?? "open";
  /** Nothing OpenBark can run here, gated or not: no token would change that. */
  const nothingToRun = Boolean(repo && repo.files.length === 0 && !repo.mlx);
  const gated = access !== "open" && !nothingToRun;
  const [acceptBefore, acceptAfter] = t("I've read the {link} and accept its terms.").split("{link}");

  return (
    <Sheet label={t("Browse Hugging Face")} onClose={onClose}>
      {(close) => (
        <>
          <header className="sheet-head">
            <h2>{repo ? repo.repo.model.id : t("Browse Hugging Face")}</h2>
            <Button
              variant="ghost"
              onClick={repo ? back : close}
              aria-label={repo ? t("Back to results") : t("Close")}
            >
              {repo ? t("Back") : <X size={20} aria-hidden />}
            </Button>
          </header>
          <div className="sheet-body">
            <p className="small muted">
              {phone
                ? t(
                    "Small note-writing models in GGUF format that fit this iPhone. Searching contacts huggingface.co; nothing else leaves this iPhone.",
                  )
                : mac
                ? t(
                    "Note-writing models in GGUF format, and builds optimized for Apple silicon with MLX. Searching contacts huggingface.co; nothing else leaves this computer.",
                  )
                : t(
                    "Note-writing models in GGUF format. Searching contacts huggingface.co; nothing else leaves this computer.",
                  )}
            </p>
            {error && (
              <p className="ob-notice" data-tone="danger" role="alert">
                {error}
              </p>
            )}
            {!repo && (
              <>
                <form className="search" onSubmit={search}>
                  <label className="ob-field">
                    <span className="label">{t("Search models")}</span>
                    <input
                      className="ob-input"
                      value={q}
                      onChange={(e) => setQ(e.target.value)}
                      placeholder="gemma, qwen, llama"
                    />
                  </label>
                  <Button type="submit" disabled={busy}>
                    {t("Search")}
                  </Button>
                </form>
                <label className="check small">
                  <input type="checkbox" checked={showAll} onChange={(e) => setShowAll(e.target.checked)} />
                  <span>{t("Also show models with restrictive or custom licences")}</span>
                </label>
                {mac && (
                  <label className="check small">
                    <input type="checkbox" checked={onlyMlx} onChange={(e) => setOnlyMlx(e.target.checked)} />
                    <span>{t("Only show MLX-optimized models")}</span>
                  </label>
                )}
                <div className="hub-starters">
                  <span className="small muted">{t("Good starting points")}</span>
                  <div className="ask-chips">
                    {phone
                      ? PHONE_STARTERS.map((st) => (
                          <button
                            key={st.repo}
                            type="button"
                            className="ob-chip ask-chip"
                            title={t(st.why)}
                            onClick={() => open(st.repo)}
                          >
                            {st.label}
                          </button>
                        ))
                      : STARTERS.map((st) => (
                          <button
                            key={st.repo}
                            type="button"
                            className="ob-chip ask-chip"
                            title={t(st.why)}
                            onClick={() => open(st.repo)}
                          >
                            {`Gemma 4 ${st.size}`}
                          </button>
                        ))}
                  </div>
                </div>
                {results === null ? (
                  <Skeleton rows={3} label={t("Loading")} />
                ) : visible.length === 0 ? (
                  <p className="small muted">
                    {t("No permissively licensed GGUF models match. Try another search, or show all licences.")}
                  </p>
                ) : (
                  <ul className="hub-list">
                    {visible.map((m) => (
                      <li key={m.id}>
                        <button type="button" className="hub-row" onClick={() => open(m.id)}>
                          <span className="hub-name">{m.id}</span>
                          <ModelTags name={m.id} />
                          <span className="hub-meta">
                            {mac && m.mlx && <span className="ob-chip mlx-chip">{t("MLX optimized")}</span>}
                            <StatusDot tone={m.license_class === "permissive" ? "ok" : "warn"}>
                              {m.license ?? t("No licence stated")}
                            </StatusDot>
                            <span className="mono faint">
                              {t("{n} downloads", {
                                n: m.downloads.toLocaleString(),
                              })}
                            </span>
                          </span>
                        </button>
                      </li>
                    ))}
                  </ul>
                )}
              </>
            )}
            {repo && (
              <>
                <ModelMeaning name={repo.repo.model.id} />
                <p>
                  <StatusDot tone={restricted ? "warn" : "ok"}>
                    {t("Licence: {licence}", {
                      licence: repo.repo.model.license ?? t("not stated"),
                    })}
                  </StatusDot>
                </p>
                {restricted && (
                  <label className="check small accept">
                    <input type="checkbox" checked={accepted} onChange={(e) => setAccepted(e.target.checked)} />
                    <span>
                      {acceptBefore}
                      <a href={`https://huggingface.co/${repo.repo.model.id}`} target="_blank" rel="noreferrer">
                        {t("licence on Hugging Face")}
                      </a>
                      {acceptAfter}
                    </span>
                  </label>
                )}
                {gated && (
                  <GatedNotice
                    repo={repo.repo.model.id}
                    access={access}
                    manual={repo.repo.gated_kind === "manual"}
                    onToken={() => void open(repo.repo.model.id)}
                  />
                )}
                {mlxRepo && repo.mlx && (
                  <MlxChoice
                    mlx={repo.mlx}
                    gated={gated}
                    blocked={Boolean(restricted && !accepted)}
                    onDownload={() => void add(engine.MLX_CHOICE)}
                    onGetEngine={onGetMlxEngine}
                  />
                )}
                {repo.phone_note && (
                  <p className="ob-notice" data-tone="warn" role="status" data-testid="phone-note">
                    {repo.phone_note}
                  </p>
                )}
                {repo.files.length === 0 ? (
                  nothingToRun && !repo.phone_note ? (
                    <p className="small muted">
                      {phone
                        ? t("No GGUF files OpenBark can run in this repository.")
                        : t("No GGUF or MLX files OpenBark can run in this repository.")}
                    </p>
                  ) : null
                ) : (
                  <div className="table-wrap">
                    <table className="ob-table">
                      <thead>
                        <tr>
                          <th>{t("File")}</th>
                          <th className="num">{t("Size")}</th>
                          <th>{t("Fit")}</th>
                          <th aria-label={t("Actions")} />
                        </tr>
                      </thead>
                      <tbody>
                        {repo.files.map((f) => (
                          <tr key={f.path}>
                            <td>
                              <span className="mono">{f.path}</span>
                              {(f.parts ?? 1) > 1 && (
                                <span className="ob-chip hub-parts">{t("{n} parts", { n: f.parts })}</span>
                              )}
                              <ModelTags name={f.path} only="compression" />
                            </td>
                            <td className="num">{fmtBytes(f.size_bytes)}</td>
                            <td>
                              <StatusDot tone={FIT[f.fit].tone}>{t(FIT[f.fit].label)}</StatusDot>
                            </td>
                            <td className="cell-action">
                              <Button
                                onClick={() => add(f.path)}
                                disabled={(restricted && !accepted) || f.fit === "wont_fit" || gated}
                              >
                                {t("Download")}
                              </Button>
                            </td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                )}
                {phone && (repo.held?.length ?? 0) > 0 && (
                  <details className="hub-held">
                    <summary className="small muted">
                      {t("Not offered on this iPhone ({n})", { n: repo.held?.length ?? 0 })}
                    </summary>
                    <ul className="hub-held-list">
                      {repo.held?.map((h) => (
                        <li key={h.path} className="small">
                          <span className="mono">{h.path}</span> <span className="faint">{fmtBytes(h.size_bytes)}</span>
                          <div className="muted">{h.reason}</div>
                        </li>
                      ))}
                    </ul>
                  </details>
                )}
                {(mlxRepo || nothingToRun) && alternatives.length > 0 && (
                  <div className="hub-alternatives">
                    <h3>{t("GGUF builds of this model")}</h3>
                    <p className="small muted">{t("They run with the built-in engine on any computer.")}</p>
                    <ul className="hub-list">
                      {alternatives.map((m) => (
                        <li key={m.id}>
                          <button type="button" className="hub-row" onClick={() => void open(m.id)}>
                            <span className="hub-name">{m.id}</span>
                            <span className="hub-meta">
                              <StatusDot tone={m.license_class === "permissive" ? "ok" : "warn"}>
                                {m.license ?? t("No licence stated")}
                              </StatusDot>
                              <span className="mono faint">
                                {t("{n} downloads", { n: m.downloads.toLocaleString() })}
                              </span>
                            </span>
                          </button>
                        </li>
                      ))}
                    </ul>
                  </div>
                )}
              </>
            )}
          </div>
          {/* The token, for gated models; a gated repo shows it in its notice instead. */}
          {!gated && (
            <footer className="sheet-foot">
              {tokenOpen ? (
                <HfTokenField onChange={repo ? () => void open(repo.repo.model.id) : undefined} />
              ) : (
                <button type="button" className="link small" onClick={() => setTokenOpen(true)}>
                  {t("Hugging Face token")}
                </button>
              )}
            </footer>
          )}
        </>
      )}
    </Sheet>
  );
}

/** A gated repo: what to do, in order (ADR-0025 §8). */
function GatedNotice({
  repo,
  access,
  manual,
  onToken,
}: {
  repo: string;
  access: engine.HubAccess;
  manual: boolean;
  onToken: () => void;
}) {
  const t = useT();
  return (
    <div className="ob-notice" role="status">
      <div className="notice-body">
        <p>
          {access === "needs_token"
            ? t("This model is gated. Request access on Hugging Face, then add your token.")
            : t("Hugging Face hasn't given your token access yet. Check your request on the model's page.")}
          {manual && ` ${t("The publisher reviews requests by hand.")}`}
        </p>
        <div className="ask-actions gated-links">
          <a className="link" href={`https://huggingface.co/${repo}`} target="_blank" rel="noreferrer">
            {t("Request access")}
          </a>
          <a className="link" href="https://huggingface.co/settings/tokens" target="_blank" rel="noreferrer">
            {t("Get a token")}
          </a>
        </div>
        <HfTokenField onChange={onToken} />
      </div>
    </div>
  );
}

/** An MLX repo is one choice: its whole model folder, or the reason it can't run here. */
function MlxChoice({
  mlx,
  gated,
  blocked,
  onDownload,
  onGetEngine,
}: {
  mlx: engine.HubMlxView;
  gated: boolean;
  /** The licence isn't accepted yet. */
  blocked: boolean;
  onDownload: () => void;
  onGetEngine: () => void;
}) {
  const t = useT();
  const problem = mlx.problem;
  const reason = !runsMlx(mlx.support)
    ? t("MLX models run on a Mac with Apple silicon and macOS 14 or later.")
    : problem
      ? problemText(t, problem)
      : !mlx.arch_supported
        ? t("OpenBark's MLX engine can't run this kind of model yet ({type}).", { type: mlx.model_type ?? "?" })
        : !mlx.pack_installed
          ? t("Download the MLX engine first. It's on the Models screen.")
          : null;

  if (reason !== null) {
    // Hidden files are explained by the gated notice above.
    if (gated && problem?.kind === "hashes_hidden") return null;
    return (
      <div className="ob-notice">
        <div className="notice-body">
          <p>
            <span className="ob-chip mlx-chip">{t("MLX optimized")}</span> {reason}
          </p>
          {runsMlx(mlx.support) && !problem && mlx.arch_supported && !mlx.pack_installed && (
            <div className="ask-actions">
              <Button onClick={onGetEngine}>{t("Get the MLX engine")}</Button>
            </div>
          )}
        </div>
      </div>
    );
  }

  return (
    <div className="table-wrap">
      <table className="ob-table">
        <thead>
          <tr>
            <th>{t("File")}</th>
            <th className="num">{t("Size")}</th>
            <th>{t("Fit")}</th>
            <th aria-label={t("Actions")} />
          </tr>
        </thead>
        <tbody>
          <tr>
            <td>
              <div>{t("Whole model folder")}</div>
              <div className="row-chips">
                <span className="ob-chip mlx-chip">{t("MLX")}</span>
                <span className="ob-chip">{t("{n} files", { n: mlx.files })}</span>
                {mlx.quantization && <span className="ob-chip">{mlx.quantization}</span>}
              </div>
            </td>
            <td className="num">{fmtBytes(mlx.size_bytes)}</td>
            <td>
              <StatusDot tone={FIT[mlx.fit].tone}>{t(FIT[mlx.fit].label)}</StatusDot>
            </td>
            <td className="cell-action">
              <Button onClick={onDownload} disabled={blocked || mlx.fit === "wont_fit" || gated}>
                {t("Download")}
              </Button>
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  );
}

function problemText(t: ReturnType<typeof useT>, p: engine.MlxProblem): string {
  switch (p.kind) {
    case "no_config":
      return t("This repository has no config.json, so it can't run.");
    case "bad_config":
      return t("This repository's config.json can't be read.");
    case "no_weights":
      return t("This repository has no safetensors weights.");
    case "incomplete_shards":
      return t("Some weight files are missing: {files}.", { files: p.missing.join(", ") });
    case "no_tokenizer":
      return t("This repository has no tokenizer files.");
    case "hashes_hidden":
      return t("This model is gated. Request access on Hugging Face, then add your token.");
  }
}
