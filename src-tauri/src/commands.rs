use crate::organizer::{
    EngineError, OrganizationPlan, ScanResult, generate_plan, scan_folder as engine_scan_folder,
};

#[tauri::command]
pub fn scan_folder(root_path: String) -> Result<ScanResult, EngineError> {
    engine_scan_folder(&root_path)
}

#[tauri::command]
pub fn generate_organization_plan(root_path: String) -> Result<OrganizationPlan, EngineError> {
    generate_plan(&root_path)
}
