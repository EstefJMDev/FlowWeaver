import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { TrustStateView, TrustStateEnum } from "../types";

const STATE_LABEL: Record<TrustStateEnum, string> = {
  Observing: "Observando",
  Learning: "Aprendiendo",
  Trusted: "Confiando",
  Autonomous: "Autónomo",
};

export function TrustStateSection() {
  const [view, setView] = useState<TrustStateView | null>(null);
  const [pending, setPending] = useState(false);

  useEffect(() => { refresh(); }, []);

  async function refresh() {
    try { setView(await invoke<TrustStateView>("get_trust_state")); }
    catch { setView(null); }
  }

  async function reset() {
    if (!confirm("¿Resetear el estado de confianza? El sistema volverá a Observando.")) return;
    setPending(true);
    try { setView(await invoke<TrustStateView>("reset_trust_state")); }
    finally { setPending(false); }
  }

  async function activateAutonomous() {
    const ok = confirm(
      "Vas a activar el modo autónomo.\n\n" +
      "El sistema aplicará automáticamente las preparaciones que coinciden con tus patrones de confianza, " +
      "sin pedir confirmación cada vez. Podrás resetear esto cuando quieras.\n\n" +
      "¿Confirmas la activación?"
    );
    if (!ok) return;
    setPending(true);
    try {
      setView(await invoke<TrustStateView>("enable_autonomous_mode", { confirmed: true }));
    } finally {
      setPending(false);
    }
  }

  if (!view) {
    return (
      <section aria-labelledby="pd-confianza">
        <h4 id="pd-confianza">Estado de confianza</h4>
        <p>Cargando…</p>
      </section>
    );
  }

  return (
    <section aria-labelledby="pd-confianza">
      <h4 id="pd-confianza">Estado de confianza</h4>
      <p className="trust__current">
        Estado actual: <strong>{STATE_LABEL[view.current_state]}</strong>
      </p>
      <p className="trust__meta">
        Patrones activos: {view.active_patterns_count} ·
        {" "}última transición hace {formatRelative(view.last_transition_at)}
      </p>
      <div className="trust__actions">
        <button onClick={reset} disabled={pending}>Resetear confianza</button>
        {view.current_state === "Trusted" && (
          <button onClick={activateAutonomous} disabled={pending} className="trust__autonomous">
            Activar preparación automática
          </button>
        )}
      </div>
    </section>
  );
}

function formatRelative(unixSec: number): string {
  const diffSec = Math.max(0, Math.floor(Date.now() / 1000) - unixSec);
  if (diffSec < 3600) return "menos de 1 h";
  if (diffSec < 86400) return `${Math.floor(diffSec / 3600)} h`;
  return `${Math.floor(diffSec / 86400)} días`;
}
