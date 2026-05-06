import { describe, it, expect } from 'vitest';
import { mapCategoryToSynthesisType, canSynthesize } from './synthesisCategory';

describe('mapCategoryToSynthesisType', () => {
  it('normaliza tilde: música → musica', () => {
    expect(mapCategoryToSynthesisType('música')).toBe('musica');
  });
  it('normaliza tilde: tecnología → tecnologia', () => {
    expect(mapCategoryToSynthesisType('tecnología')).toBe('tecnologia');
  });
  it('normaliza tilde y eñe: diseño → tecnologia', () => {
    expect(mapCategoryToSynthesisType('diseño')).toBe('tecnologia');
  });
  it('categoría sin síntesis → null', () => {
    expect(mapCategoryToSynthesisType('comercio')).toBeNull();
  });
  it('educación con tilde → educacion', () => {
    expect(mapCategoryToSynthesisType('educación')).toBe('educacion');
  });
  it('vídeo con tilde → entretenimiento', () => {
    expect(mapCategoryToSynthesisType('vídeo')).toBe('entretenimiento');
  });
});

describe('canSynthesize', () => {
  it('música → true', () => {
    expect(canSynthesize('música')).toBe(true);
  });
  it('comercio → false', () => {
    expect(canSynthesize('comercio')).toBe(false);
  });
  it('otro → false', () => {
    expect(canSynthesize('otro')).toBe(false);
  });
  it('gaming → true', () => {
    expect(canSynthesize('gaming')).toBe(true);
  });
  it('salud → true', () => {
    expect(canSynthesize('salud')).toBe(true);
  });
});
