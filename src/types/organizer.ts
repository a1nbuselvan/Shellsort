export type Category =
  | "documents"
  | "images"
  | "videos"
  | "audio"
  | "archives"
  | "code"
  | "spreadsheets"
  | "presentations"
  | "other";

export type ConflictStatus =
  | "none"
  | "existing_destination"
  | "proposed_collision"
  | "existing_and_proposed_collision"
  | "already_correct"
  | "unverifiable_destination";

export interface ScanWarning {
  path: string | null;
  kind: string;
  message: string;
}

export interface ScanIssue {
  path: string | null;
  kind: string;
  message: string;
}

export interface ScanResult {
  root_path: string;
  files: Array<{
    source: string;
    file_name: string;
    extension: string | null;
    category: Category;
    size_bytes: number;
    modified_unix_seconds: number | null;
    hidden: boolean;
  }>;
  warnings: ScanWarning[];
  issues: ScanIssue[];
  is_incomplete: boolean;
}

export interface PlanEntry {
  source: string;
  destination: string;
  file_name: string;
  extension: string | null;
  category: Category;
  size_bytes: number;
  conflict_status: ConflictStatus;
  reason: string | null;
}

export interface OrganizationPlan {
  root_path: string;
  entries: PlanEntry[];
  warnings: ScanWarning[];
  issues: ScanIssue[];
  is_incomplete: boolean;
}

export interface EngineError {
  kind: string;
  message: string;
}
