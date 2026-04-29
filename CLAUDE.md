# FlowWeaver — Contexto para Claude Code

Este archivo se carga automáticamente. Claude Code es el ejecutor de implementación de FlowWeaver. La gobernanza del proyecto (backlogs, decisiones, gates de fase) vive en el repositorio separado **EquipoEnjambre** (`../EquipoEnjambre`).

**Regla de entrada:** solo implementa tareas con Task Spec (TS) aprobada en EquipoEnjambre. Si no hay TS, no hay implementación.

---

## Stack técnico

- **Backend:** Rust 1.95 / Tauri 2 — `src-tauri/src/`
- **Frontend:** React 18 + TypeScript 5.6 / Vite 6 — `src/`
- **Base de datos:** SQLCipher (AES-256-GCM) en desktop; SQLite bundled en Android
- **Cifrado de campo:** AES-GCM 0.10 (`crypto.rs`)
- **Plataformas:** Windows + Android (primario); iOS track paralelo secundario

**Comandos de verificación:**
```bash
cd src-tauri && cargo test          # suite determinística (14 tests base)
npx tsc --noEmit                    # TypeScript limpio
```

Ambos deben pasar sin regresiones antes de cerrar cualquier tarea.

---

## Módulos existentes

### Rust (`src-tauri/src/`)
| Archivo | Función |
|---|---|
| `commands.rs` | Comandos Tauri expuestos al frontend |
| `storage.rs` | SQLCipher — tabla `resources` |
| `crypto.rs` | AES-256-GCM — cifrado de campo |
| `classifier.rs` | Clasificación de recursos por category/domain |
| `grouper.rs` | Agrupación de recursos — fuente de Panel A |
| `episode_detector.rs` | Detección de episodios de sesión activa (R12) |
| `importer.rs` | Importación de bookmarks |
| `session_builder.rs` | Construcción de sesiones de workspace |

### Frontend (`src/`)
| Archivo / Componente | Función |
|---|---|
| `App.tsx` | Componente principal |
| `types.ts` | Tipos TypeScript compartidos |
| `templates.ts` | Plantillas de resumen (baseline Fase 1) |
| `components/PanelA.tsx` | Panel de recursos agrupados |
| `components/PanelB.tsx` | Panel de resumen (Fase 1 — stateless) |
| `components/PanelC.tsx` | Panel de siguientes pasos |
| `components/EpisodePanel.tsx` | Vista de episodio activo |
| `components/AnticipatedWorkspace.tsx` | Workspace anticipado |
| `components/PrivacyDashboard.tsx` | Dashboard de privacidad (base 0b — se expande en T-2-004) |

---

## Fase activa: Fase 2

Backlog aprobado: `../EquipoEnjambre/operations/backlogs/backlog-phase-2.md`

### Cadena de tareas (orden de dependencia estricto)

```
T-2-000  Delimitación FS Watcher       ✅ APROBADO (documental — TS-2-000)
T-2-001  Pattern Detector              → pattern_detector.rs  (puede comenzar)
    └── T-2-002  Trust Scorer          → trust_scorer.rs      (depende de T-2-001)
        └── T-2-003  State Machine     → state_machine.rs     (depende de T-2-002)
T-2-004  Privacy Dashboard completo    → PrivacyDashboard.tsx (depende de T-2-001 + T-2-003)
```

**T-2-000 está aprobado.** La implementación de `fs_watcher.rs` puede comenzar (TS-2-000 firmado por Technical Architect en AR-2-002).

**T-2-001 puede comenzar** en paralelo a T-2-000 (backlog lo autoriza explícitamente).

Antes de implementar T-2-002, T-2-003 o T-2-004, verifica que la tarea predecesora tiene handoff al Technical Architect en `../EquipoEnjambre/operations/handoffs/`.

---

## Contratos de implementación por tarea

### T-2-001 — Pattern Detector (`pattern_detector.rs`)

Módulo nuevo, independiente de `episode_detector.rs` (R12 — nunca usar como base).

**Tipos de salida:**
```rust
struct DetectedPattern {
    pattern_id: Uuid,
    label: String,                        // dominant_category + time_bucket
    category_signature: Vec<CategoryWeight>,
    domain_signature: Vec<DomainWeight>,
    temporal_window: TemporalWindow,      // time_of_day_bucket + day_of_week_mask
    frequency: usize,
    first_seen: i64,
    last_seen: i64,
}
```

**Reglas:**
- Solo lee `domain`, `category`, `captured_at` de SQLCipher — nunca `url` ni `title` (D1)
- Umbral de frecuencia mínima: parámetro configurable, no constante fija
- Baseline determinístico sin LLM (D8)
- Comentario de cabecera obligatorio declarando distinción vs `episode_detector.rs` (R12)

**Test mínimo:** dado un conjunto sintético de N recursos con patrones conocidos, `detect_patterns()` devuelve los patrones esperados.

### T-2-002 — Trust Scorer (`trust_scorer.rs`)

**Input:** `Vec<DetectedPattern>` — no lee SQLCipher directamente.

**Tipos de salida:**
```rust
struct TrustScore {
    pattern_id: Uuid,
    trust_score: f64,       // [0.0, 1.0]
    stability_score: f64,   // [0.0, 1.0] — entropía normalizada (D5)
    recency_weight: f64,
    confidence_tier: ConfidenceTier,  // Low / Medium / High
}
```

**Reglas:**
- `trust_score = f(frequency, recency_weight, temporal_coherence)` — determinístico (D8)
- `stability_score`: slot concentration con entropía normalizada, acotado estrictamente en [0.0, 1.0] (D5)
- Umbrales de `confidence_tier`: parámetros configurables
- **No exponer `recommend_action()` ni similar** — las acciones son responsabilidad exclusiva de la State Machine (D4)
- Comentario de cabecera: "Trust Scorer produce inputs para la State Machine. No toma decisiones de acción (D4)."

### T-2-003 — State Machine (`state_machine.rs`)

**Estados (enum):** `Observing → Learning → Trusted → Autonomous`

**Transiciones:**
- `Observing → Learning`: `pattern_count >= MIN_PATTERNS && trust_score > THRESHOLD_LOW`
- `Learning → Trusted`: `trust_score > THRESHOLD_HIGH && !user_blocked`
- `Trusted → Autonomous`: **solo por acción explícita del usuario** (nunca automática)
- `Cualquier → Observing`: acción de reset del usuario

**Tipos de salida:**
```rust
struct TrustState {
    current_state: TrustStateEnum,
    available_transitions: Vec<Transition>,
    active_patterns_count: usize,
    last_transition_at: i64,
}
```

**Reglas:**
- Umbrales `MIN_PATTERNS`, `THRESHOLD_LOW`, `THRESHOLD_HIGH`: configurables
- Comandos Tauri: `get_trust_state`, `reset_trust_state`
- Estado persiste en SQLCipher (solo el estado enum, no los scores)
- La State Machine tiene autoridad; Trust Scorer no llama a las transiciones (D4)

### T-2-004 — Privacy Dashboard completo (`PrivacyDashboard.tsx`)

Expansión del componente existente. Tres secciones:

1. **Recursos** (ya existe en 0b): `resource_count`, `categories`, `domains` — sin cambios
2. **Patrones detectados** (nueva): `label`, `category_signature`, `domain_signature`, `frequency`, `last_seen` + botones Bloquear / Desbloquear
3. **Estado de confianza** (nueva): `current_state`, tiempo en estado, `active_patterns_count` + botón "Resetear confianza" (siempre visible) + botón "Activar preparación automática" (solo en estado Trusted, con confirmación explícita)
4. **FS Watcher** (si implementado): directorios activos, estado, contador de eventos, botón "Dejar de observar"

**Nuevos tipos en `src/types.ts`:** `PatternSummary`, `TrustStateView`

**Comandos Tauri consumidos:** `get_detected_patterns`, `block_pattern`, `unblock_pattern`, `get_trust_state`, `reset_trust_state`

**Regla absoluta:** ningún campo, tooltip ni texto del dashboard puede exponer `url` ni `title` (D1 — sin excepciones).

---

## Restricciones no negociables (D1–R12)

Estas decisiones están cerradas en EquipoEnjambre. No se modifican sin change control formal.

| ID | Restricción | Impacto directo en código |
|---|---|---|
| **D1** | Solo `domain` y `category` en claro. `url` y `title` siempre cifrados. | Ninguna query, campo de struct, campo de UI ni log puede contener `url` o `title` en claro |
| **D4** | State Machine tiene autoridad. `trust_score` es input, no decide acciones. | `trust_scorer.rs` no puede exponer métodos de acción. Las transiciones las ejecuta `state_machine.rs`. |
| **D5** | `stability_score` = slot concentration con entropía normalizada (0–1) | Fórmula fija. No inventar alternativas sin CR. Rango [0.0, 1.0] estricto. |
| **D8** | Baseline determinístico sin LLM obligatorio | Cada módulo nuevo debe funcionar sin modelo local. LLM es mejora opcional que debe declararse explícitamente. |
| **D9** | FS Watcher observa solo mientras la app está en primer plano | No hay modo background. TS-2-000 define qué directorios y extensiones son válidos. |
| **D14** | Privacy Dashboard completo es prerequisito bloqueante de Fase 3 | T-2-004 no puede quedar incompleto al cerrar Fase 2. |
| **D17** | Pattern Detector completo en Fase 2 | No dividir entre fases. O está completo con sus ACs o no se cierra la tarea. |
| **D19** | Android + Windows primario | Primero compilar y validar en Windows. Android NDK 27.3 disponible. iOS es track paralelo secundario. |
| **R12** | `pattern_detector.rs` ≠ `episode_detector.rs` | Módulos distintos, propósitos distintos. Episode Detector: sesión activa, sin estado. Pattern Detector: historial longitudinal, persiste patrones. No heredar código entre ellos. Declarar la distinción en comentario de cabecera de cada módulo nuevo de Fase 2. |

---

## FS Watcher — scope aprobado (TS-2-000)

Directorios observables: `~/Downloads`, `~/Desktop` (ninguno activo por defecto).
Extensiones permitidas: `.pdf .docx .doc .txt .md .xlsx .csv .png .jpg .jpeg .gif .webp .svg .mp4 .mov .webm .zip`
Regla de lista blanca: cualquier extensión no listada se ignora silenciosamente.
Observación: solo mientras la app está en primer plano. No existe modo background.
Módulo: `src-tauri/src/fs_watcher.rs` (independiente de `pattern_detector.rs`).
Comentario de cabecera obligatorio:
```rust
// FS Watcher: detecta eventos de archivo en sesión activa.
// Distinto de pattern_detector.rs (patrones longitudinales) — R12.
// Opera solo mientras la app está en primer plano (D9).
```

---

## Qué no implementar sin TS aprobada

- Cualquier módulo de Fase 2 no listado arriba
- Sync Layer / iCloud Drive (Fase 0b — pendiente de entorno macOS)
- Share Extension iOS (track paralelo — pendiente de macOS)
- LLM local como requisito (D8 — solo como mejora opcional declarada)
- Panel D ni nuevos paneles en el shell
- Background monitoring de ningún tipo
- Telemetría ni métricas de usuarios externos (Fase 3)
- Calibración de umbrales con datos reales de usuarios (Fase 3)

### Bloqueado adicionalmente por OD-007 (2026-04-29) — D22 aplazada

- Pattern Detector compilado para Android (más allá de la base técnica ya
  existente que da soporte a la galería)
- Episode Detector móvil específico (umbrales `GAP_SECS = 2_700`,
  `MAX_WINDOW_SECS = 7_200`, `JACCARD_THRESHOLD = 0.20` mobile-only)
- Observer semi-pasivo Android: Tile de sesión / Quick Settings tile
- Foreground service mobile para captura semi-pasiva
- Detección de intención sobre comportamiento de navegación móvil
- Anticipación proactiva mobile (notificaciones contextuales)
- Resumen / agrupación de episodios de búsqueda en móvil
- Cualquier feature etiquetada como "tier paid mobile"
- Workspace anticipado en móvil (Panel B, Episode Detector visible en mobile UI)

### Qué sí sigue válido en mobile (preservado por OD-007)

- Galería Android organizada por categoría (D20)
- Classifier + Grouper compilados para Android (infraestructura, no producto B)
- SQLCipher local Android con AES-256-GCM (D1)
- Sync bidireccional Drive relay (D21)
- Privacy Dashboard mínimo mobile
- Captura explícita vía Share Intent (D9 redacción original)

---

## Referencia a EquipoEnjambre

| Documento | Ruta |
|---|---|
| Backlog Fase 2 | `../EquipoEnjambre/operations/backlogs/backlog-phase-2.md` |
| TS-2-000 (FS Watcher aprobado) | `../EquipoEnjambre/operations/task-specs/TS-2-000-fs-watcher-delimitation.md` |
| Decisiones cerradas | `../EquipoEnjambre/project-docs/decisions-log.md` |
| Definición de fases | `../EquipoEnjambre/project-docs/phase-definition.md` |
| Handoffs | `../EquipoEnjambre/operations/handoffs/` |
