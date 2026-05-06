/// Anticipated Workspace — Phase 0b.
/// Surfaces the single most actionable episode (Precise preferred, Broad fallback)
/// Only renders when at least one episode exists.
/// Connects Episode Detector output to Panel C's template system (episode-scoped).

import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Episode, TrustStateView, TrustStateEnum } from "../types";
import { CATEGORY_TEMPLATES } from "../templates";
import { SynthesisView } from './SynthesisView';
import { mapCategoryToSynthesisType } from '../utils/synthesisCategory';

interface Props {
  episodes: Episode[];
}

export function AnticipatedWorkspace({ episodes }: Props) {
  const [trustState, setTrustState] = useState<TrustStateEnum | null>(null);
  const [redirectNote, setRedirectNote] = useState(false);

  useEffect(() => {
    invoke<TrustStateView>('get_trust_state')
      .then(v => setTrustState(v.current_state))
      .catch(() => null);
  }, []);

  function latestCapture(ep: Episode): number {
    return Math.max(...ep.resources.map(r => r.captured_at), 0);
  }

  const sortedEpisodes = [...episodes].sort(
    (a, b) => latestCapture(b) - latestCapture(a)
  );

  const bestPrecise = sortedEpisodes.find(e => e.mode === "Precise");
  const bestBroad   = sortedEpisodes.find(e => e.mode === "Broad");

  let ep: Episode | undefined;
  if (bestPrecise && bestBroad) {
    ep = latestCapture(bestPrecise) >= latestCapture(bestBroad)
      ? bestPrecise
      : bestBroad;
  } else {
    ep = bestPrecise ?? bestBroad;
  }

  if (!ep) return null;
  const category = ep.resources[0]?.category ?? "otro";
  const actions = (CATEGORY_TEMPLATES[category] ?? CATEGORY_TEMPLATES.otro).slice(0, 3);
  const preview = ep.resources.slice(0, 3);
  const extra = ep.resources.length - preview.length;

  const synthesisProps = {
    anchorKey:     ep.episode_id,
    anchorType:    'session' as const,
    category:      category,
    synthesisType: mapCategoryToSynthesisType(category),
    titles:        ep.resources.map(r => r.title),
    domains:       ep.resources.map(r => r.domain),
  };

  // Defensa en profundidad (D25): si consent falta al pulsar Generar,
  // redirige al Privacy Dashboard en lugar de intentar grabar consent aquí.
  async function handleGenerateRequest() {
    const consent = await invoke<{ has_consent: boolean }>('check_synthesis_consent');
    if (!consent.has_consent) {
      setRedirectNote(true);
      throw new Error('consent needed');
    }
  }

  return (
    <section className="anticipated-workspace" aria-label="Workspace anticipatorio">
      <div className="anticipated-workspace__badge-row">
        <span className="anticipated-workspace__badge">Próxima tarea</span>
        <span className="anticipated-workspace__label">{ep.label}</span>
        <span className="anticipated-workspace__meta">
          {ep.resources.length} recursos · {Math.round(ep.coherence * 100)}% coherencia
        </span>
      </div>

      <div className="anticipated-workspace__body">
        <div className="anticipated-workspace__resources">
          {preview.map((r) => (
            <span key={r.uuid} className="anticipated-workspace__resource-chip">
              <span className="anticipated-workspace__chip-icon">
                {r.domain.charAt(0).toUpperCase()}
              </span>
              <span className="anticipated-workspace__chip-title">{r.title}</span>
            </span>
          ))}
          {extra > 0 && (
            <span className="anticipated-workspace__resource-chip anticipated-workspace__resource-chip--more">
              +{extra} más
            </span>
          )}
        </div>

        <ul className="anticipated-workspace__actions">
          {actions.map((action, i) => {
            const id = `aw-action-${ep.episode_id}-${i}`;
            return (
              <li key={id} className="anticipated-workspace__action">
                <input
                  type="checkbox"
                  id={id}
                  className="anticipated-workspace__checkbox"
                />
                <label htmlFor={id} className="anticipated-workspace__action-label">
                  {action}
                </label>
              </li>
            );
          })}
        </ul>
      </div>

      {redirectNote && (
        <p className="anticipated-workspace__redirect-note">
          Para activar la síntesis, ve al Panel de Privacidad (🔒) y activa el toggle.
        </p>
      )}

      {(trustState === 'Trusted' || trustState === 'Autonomous') && (
        <SynthesisView
          {...synthesisProps}
          onRequest={trustState === 'Trusted' ? handleGenerateRequest : undefined}
        />
      )}
    </section>
  );
}
