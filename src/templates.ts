// Static action templates per category (T-0a-006).
// Shared by PanelC (all categories) and AnticipatedWorkspace (single episode).
// Baseline: no LLM required (D8).
export const CATEGORY_TEMPLATES: Record<string, string[]> = {
  desarrollo: [
    "Revisar el código en los recursos",
    "Ejecutar tests pendientes",
    "Abrir o actualizar issues relevantes",
    "Crear o revisar un PR",
    "Actualizar la documentación",
  ],
  artículos: [
    "Leer los artículos marcados",
    "Anotar los puntos clave",
    "Compartir con el equipo si aplica",
    "Crear una nota de síntesis",
  ],
  notas: [
    "Revisar y consolidar las notas",
    "Actualizar enlaces internos",
    "Crear elementos de acción",
    "Archivar notas ya procesadas",
  ],
  diseño: [
    "Revisar los diseños del grupo",
    "Añadir comentarios de feedback",
    "Compartir para revisión",
    "Comprobar accesibilidad básica",
  ],
  "vídeo": [
    "Ver los vídeos marcados",
    "Tomar notas de momentos clave",
    "Crear resumen para el equipo",
  ],
  productividad: [
    "Revisar tareas pendientes del grupo",
    "Actualizar el estado de los ítems",
    "Priorizar los próximos pasos",
  ],
  investigación: [
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
  comercio: [
    "Revisar productos o servicios marcados",
    "Comparar opciones disponibles",
    "Crear lista de evaluación o compra",
  ],
  entretenimiento: [
    "Ver el contenido pendiente",
    "Tomar notas o valoraciones",
    "Compartir recomendaciones con contactos",
  ],
  gaming: [
    "Revisar juegos o noticias guardados",
    "Consultar guías o walkthroughs pendientes",
    "Actualizar lista de juegos por explorar",
  ],
  noticias: [
    "Leer los artículos de actualidad guardados",
    "Identificar temas relevantes para el trabajo",
    "Guardar resumen de lo más importante",
  ],
  "educación": [
    "Continuar el curso o lección pendiente",
    "Tomar notas del material de aprendizaje",
    "Revisar ejercicios o tareas asignadas",
  ],
  "música": [
    "Escuchar la lista o álbum guardado",
    "Añadir tracks a playlist de trabajo",
    "Explorar artistas relacionados",
  ],
  otro: [
    "Revisar el contenido del grupo",
    "Organizar en notas propias",
    "Identificar próximas acciones",
  ],
};
