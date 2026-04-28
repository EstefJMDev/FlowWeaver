import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { CandidateDirectory, FsWatcherStatus } from '../types';

export function FsWatcherSection() {
  const [status, setStatus] = useState<FsWatcherStatus | null>(null);

  useEffect(() => {
    let alive = true;
    const load = () => {
      invoke<FsWatcherStatus>('fs_watcher_get_status')
        .then(s => { if (alive) setStatus(s); })
        .catch(() => {}); // Unsupported en Android → silencioso (D19)
    };
    load();
    const id = setInterval(load, 4000);
    return () => { alive = false; clearInterval(id); };
  }, []);

  // D19: oculto en Android (runtime_state === 'Unsupported')
  if (!status || status.runtime_state === 'Unsupported') return null;

  async function handleActivate(dir: CandidateDirectory) {
    // TS-2-000 §3 "Confirmación explícita" — texto literal del spec.
    const ok = confirm(
      `FlowWeaver observará ${dir} para detectar archivos mientras tengas la app abierta. ` +
      `Solo detecta el nombre y tipo de archivo — nunca el contenido.`
    );
    if (!ok) return;
    await invoke('fs_watcher_activate_directory', { directory: dir, confirmed: true })
      .catch(() => {});
    invoke<FsWatcherStatus>('fs_watcher_get_status').then(setStatus).catch(() => {});
  }

  async function handleDeactivate(dir: CandidateDirectory) {
    await invoke('fs_watcher_deactivate_directory', { directory: dir }).catch(() => {});
    invoke<FsWatcherStatus>('fs_watcher_get_status').then(setStatus).catch(() => {});
  }

  async function handleClearHistory(dir: CandidateDirectory) {
    await invoke('fs_watcher_clear_directory_history', { directory: dir }).catch(() => {});
    invoke<FsWatcherStatus>('fs_watcher_get_status').then(setStatus).catch(() => {});
  }

  return (
    <section
      aria-labelledby="pd-fs-watcher"
      className="privacy-dashboard__section"
    >
      <h4 id="pd-fs-watcher" className="privacy-dashboard__section-title">
        Observación de archivos locales
      </h4>

      {/* Texto explicativo literal — TS-2-000 §3 */}
      <p className="privacy-dashboard__fs-description">
        FlowWeaver detecta el nombre y tipo de archivo mientras tienes la app
        abierta. Nunca lee el contenido de tus archivos.
      </p>

      {/* Estado en tiempo real */}
      <p className="privacy-dashboard__fs-state">
        Estado:{' '}
        <span className={`privacy-dashboard__fs-badge privacy-dashboard__fs-badge--${status.runtime_state.toLowerCase()}`}>
          {status.runtime_state === 'Active' ? 'Activo' : 'Suspendido'}
        </span>
      </p>

      {/* Contadores — visibles solo cuando hay sesión activa */}
      {status.runtime_state === 'Active' && (
        <p className="privacy-dashboard__fs-counters">
          <span>{status.events_in_current_session} archivos en esta sesión</span>
          {' · '}
          <span>{status.events_last_24h} en las últimas 24 h</span>
        </p>
      )}

      {/* Lista de directorios */}
      <ul className="privacy-dashboard__fs-dirs">
        {status.directories.map(dir => (
          <li key={dir.directory} className="privacy-dashboard__fs-dir">
            <span className="privacy-dashboard__fs-dir-name">
              {dir.directory}
            </span>
            <span className="privacy-dashboard__fs-dir-status">
              {dir.active ? 'Activo' : 'Inactivo'}
            </span>
            <span className="privacy-dashboard__fs-dir-actions">
              {dir.active ? (
                <>
                  <button
                    className="privacy-dashboard__fs-btn"
                    onClick={() => handleDeactivate(dir.directory)}
                  >
                    Dejar de observar
                  </button>
                  <button
                    className="privacy-dashboard__fs-btn privacy-dashboard__fs-btn--danger"
                    onClick={() => handleClearHistory(dir.directory)}
                  >
                    Eliminar historial
                  </button>
                </>
              ) : (
                <button
                  className="privacy-dashboard__fs-btn"
                  onClick={() => handleActivate(dir.directory)}
                >
                  Activar
                </button>
              )}
            </span>
          </li>
        ))}
      </ul>
    </section>
  );
}
