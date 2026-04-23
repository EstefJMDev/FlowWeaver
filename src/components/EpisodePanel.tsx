import { Episode } from "../types";

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

      {episodes.map((ep) => (
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
        </div>
      ))}
    </section>
  );
}
