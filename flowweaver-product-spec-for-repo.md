# FlowWeaver — Product Specification for Repository Use

> Fuente: adaptación estructurada de la especificación técnica v4.2 para uso directo dentro del repositorio del proyecto.
> Uso recomendado: guardar este archivo como `docs/product-spec.md` o `governance/product-spec.md` en el repo del producto.

## 1. Propósito del producto

FlowWeaver es un asistente digital que detecta intención de trabajo a partir de señales reales del usuario y prepara el siguiente paso antes de que el usuario tenga que pedirlo.

### Definición del producto en una frase

Cuando el usuario guarda 3 o más recursos sobre un tema similar en menos de 24 horas desde el móvil, FlowWeaver detecta la intención, agrupa el contenido y presenta en el escritorio un workspace preparado antes de que el usuario lo pida.

## 2. Tesis de producto

La mayoría de herramientas de automatización exigen demasiada configuración manual.

FlowWeaver parte de una tesis distinta:

- las personas no necesitan más herramientas para diseñar flujos manualmente
- necesitan un sistema que detecte transiciones repetidas de intención
- ese sistema debe preparar el siguiente paso con la mínima fricción posible

## 3. Diferenciadores no negociables

### 3.1 Detecta y anticipa, sin reglas manuales
- MVP: Episode Detector para reacción inmediata
- Fase 2: Pattern Detector para hábitos longitudinales

### 3.2 Privacidad verificable por diseño
- procesamiento local
- títulos y metadatos cifrados localmente
- nunca se almacena contenido completo de páginas
- sincronización cifrada extremo a extremo

### 3.3 Puente de intención entre dispositivos
- no sincroniza solo archivos
- sincroniza la señal de intención para continuar el trabajo en otro dispositivo

### 3.4 Confianza progresiva
- el sistema empieza detectando episodios puntuales
- después sugiere
- después prepara en background
- solo tras demostrar valor repetidamente actúa de forma autónoma

## 4. Caso de uso núcleo del MVP

### Caso de uso único del MVP
- el usuario guarda 3 o más recursos sobre un tema similar desde el móvil en menos de 24 horas
- FlowWeaver detecta el episodio
- sincroniza la señal al escritorio
- prepara un workspace
- el usuario abre el desktop y encuentra el trabajo ya preparado

### Regla crítica
Este es el único caso de uso núcleo del MVP.

No deben tratarse como caso núcleo:
- bookmarks retroactivos
- organización de descargas
- timeline privada
- contexto para reuniones
- marketplace, SDK o equipo

## 5. Restricciones no negociables del MVP

- MVP = puente móvil → desktop
- desktop no observa activamente en MVP
- no hay FS Watcher en MVP
- no hay Accessibility APIs en MVP
- único observer activo del MVP = Share Extension iOS
- no hay backend propia en MVP
- sync MVP = iCloud Drive / Google Drive relay cifrado + fallback QR
- Pattern Detector, Trust Scorer, State Machine y Explainability Log entran en Fase 2
- LLM local es mejora opcional, no requisito funcional
- bookmarks retroactivos son onboarding y cold start, no caso de uso núcleo

## 6. Stack del MVP

### Observación móvil
- Swift iOS
- Share Extension

### Desktop
- Tauri 2
- backend en Rust
- frontend React/TypeScript

### Motor de episodios
- reglas determinísticas
- heurística de similitud

### Sincronización
- iCloud Drive o Google Drive como relay cifrado
- fallback QR

### Almacenamiento local
- SQLCipher sobre SQLite

### Resumen del workspace
- plantillas de alta calidad como baseline
- LLM local opcional si el hardware lo soporta

## 7. Arquitectura funcional

### Módulos del MVP
1. Observer Agent
2. Session Builder
3. Episode Detector
4. Sync Layer
5. Action Executor / Workspace Builder

### Módulos que NO entran en 0a/0b
- Pattern Detector
- Trust & Risk Scorer
- State Machine completa
- Explainability Log

## 8. Flujo del caso dorado

1. el usuario guarda un enlace desde la Share Extension
2. guarda segundo y tercer enlace dentro de 24h
3. Session Builder detecta sesión
4. Episode Detector evalúa el episodio
5. Sync Layer envía IntentSignal cifrada
6. el desktop la recibe y prepara el workspace
7. el usuario abre el escritorio y encuentra el workspace listo
8. el sistema registra interacción para fases posteriores

## 9. Episode Detector

### Función
Es el motor del valor inmediato del MVP.

### Importante
No es aprendizaje longitudinal.
Es detección de episodios puntuales con reglas determinísticas más heurística de similitud.

### Modos
#### Precise mode
- categoría dominante suficiente
- similitud fina entre títulos
- produce el mayor wow moment

#### Broad mode
- misma categoría
- menor precisión
- sigue siendo útil, pero menos impactante

### Umbrales base
- `CATEGORY_RATIO = 0.6`
- `JACCARD_MIN = 0.3`
- `MIN_CLUSTER_SIZE = 3`

### Regla crítica
Si se detecta un episodio, el workspace se prepara siempre.
Lo que cambia entre precise y broad es el copy y la prominencia, no el hecho de preparar el workspace.

## 10. Workspace

### Panel A — Recursos
- lista de recursos agrupados
- título real
- favicon
- dominio
- agrupación por subtema

### Panel B — Resumen
- 3 a 5 bullets del tema
- baseline por plantilla
- LLM opcional solo como mejora en background

### Panel C — Siguientes pasos
- checklist de 3 a 5 acciones
- generado por plantillas según el tipo de contenido

### Regla del resumen
- primero plantilla siempre
- si el LLM local es suficientemente rápido y útil, mejora el resultado
- si no, se mantiene la plantilla

## 11. Privacidad por diseño

### Nivel de privacidad por defecto
Nivel 1.

### Se almacena localmente y cifrado
- URLs guardadas
- títulos
- meta descriptions
- og tags
- hashes de deduplicación

### No se almacena
- contenido completo de páginas
- texto escrito por el usuario

### Datos públicos que pueden mantenerse en claro
- dominio

### Retención orientativa
- URLs, títulos y metadatos: 90 días
- logs de acciones de fases futuras: 30 días

## 12. Privacy Dashboard

### Fase 0b mínimo
Debe mostrar como mínimo:
- número de sesiones
- número de signals sincronizadas
- botón borrar todo
- botón pausar

### Fase 2 completo
Debe añadir:
- timeline de sesiones
- patrones
- motivos legibles
- borrado granular
- exportación JSON

## 13. Sync Layer

### Decisión cerrada
MVP usa iCloud Drive o Google Drive como relay cifrado.
No hay P2P en v0.1.

### Requisitos del protocolo
- payload cifrado E2E
- ACK
- idempotencia
- reintentos
- deduplicación por `signal_id`

### Fallback
Si iCloud no es suficientemente fiable, usar sync manual vía QR.

### Regla crítica
Mejor un flujo manual fiable que una sync automática rota.

## 14. Fases del proyecto

### Fase 0a
Objetivo:
validar que el formato workspace genera valor.

Incluye:
- app desktop standalone
- lectura local de bookmarks Safari/Chrome
- agrupación por dominio/categoría/similitud básica
- Panel A + Panel C
- almacenamiento local cifrado

No incluye:
- móvil
- sync
- Share Extension
- Episode Detector real del puente
- Pattern Detector
- Trust Scorer
- LLM local
- dashboard completo

Qué valida:
- utilidad del formato workspace
- comprensión de la agrupación visual
- claridad del contenedor de trabajo

Qué NO valida:
- PMF
- hipótesis núcleo del puente móvil→desktop
- wow moment real

### Fase 0b
Objetivo:
validar la hipótesis núcleo del puente móvil→desktop.

Incluye:
- Share Extension iOS
- Session Builder
- Episode Detector dual-mode
- sync con ACK e idempotencia
- fallback QR
- Privacy Dashboard mínimo
- testing end-to-end del momento mágico

No incluye:
- FS Watcher
- Pattern Detector
- Trust Scorer
- Explainability Log
- backend propia
- LLM local obligatorio

Qué valida:
- que el usuario sienta el “ya me lo había preparado”
- que la sync llegue a tiempo
- que el puente móvil→desktop funcione como experiencia

### Fase 1
- organización de descargas y screenshots
- FS Watcher
- adaptación del Episode Detector
- Panel B con plantillas

### Fase 2
- Pattern Detector
- Trust Scorer
- State Machine
- Privacy Dashboard completo
- aprendizaje longitudinal

### Fase 3
- beta pública
- métricas
- calibración de umbrales
- LLM local opcional donde aporte valor

## 15. Decisiones cerradas

### D1
Privacidad = Nivel 1 por defecto. Narrativa verificable, no radical.

### D2
Episode Detector dual-mode en MVP. Pattern Detector completo solo en Fase 2.

### D3
Precisión del Episode Detector = precise + broad fallback.

### D4
La máquina de estados manda. El trust score es input, no autoridad final.

### D5
Estabilidad basada en slot concentration score.

### D6
Sync MVP = iCloud o Google Drive con ACK, idempotencia y reintentos.

### D7
LAN como canal adicional en V1. P2P solo en V2+ con nuevo emparejamiento.

### D8
Resumen = plantillas primero, LLM local como upgrade opcional.

### D9
Observer del MVP = Share Extension iOS. Desktop no observa.

### D10
Fase 0 se divide en 0a y 0b.

### D11
Plataformas iniciales = macOS + iOS.

### D12
Foco MVP = único caso móvil → desktop. Bookmarks no son caso núcleo.

### D13
Narrativa del producto = detecta y anticipa, sin reglas manuales.

### D14
Privacy Dashboard progresivo: mínimo en 0b, completo en 2.

### D15
Monetización no se optimiza antes de validar product-market fit.

### D16
Esquema BD: INTEGER PRIMARY KEY + UUID indexado.

### D17
Pattern Detector completo solo en Fase 2.

### D18
Fase 0b incluye buffer de sync y escape a QR si iCloud falla.

## 16. Riesgos principales

- falsos positivos del Episode Detector
- detector demasiado conservador
- edge cases de iCloud sync
- latencia de sync
- percepción de vigilancia
- cold start sin datos
- race conditions en sync
- scope creep del MVP
- tensión entre promesa de privacidad y percepción del usuario

## 17. Métricas clave

### Técnicas
- precisión Episode Detector > 60%
- ratio precise/broad > 60%
- ACK en < 60s para > 95% de señales
- plantillas del resumen < 100 ms

### Valor
- > 40% usuarios con al menos 1 workspace activo a 14 días
- confianza subjetiva > 7/10
- > 3 momentos de magia por semana activa

## 18. Onboarding y cold start

### Onboarding inicial
1. selección de intereses
2. importación local de bookmarks
3. guardado guiado de 3 links

### Regla crítica
Los bookmarks sirven para onboarding y cold start.
No validan el caso núcleo del producto.

## 19. Qué debe proteger cualquier agente o sistema de trabajo

Cualquier agente, prompt, plan o implementación debe proteger estas invariantes:

- no confundir 0a con validación de PMF
- no tratar bookmarks como caso núcleo
- no introducir observación activa del desktop en MVP
- no introducir backend propia en MVP
- no adelantar Pattern Detector, Trust o Explainability antes de Fase 2
- no convertir el LLM local en dependencia funcional del sistema
- no redefinir el producto como agregador genérico de recursos
- no sacrificar privacidad verificable por velocidad de implementación

## 20. Uso recomendado de este archivo

Este archivo debe servir como referencia operativa del repo producto.

Se recomienda:
- usarlo como `docs/product-spec.md`
- citarlo en prompts de arranque del repo producto
- usarlo para validar que la implementación respeta el scope de la fase activa
- usarlo como base de auditoría para evitar contaminación entre fases
