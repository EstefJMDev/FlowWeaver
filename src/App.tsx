import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { PanelA } from "./components/PanelA";
import { PanelC } from "./components/PanelC";
import { Cluster, ImportResult } from "./types";
import "./App.css";

type Phase = "loading" | "ready" | "empty" | "error";

function App() {
  const [phase, setPhase] = useState<Phase>("loading");
  const [clusters, setClusters] = useState<Cluster[]>([]);
  const [importSummary, setImportSummary] = useState<string>("");
  const [error, setError] = useState<string>("");
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
        // HTML content from frontend file picker
        ir = await invoke<ImportResult>("import_bookmarks_html", {
          content: htmlContent,
        });
      } else {
        // Auto-detect Chrome / Edge / Brave bookmark files
        ir = await invoke<ImportResult>("import_bookmarks", { path: null });
      }

      if (ir.imported > 0 || ir.skipped > 0) {
        setImportSummary(
          `${ir.imported} importados, ${ir.skipped} ya existentes`
        );
      }

      const cls = await invoke<Cluster[]>("get_clusters");
      setClusters(cls);
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
    // Reset input so same file can be re-selected
    e.target.value = "";
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
      {importSummary && (
        <div className="workspace__import-banner">{importSummary}</div>
      )}
      <div className="workspace__panels">
        <PanelA clusters={clusters} />
        <PanelC clusters={clusters} />
      </div>
    </div>
  );
}

export default App;
