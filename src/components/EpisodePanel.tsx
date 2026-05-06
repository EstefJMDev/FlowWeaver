import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Episode } from "../types";
import { SynthesisConsentModal } from './SynthesisConsentModal';
import { renderMarkdown } from '../utils/renderMarkdown';

const SYNTHESIS_CATEGORY_MAP: Record<string, string> = {
  cocina: 'cocina', recetas: 'cocina', gastronomia: 'cocina',
  entretenimiento: 'entretenimiento', cine: 'entretenimiento',
  musica: 'entretenimiento', juegos: 'entretenimiento',
  noticias: 'noticias', actualidad: 'noticias',
  tecnologia: 'tecnologia', programacion: 'tecnologia', desarrollo: 'tecnologia',
};

function canSynthesize(category: string): boolean {
  return category.toLowerCase() in SYNTHESIS_CATEGORY_MAP;
}

function mapCategory(category: string): string {
  return SYNTHESIS_CATEGORY_MAP[category.toLowerCase()] ?? 'noticias';
}


function EpisodeSynthesisButton({ episode }: { episode: Episode }) {
  const [status, setStatus] = useState<'idle' | 'loading' | 'streaming' | 'complete' | 'error'>('idle');
  const [content, setContent] = useState('');
  const [error, setError] = useState('');
  const [showConsent, setShowConsent] = useState(false);

  async function handleClick() {
    try {
      const consent = await invoke<{ has_consent: boolean }>('check_synthesis_consent');
      if (!consent.has_consent) {
        setShowConsent(true);
        return;
      }
    } catch { return; }
    generate();
  }

  async function generate() {
    setStatus('loading');
    setContent('');
    const category = episode.resources[0]?.category ?? 'otro';

    let contentAccum = '';
    const unlisten1 = await listen<{ anchor_key: string; chunk: string }>(
      'synthesis_chunk', (e) => {
        if (e.payload.anchor_key !== episode.episode_id) return;
        contentAccum += e.payload.chunk;
        setContent(contentAccum);
        setStatus('streaming');
      }
    );
    const unlisten2 = await listen<{ anchor_key: string }>(
      'synthesis_complete', (e) => {
        if (e.payload.anchor_key !== episode.episode_id) return;
        setStatus('complete');
        unlisten1(); unlisten2(); unlisten3();
      }
    );
    const unlisten3 = await listen<{ anchor_key: string; error: string }>(
      'synthesis_error', (e) => {
        if (e.payload.anchor_key !== episode.episode_id) return;
        setError(e.payload.error);
        setStatus('error');
        unlisten1(); unlisten2(); unlisten3();
      }
    );

    try {
      await invoke('generate_synthesis', {
        category,
        titles: episode.resources.map(r => r.title),
        domains: episode.resources.map(r => r.domain),
        synthesisType: mapCategory(category),
        anchorKey: episode.episode_id,
        anchorType: 'session',
      });
    } catch (e) {
      setError(String(e));
      setStatus('error');
      unlisten1(); unlisten2(); unlisten3();
    }
  }

  return (
    <div className="episode-card__synthesis">
      {status === 'idle' && (
        <button className="episode-card__synth-btn" onClick={handleClick}>
          Generar síntesis
        </button>
      )}
      {status === 'loading' && <p>Generando síntesis…</p>}
      {(status === 'streaming' || status === 'complete') && (
        <div>
          <div dangerouslySetInnerHTML={{ __html: renderMarkdown(content) }} />
          {status === 'complete' && (
            <button onClick={() => navigator.clipboard.writeText(content)}>
              Copiar
            </button>
          )}
        </div>
      )}
      {status === 'error' && <p style={{ color: '#f88' }}>{error}</p>}
      {showConsent && (
        <SynthesisConsentModal
          onAccept={() => { setShowConsent(false); generate(); }}
          onDecline={() => setShowConsent(false)}
        />
      )}
    </div>
  );
}

interface Props {
  episodes: Episode[];
}

export function EpisodePanel({ episodes }: Props) {
  if (episodes.length === 0) return null;

  return (
    <section className="episode-panel" aria-label="Episodios activos">
      <header className="episode-panel__header">
        <span className="episode-panel__badge">Episodio activo</span>
        <h2 className="episode-panel__title">
          {episodes.length === 1 ? "1 episodio detectado" : `${episodes.length} episodios detectados`}
        </h2>
      </header>

      {episodes.map((ep) => {
        const category = ep.resources[0]?.category ?? 'otro';
        return (
          <div key={ep.episode_id} className={`episode-card episode-card--${ep.mode.toLowerCase()}`}>
            <div className="episode-card__meta">
              <span className="episode-card__label">{ep.label}</span>
              <span className={`episode-card__mode episode-card__mode--${ep.mode.toLowerCase()}`}>
                {ep.mode === "Precise" ? "Preciso" : "Amplio"}
              </span>
              <span className="episode-card__coherence">
                {Math.round(ep.coherence * 100)}% coherencia
              </span>
              <span className="episode-card__count">{ep.resources.length} recursos</span>
            </div>

            <ul className="episode-card__resources">
              {ep.resources.map((r) => (
                <li key={r.uuid} className="episode-card__resource">
                  <span className="episode-card__favicon" aria-hidden>
                    {r.domain.charAt(0).toUpperCase()}
                  </span>
                  <span className="episode-card__resource-title">{r.title}</span>
                  <span className="episode-card__resource-domain">{r.domain}</span>
                </li>
              ))}
            </ul>

            {ep.coherence === 1.0 && canSynthesize(category) && (
              <EpisodeSynthesisButton episode={ep} />
            )}
          </div>
        );
      })}
    </section>
  );
}
