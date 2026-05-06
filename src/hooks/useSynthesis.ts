import { useState, useEffect, useCallback, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export type SynthesisStatus =
  | { status: 'idle' }
  | { status: 'loading' }
  | { status: 'streaming'; content: string }
  | { status: 'complete'; content: string }
  | { status: 'error'; message: string };

export interface SynthesisPayload {
  category: string;
  titles: string[];
  domains: string[];
  synthesisType: string;
  anchorType: 'pattern' | 'session';
}

export function mapError(backendError: string): string {
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

// Centraliza listeners synthesis_chunk / synthesis_complete / synthesis_error con cleanup,
// generationId y contentAccumRef para evitar concatenación de contenido entre regeneraciones.
// Dos instancias en pantalla coexisten limpiamente — cada una filtra por su anchorKey.
export function useSynthesis(anchorKey: string) {
  const [state, setState] = useState<SynthesisStatus>({ status: 'idle' });
  const generationIdRef = useRef(0);
  const contentAccumRef = useRef('');

  useEffect(() => {
    let unlistenChunk: (() => void) | undefined;
    let unlistenComplete: (() => void) | undefined;
    let unlistenError: (() => void) | undefined;

    (async () => {
      unlistenChunk = await listen<{ anchor_key: string; chunk: string }>(
        'synthesis_chunk',
        (event) => {
          if (event.payload.anchor_key !== anchorKey) return;
          contentAccumRef.current += event.payload.chunk;
          setState({ status: 'streaming', content: contentAccumRef.current });
        }
      );
      unlistenComplete = await listen<{ anchor_key: string }>(
        'synthesis_complete',
        (event) => {
          if (event.payload.anchor_key !== anchorKey) return;
          setState({ status: 'complete', content: contentAccumRef.current });
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

  const generate = useCallback(async (payload: SynthesisPayload) => {
    generationIdRef.current += 1;
    contentAccumRef.current = '';
    setState({ status: 'loading' });
    try {
      await invoke('generate_synthesis', {
        category: payload.category,
        titles: payload.titles,
        domains: payload.domains,
        synthesisType: payload.synthesisType,
        anchorKey,
        anchorType: payload.anchorType,
      });
    } catch (e) {
      setState({ status: 'error', message: mapError(String(e)) });
    }
  }, [anchorKey]);

  const reset = useCallback(() => {
    generationIdRef.current += 1;
    contentAccumRef.current = '';
    setState({ status: 'idle' });
  }, []);

  return { state, generate, reset };
}
