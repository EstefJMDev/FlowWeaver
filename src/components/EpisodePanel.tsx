import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Episode } from "../types";
import { SynthesisConsentModal } from './SynthesisConsentModal';
import { renderMarkdown } from '../utils/renderMarkdown';
import { useSynthesis } from '../hooks/useSynthesis';
import { mapCategoryToSynthesisType, canSynthesize } from '../utils/synthesisCategory';

function EpisodeSynthesisButton({ episode }: { episode: Episode }) {
  const { state, generate } = useSynthesis(episode.episode_id);
  const [showConsent, setShowConsent] = useState(false);
  const [redirectNote, setRedirectNote] = useState(false);

  const category = episode.resources[0]?.category ?? 'otro';
  const payload = {
    category,
    titles: episode.resources.map(r => r.title),
    domains: episode.resources.map(r => r.domain),
    synthesisType: mapCategoryToSynthesisType(category),
    anchorType: 'session' as const,
  };

  async function handleClick() {
    try {
      const consent = await invoke<{ has_consent: boolean }>('check_synthesis_consent');
      if (!consent.has_consent) {
        setShowConsent(true);
        return;
      }
    } catch { return; }
    generate(payload);
  }

  return (
    <div className="episode-card__synthesis">
      {state.status === 'idle' && (
        <button className="episode-card__synth-btn" onClick={handleClick}>
          Generar síntesis
        </button>
      )}
      {state.status === 'loading' && <p>Generando síntesis…</p>}
      {state.status === 'streaming' && (
        <div dangerouslySetInnerHTML={{ __html: renderMarkdown(state.content) }} />
      )}
      {state.status === 'complete' && (
        <div>
          <div dangerouslySetInnerHTML={{ __html: renderMarkdown(state.content) }} />
          <button onClick={() => navigator.clipboard.writeText(state.content)}>
            Copiar
          </button>
        </div>
      )}
      {state.status === 'error' && <p style={{ color: '#f88' }}>{state.message}</p>}
      {redirectNote && (
        <p className="episode-card__redirect-note">
          Para activar la síntesis, ve al Panel de Privacidad (🔒) y activa el toggle.
        </p>
      )}
      {showConsent && (
        <SynthesisConsentModal
          onAccept={() => { setShowConsent(false); setRedirectNote(true); }}
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

            {ep.coherence >= 0.9 && canSynthesize(category) && (
              <EpisodeSynthesisButton episode={ep} />
            )}
          </div>
        );
      })}
    </section>
  );
}
