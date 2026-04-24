import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { PrivacyStats } from "../types";

// ── Props ─────────────────────────────────────────────────────────────────────

interface MobilePrivacyDashboardProps {
  onClose: () => void;
  onDataCleared: () => void;
}

// ── Component ─────────────────────────────────────────────────────────────────

export function MobilePrivacyDashboard({
  onClose,
  onDataCleared,
}: MobilePrivacyDashboardProps) {
  const [stats, setStats] = useState<PrivacyStats | null>(null);
  const [loading, setLoading] = useState(true);
  const [clearing, setClearing] = useState(false);

  useEffect(() => {
    loadStats();
  }, []);

  async function loadStats() {
    setLoading(true);
    try {
      const data = await invoke<PrivacyStats>("get_privacy_stats");
      setStats(data);
    } finally {
      setLoading(false);
    }
  }

  async function handleClearData() {
    const confirmed = window.confirm(
      "¿Eliminar todos tus datos del móvil? Esta acción no se puede deshacer."
    );
    if (!confirmed) return;

    setClearing(true);
    try {
      await invoke("clear_all_resources");
      onClose();
      onDataCleared();
    } finally {
      setClearing(false);
    }
  }

  return (
    <div className="mobile-privacy__overlay" role="dialog" aria-modal="true">
      <div className="mobile-privacy__sheet">

        {/* Header */}
        <div className="mobile-privacy__header">
          <span className="mobile-privacy__title">Privacidad</span>
          <button
            className="mobile-privacy__close-btn"
            onClick={onClose}
            type="button"
            aria-label="Cerrar"
          >
            ✕
          </button>
        </div>

        <div className="mobile-privacy__body">

          {/* Sección 1 — Recuento por categoría */}
          <section className="mobile-privacy__section">
            <p className="mobile-privacy__section-title">En este dispositivo</p>

            {loading && (
              <p className="mobile-privacy__loading">Cargando estadísticas…</p>
            )}

            {!loading && stats && (
              <>
                <p className="mobile-privacy__total">
                  <strong>{stats.resource_count}</strong>{" "}
                  {stats.resource_count === 1 ? "recurso guardado" : "recursos guardados"}
                </p>

                {stats.categories.length > 0 && (
                  <ul className="mobile-privacy__category-list">
                    {stats.categories.map((c) => (
                      <li key={c.category} className="mobile-privacy__category-item">
                        <span className="mobile-privacy__category-name">
                          {c.category}
                        </span>
                        <span className="mobile-privacy__category-count">
                          {c.count}
                        </span>
                      </li>
                    ))}
                  </ul>
                )}
              </>
            )}
          </section>

          {/* Sección 2 — Texto de transparencia */}
          <section className="mobile-privacy__section">
            <p className="mobile-privacy__section-title">
              Qué guarda FlowWeaver en este dispositivo
            </p>
            <p className="mobile-privacy__notice">
              El nombre de los sitios que compartiste (domain), la categoría
              asignada y la fecha de captura. El enlace completo (URL) y el
              título están cifrados en tu dispositivo. Otra app o persona con
              acceso físico normal no puede leerlos.
            </p>
          </section>

          <section className="mobile-privacy__section">
            <p className="mobile-privacy__section-title">Qué nunca guarda</p>
            <p className="mobile-privacy__notice">
              El contenido de las páginas, tus contraseñas ni tus datos de
              cuenta.
            </p>
          </section>

          {/* Sección 3 — Indicador de relay */}
          <section className="mobile-privacy__section">
            <p className="mobile-privacy__section-title">Almacenamiento relay</p>
            <p className="mobile-privacy__notice">
              Cuando sincronizas con otro dispositivo, tus capturas se transmiten
              cifradas a través de una carpeta privada de Google Drive que solo
              FlowWeaver puede ver.
            </p>
          </section>

          {/* Sección 4 — Botón de borrado */}
          <section className="mobile-privacy__section mobile-privacy__section--danger">
            <button
              className="mobile-privacy__clear-btn"
              onClick={handleClearData}
              disabled={clearing}
              type="button"
            >
              {clearing ? "Eliminando…" : "Eliminar todos mis datos del móvil"}
            </button>
          </section>

        </div>
      </div>
    </div>
  );
}
