import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { PanelA } from "./components/PanelA";
import { PanelC } from "./components/PanelC";
import { EpisodePanel } from "./components/EpisodePanel";
import { PrivacyDashboard } from "./components/PrivacyDashboard";
import { AnticipatedWorkspace } from "./components/AnticipatedWorkspace";
import { Cluster, Episode, ImportResult } from "./types";
import "./App.css";

type Phase = "loading" | "ready" | "empty" | "error";

function App() {
  const [phase, setPhase] = useState<Phase>("loading");
  const [clusters, setClusters] = useState<Cluster[]>([]);
  const [episodes, setEpisodes] = useState<Episode[]>([]);
  const [importSummary, setImportSummary] = useState<string>("");
  const [error, setError] = useState<string>("");
  const [captureUrl, setCaptureUrl] = useState("");
  const [captureTitle, setCaptureTitle] = useState("");
  const [capturing, setCapturing] = useState(false);
  const initialized = useRef(false);

  useEffect(() => {
    if (initialized.current) return;
    initialized.current = true;
    initWorkspace();
  }, []);

  async function initWorkspace(htmlContent?: string) {
    setPhase("loading");
    try {
      let ir: ImportResult;

      if (htmlContent !== undefined) {
        ir = await invoke<ImportResult>("import_bookmarks_html", { content: htmlContent });
      } else {
        ir = await invoke<ImportResult>("import_bookmarks", { path: null });
      }

      if (ir.imported > 0 || ir.skipped > 0) {
        setImportSummary(`${ir.imported} importados, ${ir.skipped} ya existentes`);
      }

      const [cls, eps] = await Promise.all([
        invoke<Cluster[]>("get_clusters"),
        invoke<Episode[]>("get_episodes"),
      ]);

      setClusters(cls);
      setEpisodes(eps);
      setPhase(cls.length === 0 ? "empty" : "ready");
    } catch (e) {
      setError(String(e));
      setPhase("error");
    }
  }

  async function handleFileInput(e: React.ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0];
    if (!file) return;
    const text = await file.text();
    await initWorkspace(text);
    e.target.value = "";
  }

  async function handleCapture(e: React.FormEvent) {
    e.preventDefault();
    if (!captureUrl.trim()) return;
    setCapturing(true);
    try {
      await invoke("add_capture", { url: captureUrl.trim(), title: captureTitle.trim() });
      setCaptureUrl("");
      setCaptureTitle("");
      const [cls, eps] = await Promise.all([
        invoke<Cluster[]>("get_clusters"),
        invoke<Episode[]>("get_episodes"),
      ]);
      setClusters(cls);
      setEpisodes(eps);
    } catch {
      // ignore
    } finally {
      setCapturing(false);
    }
  }

  async function handleDataCleared() {
    setClusters([]);
    setEpisodes([]);
    setImportSummary("");
    setPhase("empty");
  }

  if (phase === "loading") {
    return (
      <div className="workspace workspace--center">
        <p className="workspace__status">Cargando workspace…</p>
      </div>
    );
  }

  if (phase === "error") {
    return (
      <div className="workspace workspace--center">
        <p className="workspace__status workspace__status--error">
          Error al inicializar: {error}
        </p>
      </div>
    );
  }

  if (phase === "empty") {
    return (
      <div className="workspace workspace--center">
        <div className="workspace__onboarding">
          <h1 className="workspace__onboarding-title">FlowWeaver</h1>
          <p className="workspace__onboarding-desc">
            No se encontraron bookmarks automáticamente.
          </p>
          <p className="workspace__onboarding-hint">
            Exporta tus bookmarks a HTML desde Chrome o Firefox y cárgalos:
          </p>
          <label className="workspace__file-label">
            Seleccionar archivo de bookmarks (.html)
            <input
              type="file"
              accept=".html,.htm"
              className="workspace__file-input"
              onChange={handleFileInput}
            />
          </label>
        </div>
      </div>
    );
  }

  return (
    <div className="workspace">
      <div className="workspace__topbar">
        {importSummary && (
          <span className="workspace__import-summary">{importSummary}</span>
        )}
        <div className="workspace__topbar-actions">
          {/* Capture form — simulates Share Extension iOS for desktop testing */}
          <form className="capture-form" onSubmit={handleCapture}>
            <input
              className="capture-form__url"
              type="url"
              placeholder="https://… (capturar URL)"
              value={captureUrl}
              onChange={(e) => setCaptureUrl(e.target.value)}
            />
            <input
              className="capture-form__title"
              type="text"
              placeholder="Título (opcional)"
              value={captureTitle}
              onChange={(e) => setCaptureTitle(e.target.value)}
            />
            <button className="capture-form__btn" type="submit" disabled={capturing}>
              {capturing ? "…" : "Capturar"}
            </button>
          </form>
          <PrivacyDashboard onDataCleared={handleDataCleared} />
        </div>
      </div>

      <AnticipatedWorkspace episodes={episodes} />

      {episodes.length > 0 && <EpisodePanel episodes={episodes} />}

      <div className="workspace__panels">
        <PanelA clusters={clusters} />
        <PanelC clusters={clusters} />
      </div>
    </div>
  );
}

export default App;
