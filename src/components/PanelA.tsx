import { Cluster } from "../types";

interface Props {
  clusters: Cluster[];
}

export function PanelA({ clusters }: Props) {
  const total = clusters.reduce((n, c) => n + c.resources.length, 0);

  return (
    <section className="panel-a" aria-label="Recursos agrupados">
      <header className="panel-a__header">
        <h2 className="panel-a__title">Recursos</h2>
        {total > 0 && (
          <span className="panel-a__total">{total} total</span>
        )}
      </header>

      {clusters.length === 0 ? (
        <p className="panel-a__empty">No hay recursos en el workspace.</p>
      ) : (
        <div className="panel-a__groups">
          {clusters.map((cluster) => (
            <div key={cluster.group_key} className="panel-a__group">
              <div className="panel-a__group-header">
                <span className="panel-a__domain">{cluster.domain}</span>
                <span className="panel-a__sep" aria-hidden>·</span>
                <span className={`panel-a__category panel-a__category--${cluster.category}`}>
                  {cluster.category}
                </span>
                {cluster.sub_label && (
                  <>
                    <span className="panel-a__sep" aria-hidden>·</span>
                    <span className="panel-a__sub-label">{cluster.sub_label}</span>
                  </>
                )}
                <span className="panel-a__count">{cluster.resources.length}</span>
              </div>

              <ul className="panel-a__resources">
                {cluster.resources.map((r) => (
                  <li key={r.uuid} className="panel-a__resource">
                    <span className="panel-a__favicon" aria-hidden>
                      {r.domain.charAt(0).toUpperCase()}
                    </span>
                    <span className="panel-a__resource-title">{r.title}</span>
                    <span className="panel-a__resource-domain">{r.domain}</span>
                  </li>
                ))}
              </ul>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}
