import { describe, it, expect } from 'vitest';
import { renderMarkdown } from './renderMarkdown';

describe('renderMarkdown', () => {
  it('escapa script tags — no aparece <script> literal en el output', () => {
    const output = renderMarkdown('<script>alert(1)</script>');
    expect(output).not.toContain('<script>');
    expect(output).toContain('&lt;script&gt;');
  });

  it('escapa img onerror dentro de heading — <img> queda escapado', () => {
    const output = renderMarkdown('## Título <img onerror=x>');
    expect(output).toContain('<h2>');
    expect(output).not.toContain('<img');
    expect(output).toContain('&lt;img');
  });

  it('sigue renderizando negrita correctamente', () => {
    const output = renderMarkdown('**negrita**');
    expect(output).toContain('<strong>negrita</strong>');
  });

  it('renderiza heading y body con br', () => {
    const output = renderMarkdown('## Heading\n\nbody');
    expect(output).toContain('<h2>Heading</h2>');
    expect(output).toContain('<br/>');
    expect(output).toContain('body');
  });
});
