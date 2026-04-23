// Static action templates per category (T-0a-006).
// Shared by PanelC (all categories) and AnticipatedWorkspace (single episode).
// Baseline: no LLM required (D8).
export const CATEGORY_TEMPLATES: Record<string, string[]> = {
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
