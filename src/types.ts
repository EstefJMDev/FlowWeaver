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
