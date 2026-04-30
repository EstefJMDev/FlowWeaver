/// Anticipated Workspace — Phase 0b.
/// Surfaces the single most actionable precise episode as a focused action zone.
/// Only renders when at least one Precise episode exists.
/// Connects Episode Detector output to Panel C's template system (episode-scoped).

import { Episode } from "../types";
import { CATEGORY_TEMPLATES } from "../templates";

interface Props {
  episodes: Episode[];
}

export function AnticipatedWorkspace({ episodes }: Props) {
  const preciseEpisodes = episodes
    .filter((e) => e.mode === "Precise")
    .sort((a, b) => b.coherence - a.coherence);

  if (preciseEpisodes.length === 0) return null;

  const ep = preciseEpisodes[0];
  const category = ep.resources[0]?.category ?? "otro";
  const actions = (CATEGORY_TEMPLATES[category] ?? CATEGORY_TEMPLATES.otro).slice(0, 3);
  const preview = ep.resources.slice(0, 3);
  const extra = ep.resources.length - preview.length;

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
    </section>
  );
}
