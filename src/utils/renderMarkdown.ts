/// Renderer Markdown inline mínimo para síntesis del proxy.
/// Soporta h2-h4 y negrita. Sin dependencias externas.
export function renderMarkdown(text: string): string {
  return text
    .replace(/^#### (.+)$/gm, '<h4>$1</h4>')
    .replace(/^### (.+)$/gm, '<h3>$1</h3>')
    .replace(/^## (.+)$/gm, '<h2>$1</h2>')
    .replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>')
    .replace(/\n/g, '<br/>');
}
