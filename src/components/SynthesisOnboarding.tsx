import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

type OnboardingState = 'idle' | 'saving' | 'error';

interface Props {
  onComplete: () => void;
  onSkip: () => void;
}

export function SynthesisOnboarding({ onComplete, onSkip }: Props) {
  const [token, setToken] = useState('');
  const [status, setStatus] = useState<OnboardingState>('idle');
  const [errorMsg, setErrorMsg] = useState('');

  async function handleActivate() {
    const trimmed = token.trim();
    if (!trimmed) return;
    setStatus('saving');
    setErrorMsg('');
    try {
      await invoke('set_synthesis_token', { token: trimmed });
      onComplete();
    } catch (e) {
      setStatus('error');
      setErrorMsg(typeof e === 'string' ? e : 'Error al activar el token.');
    }
  }

  return (
    <div className="synthesis-onboarding">
      <h3 className="synthesis-onboarding__title">Activar síntesis con IA</h3>
      <p className="synthesis-onboarding__description">
        Introduce tu token de acceso para habilitar los resúmenes inteligentes.
      </p>

      <div className="synthesis-onboarding__field">
        <input
          type="password"
          className="synthesis-onboarding__input"
          placeholder="Introduce tu token de acceso beta"
          value={token}
          onChange={e => setToken(e.target.value)}
          disabled={status === 'saving'}
        />
        <p className="synthesis-onboarding__hint">
          Tu token te fue enviado por el equipo FlowWeaver.
        </p>
      </div>

      {status === 'error' && (
        <p className="synthesis-onboarding__error">{errorMsg}</p>
      )}

      <div className="synthesis-onboarding__actions">
        <button
          className="synthesis-onboarding__btn synthesis-onboarding__btn--primary"
          onClick={handleActivate}
          disabled={token.trim() === '' || status === 'saving'}
        >
          {status === 'saving' ? 'Activando…' : 'Activar síntesis'}
        </button>

        <button
          className="synthesis-onboarding__btn synthesis-onboarding__btn--secondary"
          onClick={onSkip}
        >
          Continuar sin síntesis
        </button>
      </div>
    </div>
  );
}
