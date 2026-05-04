# FlowWeaver

App de escritorio que detecta intención de trabajo a partir de recursos guardados desde el móvil y prepara el workspace del ordenador automáticamente antes de que el usuario lo pida.

## Stack

- **Backend:** Rust (`src-tauri/src/`) — SQLCipher, classifier, grouper, session builder, episode detector
- **Frontend:** React + TypeScript (`src/`) — Panel A, Panel B, Panel C, Privacy Dashboard
- **Framework:** Tauri 2
- **Plataforma primaria:** Android + Windows

## Desarrollo

```bash
npm install
npm run tauri dev
```

### Tests

```bash
# Rust
cd src-tauri && cargo test

# TypeScript
npx tsc --noEmit
```

## Gobernanza

El backlog, las decisiones de arquitectura, los task specs y toda la gobernanza del proyecto se gestionan en el repo de orquestación:

**[EquipoEnjambre](https://github.com/EstefJMDev/EquipoEnjambre)**

## Estado actual

**Fase 2 activa** — Pattern Detector, Trust Scorer, State Machine, Privacy Dashboard completo.

## Privacidad

- Procesamiento 100% local
- Títulos y URLs cifrados localmente (SQLCipher)
- Sin backend propia
- Sin telemetría
