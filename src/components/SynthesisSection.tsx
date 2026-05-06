import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { SynthesisOnboarding } from './SynthesisOnboarding';

interface SynthesisUsage {
  used_this_month: number;
  limit_this_month: number;
  synthesis_active: boolean;
}

export function SynthesisSection() {
  const [usage, setUsage] = useState<SynthesisUsage | null>(null);
  const [showOnboarding, setShowOnboarding] = useState(false);

  function refresh() {
    invoke<SynthesisUsage>('get_synthesis_usage').then(setUsage).catch(() => null);
  }

  useEffect(() => {
    refresh();
  }, []);

  async function handleToggle(e: React.ChangeEvent<HTMLInputElement>) {
    if (!e.target.checked) {
      const ok = confirm(
        "Al desactivar la síntesis, tu token de acceso beta se eliminará " +
        "de este dispositivo. Necesitarás introducirlo de nuevo para reactivarla. " +
        "¿Confirmas?"
      );
      if (!ok) return;
      await invoke("clear_synthesis_token").catch(() => null);
      refresh();
    } else {
      setShowOnboarding(true);
    }
  }

  return (
    <section aria-labelledby="pd-sintesis">
      <h4 id="pd-sintesis">Síntesis inteligente</h4>

      {/* Elemento 1 — Qué se envía (PG-002, texto EXACTO) */}
      <p className="synthesis__description">
        Cuando solicitas una síntesis, FlowWeaver envía al proxy únicamente:
        la categoría del episodio, los títulos de las páginas que guardaste,
        y los dominios. Nunca se envía la URL completa ni el contenido de las páginas.
      </p>

      {/* Elemento 2 — Destino + referencia política (PG-005) */}
      <p className="synthesis__destination">
        Los datos se envían al{" "}
        <strong>Proxy FlowWeaver en Cloudflare (zero-retention)</strong>.{" "}
        <a
          href="https://developers.cloudflare.com/workers-ai/privacy/"
          target="_blank"
          rel="noopener noreferrer"
        >
          Política de privacidad de Cloudflare Workers AI
        </a>
      </p>

      {/* Elemento 3 — Política de retención (PG-002, texto EXACTO) */}
      <p className="synthesis__retention">
        El proxy no almacena tu contenido. La síntesis generada se guarda
        solo en tu dispositivo, cifrada.
      </p>

      {/* Elemento 4 — Toggle activación/desactivación (PG-006) */}
      <div className="synthesis__toggle">
        <label htmlFor="synthesis-toggle">Síntesis activa</label>
        <input
          id="synthesis-toggle"
          type="checkbox"
          checked={usage?.synthesis_active ?? false}
          onChange={handleToggle}
          aria-describedby="synthesis-toggle-desc"
        />
        <span id="synthesis-toggle-desc" className="synthesis__toggle-note">
          Al desactivar, tu token de acceso se elimina de este dispositivo.
        </span>
      </div>

      {/* Elemento 5 — Contador de uso */}
      {usage?.synthesis_active && (
        <p className="synthesis__counter">
          {usage.used_this_month} de {usage.limit_this_month} síntesis usadas este mes
        </p>
      )}

      {/* Onboarding condicional — mostrado al activar */}
      {showOnboarding && (
        <SynthesisOnboarding
          onComplete={() => { setShowOnboarding(false); refresh(); }}
          onSkip={() => setShowOnboarding(false)}
        />
      )}
    </section>
  );
}
