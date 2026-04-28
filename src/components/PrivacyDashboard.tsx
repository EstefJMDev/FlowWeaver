/// Privacy Dashboard — Phase 0b, D14.
/// Shows aggregate stats (category/domain counts) about locally stored data.
/// Never exposes url or title fields — those stay encrypted (D1).
/// Provides a single "clear all" action with confirmation.

import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { PrivacyStats } from "../types";
import { PatternsSection } from "./PatternsSection";
import { TrustStateSection } from "./TrustStateSection";
import { FsWatcherSection } from "./FsWatcherSection";
import { PrivacyDashboardNeverSeen } from "./PrivacyDashboardNeverSeen";

interface Props {
  onDataCleared: () => void;
}

export function PrivacyDashboard({ onDataCleared }: Props) {
  const [open, setOpen] = useState(false);
  const [stats, setStats] = useState<PrivacyStats | null>(null);
  const [clearing, setClearing] = useState(false);

  useEffect(() => {
    if (!open) return;
    invoke<PrivacyStats>("get_privacy_stats").then(setStats).catch(() => null);
  }, [open]);

  async function handleClear() {
    if (!confirm("¿Eliminar todos los recursos almacenados? Esta acción no se puede deshacer.")) return;
    setClearing(true);
    try {
      await invoke("clear_all_resources");
      setStats(null);
      setOpen(false);
      onDataCleared();
    } catch {
      // ignore
    } finally {
      setClearing(false);
    }
  }

  return (
    <div className="privacy-dashboard">
      <button
        className="privacy-dashboard__toggle"
        onClick={() => setOpen((v) => !v)}
        title="Panel de privacidad"
        aria-expanded={open}
        aria-label="Abrir panel de privacidad"
      >
        🔒
      </button>

      {open && (
        <div className="privacy-dashboard__panel" role="dialog" aria-label="Datos almacenados">
          <header className="privacy-dashboard__header">
            <h3 className="privacy-dashboard__title">Datos almacenados localmente</h3>
            <button
              className="privacy-dashboard__close"
              onClick={() => setOpen(false)}
              aria-label="Cerrar"
            >
              ✕
            </button>
          </header>

          {stats ? (
            <div className="privacy-dashboard__body">
              <section aria-labelledby="pd-recursos">
                <h4 id="pd-recursos">Recursos almacenados</h4>
                <p className="privacy-dashboard__count">
                  <strong>{stats.resource_count}</strong> recursos cifrados
                </p>

                <div className="privacy-dashboard__section">
                  <h4 className="privacy-dashboard__section-title">Por categoría</h4>
                  <ul className="privacy-dashboard__list">
                    {stats.categories.map((c) => (
                      <li key={c.category} className="privacy-dashboard__item">
                        <span className="privacy-dashboard__item-label">{c.category}</span>
                        <span className="privacy-dashboard__item-count">{c.count}</span>
                      </li>
                    ))}
                  </ul>
                </div>

                <div className="privacy-dashboard__section">
                  <h4 className="privacy-dashboard__section-title">Dominios (en claro)</h4>
                  <ul className="privacy-dashboard__list">
                    {stats.domains.slice(0, 10).map((d) => (
                      <li key={d.domain} className="privacy-dashboard__item">
                        <span className="privacy-dashboard__item-label">{d.domain}</span>
                        <span className="privacy-dashboard__item-count">{d.count}</span>
                      </li>
                    ))}
                  </ul>
                </div>

                <button
                  className="privacy-dashboard__clear"
                  onClick={handleClear}
                  disabled={clearing}
                >
                  {clearing ? "Eliminando…" : "Eliminar todos los datos"}
                </button>
              </section>

              <PatternsSection />

              <TrustStateSection />

              <FsWatcherSection />

              <PrivacyDashboardNeverSeen />
            </div>
          ) : (
            <p className="privacy-dashboard__loading">Cargando estadísticas…</p>
          )}
        </div>
      )}
    </div>
  );
}
