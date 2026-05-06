export const SYNTHESIS_CATEGORY_MAP: Record<string, string> = {
  cocina:           'cocina',
  recetas:          'cocina',
  gastronomia:      'cocina',
  entretenimiento:  'entretenimiento',
  cine:             'entretenimiento',
  musica:           'entretenimiento',
  juegos:           'entretenimiento',
  noticias:         'noticias',
  actualidad:       'noticias',
  tecnologia:       'tecnologia',
  programacion:     'tecnologia',
  desarrollo:       'tecnologia',
};

export function mapCategoryToSynthesisType(category: string): string {
  return SYNTHESIS_CATEGORY_MAP[category.toLowerCase()] ?? 'noticias';
}

export function canSynthesize(category: string): boolean {
  return category.toLowerCase() in SYNTHESIS_CATEGORY_MAP;
}
