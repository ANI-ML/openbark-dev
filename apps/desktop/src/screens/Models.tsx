import { Button, StatusDot } from "@openbark/ui";
import { DownloadSimple, MagnifyingGlass, X } from "@phosphor-icons/react";
import { useEffect, useState } from "react";
import { fmtBytes, useApp } from "../context";
import * as engine from "../engine";
import { useT } from "../i18n";

const SLOT_LABEL: Record<engine.Slot, string> = {
  llm: "Notes",
  stt: "Transcription",
  vad: "Speech detection",
  diarization: "Speakers",
  speaker_embedding: "Speakers",
};

const FIT: Record<engine.Fit, { label: string; tone: "ok" | "warn" | "neutral" }> = {
  fits: { label: "Fits", tone: "ok" },
  tight: { label: "Tight", tone: "warn" },
  wont_fit: { label: "Won't fit", tone: "neutral" },
};

/** DESIGN.md §5 Model manager: catalog rows, fit words, determinate progress. */
export function Models() {
  const app = useApp();
  const t = useT();
  const [rows, setRows] = useState<engine.ModelRow[]>([]);
  const [progress, setProgress] = useState<Record<string, { done: number; total: number }>>({});
  const [error, setError] = useState<string | null>(null);
  const [browsing, setBrowsing] = useState(false);

  const load = () => engine.models().then(setRows, (e) => setError(String(e?.message ?? e)));

  useEffect(() => {
    void load();
    const off = engine.on("download", (d) => setProgress((p) => ({ ...p, [d.model]: { done: d.done, total: d.total } })));
    return () => void off.then((f) => f());
  }, []);

  async function download(id: string) {
    setError(null);
    setProgress((p) => ({ ...p, [id]: { done: 0, total: rows.find((r) => r.id === id)?.size_bytes ?? 1 } }));
    try {
      await engine.downloadModel(id);
      app.toast(t("Model downloaded"));
      app.refreshStatus();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setProgress((p) => {
        const { [id]: _, ...rest } = p;
        return rest;
      });
      void load();
    }
  }

  const llms = rows.filter((r) => r.slot === "llm" && r.installed);

  return (
    <div className="screen">
      <header className="screen-head">
        <h1>{t("Models")}</h1>
        <Button variant="primary" onClick={() => setBrowsing(true)}>
          <MagnifyingGlass size={20} aria-hidden />
          {t("Browse Hugging Face")}
        </Button>
      </header>
      <p className="small muted">{app.status?.hardware.summary}</p>
      {error && (
        <p className="ob-notice" data-tone="danger" role="alert">
          {error}
        </p>
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
                  <option key={m.id} value={m.id}>
                    {m.name}
                  </option>
                ))}
                {!llms.some((m) => m.id === app.settings.llm) && <option value={app.settings.llm}>{app.settings.llm}</option>}
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

      <section className="group">
        <h2>{t("Catalog")}</h2>
        <div className="table-wrap">
          <table className="ob-table">
            <thead>
              <tr>
                <th>{t("Model")}</th>
                <th>{t("Use")}</th>
                <th className="num">{t("Size")}</th>
                <th>{t("Licence")}</th>
                <th>{t("On this computer")}</th>
                <th aria-label={t("Actions")} />
              </tr>
            </thead>
            <tbody>
              {rows.map((r) => {
                const p = progress[r.id];
                return (
                  <tr key={r.id}>
                    <td>
                      <div>{r.name}</div>
                      <div className="small faint">{r.publisher}</div>
                    </td>
                    <td>{t(SLOT_LABEL[r.slot])}</td>
                    <td className="num">{fmtBytes(r.size_bytes)}</td>
                    <td>
                      <span className="ob-chip">{r.license}</span>
                    </td>
                    <td>
                      {r.installed ? (
                        <StatusDot tone="ok">{t("Ready")}</StatusDot>
                      ) : r.slot === "llm" ? (
                        <StatusDot tone={FIT[r.fit].tone}>{t(FIT[r.fit].label)}</StatusDot>
                      ) : (
                        <StatusDot>{t("Not downloaded")}</StatusDot>
                      )}
                    </td>
                    <td className="cell-action">
                      {p ? (
                        <div className="dl">
                          <div className="ob-progress" role="progressbar" aria-valuenow={Math.round((p.done / p.total) * 100)}>
                            <span style={{ transform: `scaleX(${p.done / Math.max(1, p.total)})` }} />
                          </div>
                          <span className="mono faint">{t("{size} left", { size: fmtBytes(Math.max(0, p.total - p.done)) })}</span>
                        </div>
                      ) : !r.installed ? (
                        <Button onClick={() => download(r.id)} disabled={r.fit === "wont_fit"}>
                          <DownloadSimple size={20} aria-hidden />
                          {t("Download ({size})", { size: fmtBytes(r.size_bytes) })}
                        </Button>
                      ) : null}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      </section>

      {browsing && (
        <HubBrowser
          mac={app.status?.platform === "macos"}
          onClose={() => setBrowsing(false)}
          onAdded={(id) => {
            setBrowsing(false);
            void load().then(() => download(id));
          }}
        />
      )}
    </div>
  );
}

function HubBrowser({ mac, onClose, onAdded }: { mac: boolean; onClose: () => void; onAdded: (id: string) => void }) {
  const t = useT();
  const [onlyMlx, setOnlyMlx] = useState(false);
  const [q, setQ] = useState("gemma 4");
  const [results, setResults] = useState<engine.HubModel[] | null>(null);
  const [showAll, setShowAll] = useState(false);
  const [repo, setRepo] = useState<engine.HubRepoView | null>(null);
  const [accepted, setAccepted] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function search(e?: React.FormEvent) {
    e?.preventDefault();
    setError(null);
    setRepo(null);
    setBusy(true);
    try {
      setResults(await engine.hubSearch(q));
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }

  useEffect(() => {
    void search();
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function open(id: string) {
    setError(null);
    setAccepted(false);
    setBusy(true);
    try {
      setRepo(await engine.hubRepo(id));
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }

  async function add(file: string) {
    if (!repo) return;
    setError(null);
    try {
      const row = await engine.hubAdd(repo.repo.model.id, file, accepted);
      onAdded(row.id);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }

  const visible = (results ?? []).filter((m) => (showAll || m.license_class === "permissive") && (!onlyMlx || m.mlx));
  const mlxRepo = Boolean(repo?.repo.model.mlx);
  const restricted = repo && repo.repo.model.license_class !== "permissive";
  const [acceptBefore, acceptAfter] = t("I've read the {link} and accept its terms.").split("{link}");

  return (
    <>
      <div className="ob-backdrop" onClick={onClose} />
      <aside className="ob-sheet" role="dialog" aria-modal="true" aria-label={t("Browse Hugging Face")}>
        <header className="sheet-head">
          <h2>{repo ? repo.repo.model.id : t("Browse Hugging Face")}</h2>
          <Button variant="ghost" onClick={repo ? () => setRepo(null) : onClose} aria-label={repo ? t("Back to results") : t("Close")}>
            {repo ? t("Back") : <X size={20} aria-hidden />}
          </Button>
        </header>
        <div className="sheet-body">
          <p className="small muted">
            {mac
              ? t("Note-writing models in GGUF format, and builds optimized for Apple silicon with MLX. Searching contacts huggingface.co; nothing else leaves this computer.")
              : t("Note-writing models in GGUF format. Searching contacts huggingface.co; nothing else leaves this computer.")}
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
                  <input className="ob-input" value={q} onChange={(e) => setQ(e.target.value)} placeholder="gemma, qwen, llama" />
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
              {results === null ? (
                <div className="ob-skeleton" style={{ height: 120 }} />
              ) : visible.length === 0 ? (
                <p className="small muted">{t("No permissively licensed GGUF models match. Try another search, or show all licences.")}</p>
              ) : (
                <ul className="hub-list">
                  {visible.map((m) => (
                    <li key={m.id}>
                      <button type="button" className="hub-row" onClick={() => open(m.id)}>
                        <span className="hub-name">{m.id}</span>
                        <span className="hub-meta">
                          {mac && m.mlx && <span className="ob-chip mlx-chip">{t("MLX optimized")}</span>}
                          <StatusDot tone={m.license_class === "permissive" ? "ok" : "warn"}>{m.license ?? t("No licence stated")}</StatusDot>
                          <span className="mono faint">{t("{n} downloads", { n: m.downloads.toLocaleString() })}</span>
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
              <p>
                <StatusDot tone={restricted ? "warn" : "ok"}>{t("Licence: {licence}", { licence: repo.repo.model.license ?? t("not stated") })}</StatusDot>
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
              {mlxRepo && (
                <p className="ob-notice">
                  <span className="ob-chip mlx-chip">{t("MLX optimized")}</span>{" "}
                  {t(
                    "Converted for Apple silicon with Apple's MLX framework. OpenBark's built-in engine runs GGUF models on the Mac's GPU (Metal); MLX models need the MLX engine, which is planned. Search for the GGUF build of this model to use it now.",
                  )}
                </p>
              )}
              {repo.files.length === 0 ? (
                mlxRepo ? null : <p className="small muted">{t("This repository has no single-file GGUF weights OpenBark can run.")}</p>
              ) : (
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
                        <td className="mono">{f.path}</td>
                        <td className="num">{fmtBytes(f.size_bytes)}</td>
                        <td>
                          <StatusDot tone={FIT[f.fit].tone}>{t(FIT[f.fit].label)}</StatusDot>
                        </td>
                        <td className="cell-action">
                          <Button onClick={() => add(f.path)} disabled={(restricted && !accepted) || f.fit === "wont_fit"}>
                            {t("Download")}
                          </Button>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
            </>
          )}
        </div>
      </aside>
    </>
  );
}
