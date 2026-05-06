import { useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

type SynthesisState =
  | { status: 'idle' }
  | { status: 'loading' }
  | { status: 'streaming'; content: string }
  | { status: 'complete'; content: string }
  | { status: 'error'; message: string };

interface SynthesisViewProps {
  anchorKey: string;
  anchorType: 'pattern' | 'session';
  category: string;
  synthesisType: string;
  titles: string[];
  domains: string[];
  onRequest?: () => Promise<void>;
}

function renderMarkdown(text: string): string {
  return text
    .replace(/^## (.+)$/gm, '<h2>$1</h2>')
    .replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>')
    .replace(/\n/g, '<br/>');
}

function mapError(backendError: string): string {
  if (backendError.includes('NoConnectivity') || backendError.includes('NO_CONNECTIVITY'))
    return 'Sin conexión — el proxy no está disponible.';
  if (backendError.includes('RateLimitExceeded') || backendError.includes('RATE_LIMIT_EXCEEDED'))
    return 'Has alcanzado el límite de 5 síntesis al mes.';
  if (backendError.includes('InvalidToken') || backendError.includes('INVALID_TOKEN'))
    return 'Token de acceso no válido. Contacta con el equipo.';
  if (backendError.includes('NoConsent') || backendError.includes('NO_CONSENT'))
    return 'Activa la síntesis desde el Privacy Dashboard primero.';
  return 'El servicio de síntesis no está disponible temporalmente.';
}

export function SynthesisView(props: SynthesisViewProps) {
  const { anchorKey, anchorType, category, synthesisType, titles, domains, onRequest } = props;
  const [state, setState] = useState<SynthesisState>({ status: 'idle' });
  const [copied, setCopied] = useState(false);

  const handleGenerate = useCallback(async () => {
    setState({ status: 'loading' });
    try {
      await invoke('generate_synthesis', {
        category,
        titles,
        domains,
        synthesisType,
        anchorKey,
        anchorType,
      });
    } catch (e) {
      setState({ status: 'error', message: mapError(String(e)) });
    }
  }, [anchorKey, anchorType, category, synthesisType, titles, domains]);

  // Auto-generar en Autonomous (sin onRequest)
  useEffect(() => {
    if (onRequest === undefined) {
      handleGenerate();
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    let unlistenChunk: (() => void) | undefined;
    let unlistenComplete: (() => void) | undefined;
    let unlistenError: (() => void) | undefined;
    let contentAccum = '';

    (async () => {
      unlistenChunk = await listen<{ anchor_key: string; chunk: string }>(
        'synthesis_chunk',
        (event) => {
          if (event.payload.anchor_key !== anchorKey) return;
          contentAccum += event.payload.chunk;
          setState({ status: 'streaming', content: contentAccum });
        }
      );
      unlistenComplete = await listen<{ anchor_key: string }>(
        'synthesis_complete',
        (event) => {
          if (event.payload.anchor_key !== anchorKey) return;
          setState({ status: 'complete', content: contentAccum });
        }
      );
      unlistenError = await listen<{ anchor_key: string; error: string }>(
        'synthesis_error',
        (event) => {
          if (event.payload.anchor_key !== anchorKey) return;
          setState({ status: 'error', message: mapError(event.payload.error) });
        }
      );
    })();

    return () => {
      unlistenChunk?.();
      unlistenComplete?.();
      unlistenError?.();
    };
  }, [anchorKey]);

  async function copyToClipboard(content: string) {
    await navigator.clipboard.writeText(content);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  }

  function exportMarkdown(content: string) {
    const blob = new Blob([content], { type: 'text/markdown' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `synthesis-${new Date().toISOString().slice(0, 10)}.md`;
    a.click();
    URL.revokeObjectURL(url);
  }

  if (state.status === 'idle') {
    if (onRequest === undefined) return null;
    return (
      <div className="synthesis-view synthesis-view--idle">
        <button className="synthesis-view__generate" onClick={async () => {
          await onRequest();
          handleGenerate();
        }}>
          Generar síntesis
        </button>
      </div>
    );
  }

  if (state.status === 'loading') {
    return (
      <div className="synthesis-view synthesis-view--loading">
        <p>Generando síntesis…</p>
      </div>
    );
  }

  if (state.status === 'streaming') {
    return (
      <div className="synthesis-view synthesis-view--streaming">
        <div
          className="synthesis-view__content"
          dangerouslySetInnerHTML={{ __html: renderMarkdown(state.content) }}
        />
      </div>
    );
  }

  if (state.status === 'complete') {
    return (
      <div className="synthesis-view synthesis-view--complete">
        <div
          className="synthesis-view__content"
          dangerouslySetInnerHTML={{ __html: renderMarkdown(state.content) }}
        />
        <div className="synthesis-view__actions">
          <button
            className="synthesis-view__copy"
            onClick={() => copyToClipboard(state.content)}
          >
            {copied ? '¡Copiado!' : 'Copiar'}
          </button>
          <button
            className="synthesis-view__export"
            onClick={() => exportMarkdown(state.content)}
          >
            Exportar Markdown
          </button>
          {onRequest !== undefined && (
            <button className="synthesis-view__regenerate" onClick={handleGenerate}>
              Regenerar
            </button>
          )}
        </div>
      </div>
    );
  }

  // error
  return (
    <div className="synthesis-view synthesis-view--error">
      <p className="synthesis-view__error">{state.message}</p>
      {onRequest !== undefined && (
        <button className="synthesis-view__retry" onClick={handleGenerate}>
          Reintentar
        </button>
      )}
    </div>
  );
}
