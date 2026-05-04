# FlowWeaver — Product Specification

La especificación completa y canónica del producto se mantiene en el repositorio de orquestación:

**Fuente canónica:** [EquipoEnjambre/docs/product-spec.md](https://github.com/EstefJMDev/EquipoEnjambre)

Consulta ese archivo para la definición de fases, decisiones cerradas, restricciones del MVP, stack técnico y caso de uso núcleo.

Este archivo se mantiene como referencia para evitar duplicación y divergencia entre repos.

## Resumen rápido

- **Producto:** App de escritorio (Tauri 2 + React/TS + Rust) con companion Android
- **Plataforma primaria:** Android + Windows (decisión D19)
- **Caso de uso núcleo:** Detectar intención de trabajo desde señales móviles y preparar el workspace en desktop antes de que el usuario lo pida
- **Fase activa:** Fase 2 (Pattern Detector, Trust Scorer, State Machine, Privacy Dashboard completo)
- **Privacidad:** Procesamiento 100% local. Sin backend propia. Sin telemetría.
