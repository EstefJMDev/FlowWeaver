# FlowWeaver — Contexto para Claude Code

Cargado automáticamente. Claude Code es el ejecutor de implementación.
La gobernanza (backlogs, decisiones, gates) vive en `../EquipoEnjambre`.

**Regla de entrada:** solo implementa tareas con Task Spec (TS) aprobada en EquipoEnjambre.

---

## Stack

- **Backend:** Rust 1.95 / Tauri 2 — `src-tauri/src/`
- **Frontend:** React 18 + TypeScript 5.6 / Vite 6 — `src/`
- **DB:** SQLCipher (AES-256-GCM) desktop; SQLite Android
- **Cifrado campo:** `crypto.rs` — AES-GCM. Siempre usar `decrypt_any` para leer (detecta XOR legacy o AES automáticamente)
- **Plataformas:** Windows + Android primario

**Verificaciones obligatorias antes de cerrar cualquier tarea:**
```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib   # 105 tests, 0 fail
npx tsc --noEmit                                          # EXIT=0
```

**Arrancar en dev:**
```bash
npm run tauri dev    # desde C:\Users\pinnovacion\Desktop\FlowWeaver
```

---

## Estado actual — Fase 3 (en curso)

### Fases completadas
- **Fase 0a:** Panel A + C, bookmark importer, classifier, grouper, SQLCipher
- **Fase 0b:** Session Builder, Episode Detector dual-mode, Privacy Dashboard mínimo, add_capture (Share Intent simulado)
- **Fase 0c:** Relay Drive, galería Android, Privacy Dashboard expandido
- **Fase 1:** Panel B con plantillas. Gate pasado (PIR-003)
- **Fase 2:** Pattern Detector, Trust Scorer, State Machine (4 estados), Privacy Dashboard completo, FS Watcher

### Fase 3 — tareas implementadas
| Tarea | Estado | Módulos |
|---|---|---|
| T-3-008 | ✅ | install token cifrado en SQLCipher (`synthesis_tokens.rs`, `commands.rs`) |
| T-3-009 | ✅ | synthesis engine — proxy SSE, cifrado persistido (`synthesis_engine.rs`, `syntheses_store.rs`, `consent_log_store.rs` stub) |
| T-3-011 | ✅ | SynthesisSection en Privacy Dashboard (`SynthesisSection.tsx`) |
| T-3-012 | ✅ | Modal consentimiento informado (`SynthesisConsentModal.tsx`, `consent_log_store.rs` completo) |
| T-3-010 | ✅ | SynthesisView streaming Markdown (`SynthesisView.tsx`, integración `AnticipatedWorkspace.tsx`) |

---

## Módulos Rust (`src-tauri/src/`)

| Archivo | Función |
|---|---|
| `commands.rs` | Comandos Tauri. Sección Phase 3 al final. |
| `storage.rs` | SQLCipher — tabla `resources` |
| `crypto.rs` | AES-256-GCM. **Siempre usar `decrypt_any`** para leer títulos/urls (detecta XOR legacy) |
| `classifier.rs` | Clasificación domain → category |
| `grouper.rs` | Agrupación recursos para Panel A |
| `session_builder.rs` | Sesiones temporales. Usa `decrypt_any` para títulos (fix crítico aplicado) |
| `episode_detector.rs` | Episodios de sesión activa (R12 — distinto de pattern_detector) |
| `importer.rs` | Importación bookmarks |
| `pattern_detector.rs` | Patrones longitudinales (R12 — distinto de episode_detector) |
| `trust_scorer.rs` | Scores de confianza — input para State Machine (D4, D5) |
| `state_machine.rs` | Autoridad de transiciones: Observing→Learning→Trusted→Autonomous |
| `pattern_blocks.rs` | Patrones bloqueados por usuario |
| `fs_watcher.rs` | FS Watcher — solo mientras app en primer plano (D9) |
| `synthesis_engine.rs` | SSE streaming al proxy, payload PG-001 |
| `syntheses_store.rs` | Persistencia síntesis cifradas |
| `synthesis_tokens.rs` | Install token cifrado |
| `consent_log_store.rs` | Tabla `consent_log` — `ensure_schema`, `has_consent`, `record_consent`, `revoke_consent` |
| `drive_relay.rs` | Relay Google Drive (desktop only) |

---

## Módulos Frontend (`src/`)

| Archivo | Función |
|---|---|
| `App.tsx` | Raíz. Sin StrictMode (incompatible con listeners async Tauri). |
| `main.tsx` | Entry point. Sin `React.StrictMode`. |
| `types.ts` | Tipos TypeScript compartidos (Episode, TrustStateView, etc.) |
| `vite-env.d.ts` | Referencia `vite/client` para `import.meta.env` |
| `templates.ts` | Plantillas de resumen por categoría |
| `utils/renderMarkdown.ts` | Renderer Markdown inline compartido (h2–h4, negrita) |
| `components/PanelA.tsx` | Recursos agrupados |
| `components/PanelB.tsx` | Resumen por plantillas (Fase 1, stateless) |
| `components/PanelC.tsx` | Siguientes pasos |
| `components/EpisodePanel.tsx` | Lista episodios. Botón síntesis en Broad 100% + categoría válida. |
| `components/AnticipatedWorkspace.tsx` | Episodio más reciente (Precise preferred, Broad fallback) + SynthesisView |
| `components/PrivacyDashboard.tsx` | Dashboard privacidad completo |
| `components/SynthesisSection.tsx` | Sección síntesis en Privacy Dashboard |
| `components/SynthesisView.tsx` | Streaming SSE, estados idle/loading/streaming/complete/error |
| `components/SynthesisConsentModal.tsx` | Modal consentimiento PG-003 (textos EXACTOS, no parafrasear) |
| `components/TrustStateSection.tsx` | Estado SM en Privacy Dashboard |
| `components/PatternsSection.tsx` | Patrones detectados en Privacy Dashboard |

---

## Arquitectura de síntesis (flujo completo)

```
Usuario pulsa "Generar síntesis"
  ↓
onRequest() en AnticipatedWorkspace
  ↓
invoke('check_synthesis_consent') → sin consent → SynthesisConsentModal
  ↓ (con consent)
handleGenerate() en SynthesisView
  ↓
invoke('generate_synthesis', { category, titles, domains, synthesisType, anchorKey, anchorType })
  ↓
Rust: verifica SM ≥ Trusted (D4) + has_consent (D25) + token
  ↓
fetch_from_proxy → SSE stream → emit('synthesis_chunk', { anchor_key, chunk })
  ↓ (al completar)
emit('synthesis_complete') → emit('synthesis_error') si falla
  ↓
SynthesisView escucha eventos, acumula en contentAccum, renderiza Markdown
```

**Proxy URL:** `https://flowweaver-proxy.bananasplitsound.workers.dev/synthesize`

**Tipos válidos para el proxy:** `cocina | entretenimiento | noticias | tecnologia`

**Mapeo de categorías** (en `AnticipatedWorkspace.tsx` y `EpisodePanel.tsx`):
```typescript
cocina/recetas/gastronomia → 'cocina'
entretenimiento/cine/musica/juegos → 'entretenimiento'
noticias/actualidad → 'noticias'
tecnologia/programacion/desarrollo → 'tecnologia'
fallback → 'noticias'
```

---

## Bugs críticos resueltos — no reintroducir

| Bug | Causa | Fix aplicado |
|---|---|---|
| Títulos vacíos en episodios | `session_builder.rs` usaba `crypto::decrypt` (XOR) para títulos cifrados con AES | Cambiado a `crypto::decrypt_any` — siempre usar esto para leer campos cifrados |
| Texto síntesis duplicado | React StrictMode ejecuta useEffect 2x; `listen()` async crea race condition | Eliminado StrictMode de `main.tsx`. **No restaurar.** |
| Episodio activo incorrecto | `window_end` no existe en Episode (existe en Session) | `latestCapture()` usando `Math.max(...resources.map(r => r.captured_at))` |
| Solo mostraba episodios Precise | Broad episodios ignorados aunque fueran más recientes | Lógica: Precise preferred, Broad fallback por recencia |
| Generar síntesis no hacía nada | `handleGenerateRequest` no disparaba la generación si consent OK | `onRequest: () => Promise<void>` + `await onRequest(); handleGenerate()` |

---

## Restricciones no negociables (D1–D25, R12)

| ID | Restricción | Impacto en código |
|---|---|---|
| **D1** | `url` y `title` siempre cifrados, nunca al frontend | Sin excepciones en queries, structs, UI, logs |
| **D4** | State Machine tiene autoridad; trust_score es input | `trust_scorer.rs` no expone acciones. Solo `state_machine.rs` transiciona. |
| **D5** | stability_score = entropía normalizada [0, 1] | Fórmula fija. Rango estricto. |
| **D8** | Baseline determinístico sin LLM | Cada módulo funciona sin modelo local. LLM es mejora opcional. |
| **D9** | FS Watcher solo mientras app en primer plano | No hay modo background. |
| **D14** | Privacy Dashboard completo antes de beta | ✅ Implementado |
| **D17** | Pattern Detector completo en Fase 2 | ✅ Implementado |
| **D19** | Android + Windows primario | iOS track paralelo secundario |
| **D25** | synthesis_engine verifica `has_consent` antes de llamar al proxy | La verificación en `commands.rs::generate_synthesis` no puede eliminarse |
| **R12** | `pattern_detector.rs` ≠ `episode_detector.rs` | Módulos distintos. No heredar código. Declarar distinción en cabecera. |

---

## Notas de implementación críticas

**React StrictMode:** eliminado permanentemente. Los listeners async de Tauri (`listen()` devuelve Promise) crean race conditions con el doble-montaje de StrictMode que no tienen solución limpia. StrictMode no existe en producción de todas formas.

**decrypt_any vs decrypt:** `crypto::decrypt` solo entiende XOR (legacy). `crypto::decrypt_any` detecta automáticamente XOR o AES por prefijo. **Siempre usar `decrypt_any`** para leer `url` y `title` de la BD — `add_capture` y nuevas capturas usan AES.

**Listeners Tauri en useEffect:** patrón correcto:
```typescript
useEffect(() => {
  let contentAccum = '';
  let unlisten: (() => void) | undefined;
  (async () => {
    unlisten = await listen('event_name', handler);
  })();
  return () => { unlisten?.(); };
}, [dep]);
```

**onRequest en SynthesisView:** tipo `() => Promise<void>`. El botón idle hace `await onRequest(); handleGenerate()`. Si `onRequest` lanza (no consent), `handleGenerate` no se ejecuta.

---

## Referencia a EquipoEnjambre

| Documento | Ruta |
|---|---|
| Backlog Fase 3 | `../EquipoEnjambre/operations/backlogs/backlog-phase-3.md` |
| Task Specs Fase 3 | `../EquipoEnjambre/operations/task-specs/` |
| Decisiones cerradas | `../EquipoEnjambre/project-docs/decisions-log.md` |
| Handoffs | `../EquipoEnjambre/operations/handoffs/` |
