import { Button, StatusDot } from "@openbark/ui";
import { DownloadSimple, MagnifyingGlass, X } from "@phosphor-icons/react";
import { useEffect, useState } from "react";
import { fmtBytes, useApp } from "../context";
import * as engine from "../engine";

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
      app.toast("Model downloaded");
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
        <h1>Models</h1>
        <Button variant="primary" onClick={() => setBrowsing(true)}>
          <MagnifyingGlass size={20} aria-hidden />
          Browse Hugging Face
        </Button>
      </header>
      <p className="small muted">{app.status?.hardware.summary}</p>
      {error && (
        <p className="ob-notice" data-tone="danger" role="alert">
          {error}
        </p>
      )}

      <section className="group">
        <h2>In use</h2>
        <dl className="slots">
          <div>
            <dt className="label muted">Notes</dt>
            <dd>
              <select
                className="ob-select"
                value={app.settings.llm}
                onChange={(e) => app.setSettings({ llm: e.target.value })}
                aria-label="Model that writes notes"
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
            <dt className="label muted">Transcription</dt>
            <dd>Parakeet TDT 0.6B v3</dd>
          </div>
          <div>
            <dt className="label muted">Speakers</dt>
            <dd>WeSpeaker ResNet34</dd>
          </div>
        </dl>
      </section>

      <section className="group">
        <h2>Catalog</h2>
        <div className="table-wrap">
          <table className="ob-table">
            <thead>
              <tr>
                <th>Model</th>
                <th>Use</th>
                <th className="num">Size</th>
                <th>Licence</th>
                <th>On this computer</th>
                <th aria-label="Actions" />
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
                    <td>{SLOT_LABEL[r.slot]}</td>
                    <td className="num">{fmtBytes(r.size_bytes)}</td>
                    <td>
                      <span className="ob-chip">{r.license}</span>
                    </td>
                    <td>
                      {r.installed ? (
                        <StatusDot tone="ok">Ready</StatusDot>
                      ) : r.slot === "llm" ? (
                        <StatusDot tone={FIT[r.fit].tone}>{FIT[r.fit].label}</StatusDot>
                      ) : (
                        <StatusDot>Not downloaded</StatusDot>
                      )}
                    </td>
                    <td className="cell-action">
                      {p ? (
                        <div className="dl">
                          <div className="ob-progress" role="progressbar" aria-valuenow={Math.round((p.done / p.total) * 100)}>
                            <span style={{ transform: `scaleX(${p.done / Math.max(1, p.total)})` }} />
                          </div>
                          <span className="mono faint">{fmtBytes(Math.max(0, p.total - p.done))} left</span>
                        </div>
                      ) : !r.installed ? (
                        <Button onClick={() => download(r.id)} disabled={r.fit === "wont_fit"}>
                          <DownloadSimple size={20} aria-hidden />
                          Download ({fmtBytes(r.size_bytes)})
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

function HubBrowser({ onClose, onAdded }: { onClose: () => void; onAdded: (id: string) => void }) {
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

  const visible = (results ?? []).filter((m) => showAll || m.license_class === "permissive");
  const restricted = repo && repo.repo.model.license_class !== "permissive";

  return (
    <>
      <div className="ob-backdrop" onClick={onClose} />
      <aside className="ob-sheet" role="dialog" aria-modal="true" aria-label="Browse Hugging Face">
        <header className="sheet-head">
          <h2>{repo ? repo.repo.model.id : "Browse Hugging Face"}</h2>
          <Button variant="ghost" onClick={repo ? () => setRepo(null) : onClose} aria-label={repo ? "Back to results" : "Close"}>
            {repo ? "Back" : <X size={20} aria-hidden />}
          </Button>
        </header>
        <div className="sheet-body">
          <p className="small muted">
            Note-writing models in GGUF format. Searching contacts huggingface.co; nothing else leaves this computer.
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
                  <span className="label">Search models</span>
                  <input className="ob-input" value={q} onChange={(e) => setQ(e.target.value)} placeholder="gemma, qwen, llama" />
                </label>
                <Button type="submit" disabled={busy}>
                  Search
                </Button>
              </form>
              <label className="check small">
                <input type="checkbox" checked={showAll} onChange={(e) => setShowAll(e.target.checked)} />
                <span>Also show models with restrictive or custom licences</span>
              </label>
              {results === null ? (
                <div className="ob-skeleton" style={{ height: 120 }} />
              ) : visible.length === 0 ? (
                <p className="small muted">No permissively licensed GGUF models match. Try another search, or show all licences.</p>
              ) : (
                <ul className="hub-list">
                  {visible.map((m) => (
                    <li key={m.id}>
                      <button type="button" className="hub-row" onClick={() => open(m.id)}>
                        <span className="hub-name">{m.id}</span>
                        <span className="hub-meta">
                          <StatusDot tone={m.license_class === "permissive" ? "ok" : "warn"}>{m.license ?? "No licence stated"}</StatusDot>
                          <span className="mono faint">{m.downloads.toLocaleString()} downloads</span>
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
                <StatusDot tone={restricted ? "warn" : "ok"}>Licence: {repo.repo.model.license ?? "not stated"}</StatusDot>
              </p>
              {restricted && (
                <label className="check small accept">
                  <input type="checkbox" checked={accepted} onChange={(e) => setAccepted(e.target.checked)} />
                  <span>
                    I've read the{" "}
                    <a href={`https://huggingface.co/${repo.repo.model.id}`} target="_blank" rel="noreferrer">
                      licence on Hugging Face
                    </a>{" "}
                    and accept its terms.
                  </span>
                </label>
              )}
              {repo.files.length === 0 ? (
                <p className="small muted">This repository has no single-file GGUF weights OpenBark can run.</p>
              ) : (
                <table className="ob-table">
                  <thead>
                    <tr>
                      <th>File</th>
                      <th className="num">Size</th>
                      <th>Fit</th>
                      <th aria-label="Actions" />
                    </tr>
                  </thead>
                  <tbody>
                    {repo.files.map((f) => (
                      <tr key={f.path}>
                        <td className="mono">{f.path}</td>
                        <td className="num">{fmtBytes(f.size_bytes)}</td>
                        <td>
                          <StatusDot tone={FIT[f.fit].tone}>{FIT[f.fit].label}</StatusDot>
                        </td>
                        <td className="cell-action">
                          <Button onClick={() => add(f.path)} disabled={(restricted && !accepted) || f.fit === "wont_fit"}>
                            Download
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
