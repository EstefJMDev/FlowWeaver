import { Cluster, Episode } from "../types";
import { CATEGORY_TEMPLATES } from "../templates";

interface Props {
  clusters: Cluster[];
  episodes?: Episode[];
}

function topPreciseEpisode(episodes: Episode[]): Episode | undefined {
  return episodes
    .filter((e) => e.mode === "Precise")
    .sort((a, b) => b.coherence - a.coherence)[0];
}

function episodeDominantCategory(episode: Episode): string {
  const counts: Record<string, number> = {};
  for (const r of episode.resources) {
    counts[r.category] = (counts[r.category] ?? 0) + 1;
  }
  const sorted = Object.entries(counts).sort((a, b) => b[1] - a[1]);
  return sorted[0]?.[0] ?? "other";
}

// Returns 2–4 lines. Only uses domain, category, and resource count (D1: no url/title).
function buildSummaryLines(cluster: Cluster, episodeLabel?: string): string[] {
  const count = cluster.resources.length;
  const templates = CATEGORY_TEMPLATES[cluster.category] ?? CATEGORY_TEMPLATES["other"];
  const lines: string[] = [];

  lines.push(`${count} recurso${count !== 1 ? "s" : ""} en ${cluster.domain}`);
  lines.push(templates[0]);
  if (count >= 3 || episodeLabel) {
    lines.push(templates[1] ?? templates[0]);
  }
  if (episodeLabel) {
    lines.push(`Episodio activo: ${episodeLabel}`);
  }
  return lines.slice(0, 4);
}

export function PanelB({ clusters, episodes = [] }: Props) {
  const topEpisode = topPreciseEpisode(episodes);
  const episodeCategory = topEpisode ? episodeDominantCategory(topEpisode) : undefined;

  return (
    <section className="panel-b" aria-label="Resumen del workspace">
      <header className="panel-b__header">
        <h2 className="panel-b__title">Resumen</h2>
        {clusters.length > 0 && (
          <span className="panel-b__cluster-count">{clusters.length} grupos</span>
        )}
        {topEpisode && (
          <span className="panel-b__episode-badge">{topEpisode.label}</span>
        )}
      </header>

      {clusters.length === 0 ? (
        <p className="panel-b__empty">No hay clusters para resumir.</p>
      ) : (
        <div className="panel-b__cards">
          {clusters.map((cluster) => {
            const episodeLabel =
              episodeCategory === cluster.category ? topEpisode?.label : undefined;
            const lines = buildSummaryLines(cluster, episodeLabel);

            return (
              <div key={cluster.group_key} className="panel-b__card">
                <div className="panel-b__card-header">
                  <span className="panel-b__domain">{cluster.domain}</span>
                  <span
                    className={`panel-b__category panel-b__category--${cluster.category}`}
                  >
                    {cluster.category}
                  </span>
                </div>
                <ul className="panel-b__lines">
                  {lines.map((line, i) => (
                    <li
                      key={i}
                      className={`panel-b__line${
                        i === lines.length - 1 && episodeLabel
                          ? " panel-b__line--episode"
                          : ""
                      }`}
                    >
                      {line}
                    </li>
                  ))}
                </ul>
              </div>
            );
          })}
        </div>
      )}
    </section>
  );
}
