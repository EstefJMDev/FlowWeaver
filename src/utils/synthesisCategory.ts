function normalizeCategory(category: string): string {
  return category
    .toLowerCase()
    .normalize('NFD')
    .replace(/[̀-ͯ]/g, '')
    .replace(/ñ/g, 'n'); // ñ → n (NFD no descompone ñ en todos los entornos)
}

// Claves siempre normalizadas (sin tilde, sin eñe, lowercase).
// Categorías no presentes → null → sin botón de síntesis (canSynthesize = false).
const SYNTHESIS_CATEGORY_MAP: Record<string, string> = {
  cocina:        'cocina',
  recetas:       'cocina',
  gastronomia:   'cocina',
  cine:          'entretenimiento',
  streaming:     'entretenimiento',
  video:         'entretenimiento',
  gaming:        'gaming',
  juegos:        'gaming',
  musica:        'musica',
  noticias:      'noticias',
  actualidad:    'noticias',
  gobierno:      'noticias',
  tecnologia:    'tecnologia',
  programacion:  'tecnologia',
  desarrollo:    'tecnologia',
  diseno:        'tecnologia',
  productividad: 'tecnologia',
  ciencia:       'ciencia',
  investigacion: 'ciencia',
  viajes:        'viajes',
  salud:         'salud',
  deportes:      'deportes',
  finanzas:      'finanzas',
  educacion:     'educacion',
};

export function mapCategoryToSynthesisType(category: string): string | null {
  return SYNTHESIS_CATEGORY_MAP[normalizeCategory(category)] ?? null;
}

export function canSynthesize(category: string): boolean {
  return mapCategoryToSynthesisType(category) !== null;
}
