import { invoke } from "@tauri-apps/api/core";
import type { OrganizationPlan, ScanResult } from "../types/organizer";

export function scanFolder(rootPath: string): Promise<ScanResult> {
  return invoke<ScanResult>("scan_folder", { rootPath });
}

export function generateOrganizationPlan(rootPath: string): Promise<OrganizationPlan> {
  return invoke<OrganizationPlan>("generate_organization_plan", { rootPath });
}

export function describeEngineError(error: unknown): string {
  if (typeof error === "object" && error !== null && "message" in error) {
    const message = error.message;
    if (typeof message === "string") return message;
  }
  return typeof error === "string" ? error : "Could not scan this folder. Please try again.";
}
