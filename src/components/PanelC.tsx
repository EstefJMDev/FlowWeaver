import { Cluster } from "../types";

interface Props {
  clusters: Cluster[];
}

// Static action templates per category (T-0a-006, spec table).
// Baseline: no LLM required; templates function in any environment.
const TEMPLATES: Record<string, string[]> = {
  development: [
    "Revisar el código en los recursos",
    "Ejecutar tests pendientes",
    "Abrir o actualizar issues relevantes",
    "Crear o revisar un PR",
    "Actualizar la documentación",
  ],
  articles: [
    "Leer los artículos marcados",
    "Anotar los puntos clave",
    "Compartir con el equipo si aplica",
    "Crear una nota de síntesis",
  ],
  notes: [
    "Revisar y consolidar las notas",
    "Actualizar enlaces internos",
    "Crear elementos de acción",
    "Archivar notas ya procesadas",
  ],
  design: [
    "Revisar los diseños del grupo",
    "Añadir comentarios de feedback",
    "Compartir para revisión",
    "Comprobar accesibilidad básica",
  ],
  video: [
    "Ver los vídeos marcados",
    "Tomar notas de momentos clave",
    "Crear resumen para el equipo",
  ],
  productivity: [
    "Revisar tareas pendientes del grupo",
    "Actualizar el estado de los ítems",
    "Priorizar los próximos pasos",
  ],
  research: [
    "Sintetizar los hallazgos del grupo",
    "Identificar brechas en la investigación",
    "Crear notas bibliográficas",
    "Planificar siguientes pasos de investigación",
  ],
  social: [
    "Revisar actualizaciones del grupo",
    "Responder a hilos pendientes",
    "Guardar contenido relevante para referencia",
  ],
  commerce: [
    "Revisar productos o servicios marcados",
    "Comparar opciones disponibles",
    "Crear lista de evaluación o compra",
  ],
  other: [
    "Revisar el contenido del grupo",
    "Organizar en notas propias",
    "Identificar próximas acciones",
  ],
};

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
            const actions = TEMPLATES[cat] ?? TEMPLATES.other;
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
