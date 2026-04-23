import { Cluster } from "../types";
import { CATEGORY_TEMPLATES } from "../templates";

interface Props {
  clusters: Cluster[];
}

export function PanelC({ clusters }: Props) {
  // Deduplicate by category; order preserved by first occurrence.
  const seen = new Set<string>();
  const categories: string[] = [];
  for (const c of clusters) {
    if (!seen.has(c.category)) {
      seen.add(c.category);
      categories.push(c.category);
    }
  }

  return (
    <section className="panel-c" aria-label="Siguientes pasos">
      <header className="panel-c__header">
        <h2 className="panel-c__title">Siguientes pasos</h2>
      </header>

      {categories.length === 0 ? (
        <p className="panel-c__empty">Sin categorías en el workspace.</p>
      ) : (
        <div className="panel-c__sections">
          {categories.map((cat) => {
            const actions = CATEGORY_TEMPLATES[cat] ?? CATEGORY_TEMPLATES.other;
            return (
              <div key={cat} className="panel-c__section">
                <div className={`panel-c__category-header panel-c__category-header--${cat}`}>
                  {cat}
                </div>
                <ul className="panel-c__actions">
                  {actions.map((action, i) => {
                    const id = `${cat}-action-${i}`;
                    return (
                      <li key={id} className="panel-c__action">
                        <input
                          type="checkbox"
                          id={id}
                          className="panel-c__checkbox"
                        />
                        <label htmlFor={id} className="panel-c__action-label">
                          {action}
                        </label>
                      </li>
                    );
                  })}
                </ul>
              </div>
            );
          })}
        </div>
      )}
    </section>
  );
}
