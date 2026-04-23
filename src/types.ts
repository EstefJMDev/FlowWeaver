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
