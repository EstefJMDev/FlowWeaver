import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

interface SynthesisConsentModalProps {
  onAccept: () => void;
  onDecline: () => void;
}

export function SynthesisConsentModal(props: SynthesisConsentModalProps) {
  const [pending, setPending] = useState(false);

  async function handleAccept() {
    setPending(true);
    try {
      await invoke('record_synthesis_consent');
      props.onAccept();
    } catch {
      // error de red/BD: no cerrar el modal, permitir reintento
    } finally {
      setPending(false);
    }
  }

  return (
    <div role="dialog" aria-modal="true" aria-labelledby="consent-title"
         className="consent-modal">
      <div className="consent-modal__backdrop" onClick={props.onDecline} />
      <div className="consent-modal__content">
        <h2 id="consent-title">Antes de activar la síntesis inteligente</h2>

        <p>
          La síntesis envía al proxy FlowWeaver: los títulos de tus páginas guardadas,
          la categoría y los dominios. La URL completa y el contenido de las páginas
          nunca se transmiten.
        </p>
        <p>
          El proxy no almacena tu contenido. La síntesis generada se guarda solo en
          tu dispositivo.
        </p>
        <p>
          Puedes desactivar la síntesis en cualquier momento desde el Privacy Dashboard.
        </p>

        <div className="consent-modal__actions">
          <button
            onClick={handleAccept}
            disabled={pending}
            className="consent-modal__accept"
          >
            {pending ? 'Activando…' : 'Activar síntesis'}
          </button>
          <button
            onClick={props.onDecline}
            className="consent-modal__decline"
          >
            No activar
          </button>
        </div>
      </div>
    </div>
  );
}
