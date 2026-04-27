export interface ClusterResource {
  uuid: string;
  title: string;
  domain: string;
  category: string;
}

export interface Cluster {
  group_key: string;
  domain: string;
  category: string;
  sub_label: string;
  resources: ClusterResource[];
}

export interface ImportResult {
  imported: number;
  skipped: number;
  errors: string[];
  sources: string[];
}

// ── Phase 0b types ────────────────────────────────────────────────────────────

export interface SessionResource {
  uuid: string;
  title: string;
  domain: string;
  category: string;
  captured_at: number;
}

export type DetectionMode = "Precise" | "Broad";

export interface Episode {
  episode_id: string;
  label: string;
  resources: SessionResource[];
  mode: DetectionMode;
  coherence: number;
}

export interface Session {
  session_id: string;
  window_start: number;
  window_end: number;
  is_bootstrap: boolean;
  resources: SessionResource[];
}

// ── Phase 0b — Privacy Dashboard (D14) ───────────────────────────────────────

export interface CategoryCount {
  category: string;
  count: number;
}

export interface DomainCount {
  domain: string;
  count: number;
}

export interface PrivacyStats {
  resource_count: number;
  categories: CategoryCount[];
  domains: DomainCount[];
}

// ── Phase 0c — Android gallery ────────────────────────────────────────────────

export interface MobileResource {
  uuid: string;
  domain: string;
  category: string;
  title: string;
  captured_at: number;
}

export interface CategoryGroup {
  category: string;
  resources: MobileResource[];
}

// ── Phase 2 — Trust State (T-2-003) ──────────────────────────────────────────

export type TrustStateEnum = 'Observing' | 'Learning' | 'Trusted' | 'Autonomous';

export interface Transition {
  from: TrustStateEnum;
  to: TrustStateEnum;
  requires_user_action: boolean;
}

export interface TrustStateView {
  current_state: TrustStateEnum;
  available_transitions: Transition[];
  active_patterns_count: number;
  last_transition_at: number;
}

// ── Phase 2 — Privacy Dashboard (T-2-004) ────────────────────────────────────

export interface CategorySignatureItem {
  category: string;
  weight: number;
}

export interface DomainSignatureItem {
  domain: string;
  weight: number;
}

export type TimeBucket = 'Morning' | 'Afternoon' | 'Evening';

export interface TemporalWindowView {
  time_bucket: TimeBucket;
  day_of_week_mask: number;
}

export interface PatternSummary {
  pattern_id: string;
  label: string;
  category_signature: CategorySignatureItem[];
  domain_signature: DomainSignatureItem[];
  temporal_window: TemporalWindowView;
  frequency: number;
  last_seen: number;
  is_blocked: boolean;
}

// ── Phase 2 — FS Watcher (T-2-000) ───────────────────────────────────────────

export type CandidateDirectory = 'Downloads' | 'Desktop';

export type FsWatcherRuntimeState = 'Active' | 'Suspended' | 'Unsupported';

export interface FsWatcherDirectory {
  directory: CandidateDirectory;
  absolute_path: string;
  active: boolean;
  activated_at: number | null;
}

// `file_name_encrypted` (Vec<u8> en Rust) NO se expone aquí. El frontend
// solo necesita metadatos (directorio, extensión, timestamp). Si HO-FW-PD
// requiere mostrar el nombre desencriptado, se añadirá un comando dedicado
// `fs_watcher_decrypt_event_name(event_id)` con auditoría explícita —
// fuera de scope de HO-017.
export interface FsWatcherEvent {
  event_id: string;
  directory: CandidateDirectory;
  extension: string;
  detected_at: number;
}

export interface FsWatcherStatus {
  runtime_state: FsWatcherRuntimeState;
  directories: FsWatcherDirectory[];
  events_in_current_session: number;
  events_last_24h: number;
}
