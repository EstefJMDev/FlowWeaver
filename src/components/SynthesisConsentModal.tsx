import { useState } from 'react';

interface SynthesisConsentModalProps {
  onAccept: () => void;
  onDecline: () => void;
}

// record_synthesis_consent se llama en SynthesisOnboarding.handleActivate,
// no aquí. El modal solo confirma que el usuario leyó el aviso de privacidad.
export function SynthesisConsentModal(props: SynthesisConsentModalProps) {
  const [pending, setPending] = useState(false);

  function handleAccept() {
    setPending(true);
    props.onAccept();
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
            {pending ? 'Continuando…' : 'Continuar'}
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
