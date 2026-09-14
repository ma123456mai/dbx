use std::sync::Arc;

use tauri::{AppHandle, Manager};

use super::connection::AppState;
pub use dbx_core::cc_switch::{CcSwitchImportResult, CcSwitchPluginStatus};

const CC_SWITCH_IMPORT_METHOD: &str = "import_ai_configs";

#[tauri::command]
pub async fn load_cc_switch_ai_configs(
    state: tauri::State<'_, Arc<AppState>>,
    app: AppHandle,
) -> Result<CcSwitchImportResult, String> {
    if state.plugins.find_capability(dbx_core::cc_switch::CC_SWITCH_PLUGIN_CAPABILITY)?.is_none() {
        return Err("ccSwitchPluginNotInstalled".to_string());
    }
    let home_dir = app
        .path()
        .home_dir()
        .map_err(|error| format!("ccSwitchHomeDirectoryFailed:{error}"))?;
    let database_path = home_dir.join(".cc-switch").join("cc-switch.db");
    state
        .plugins
        .invoke_capability(
            dbx_core::cc_switch::CC_SWITCH_PLUGIN_CAPABILITY,
            CC_SWITCH_IMPORT_METHOD,
            serde_json::json!({ "databasePath": database_path }),
        )
        .await
}

#[tauri::command]
pub async fn cc_switch_plugin_status(state: tauri::State<'_, Arc<AppState>>) -> Result<CcSwitchPluginStatus, String> {
    let root_dir = state.plugins.root_dir().to_path_buf();
    tauri::async_runtime::spawn_blocking(move || dbx_core::cc_switch::cc_switch_plugin_status(&root_dir))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn install_cc_switch_plugin(state: tauri::State<'_, Arc<AppState>>) -> Result<CcSwitchPluginStatus, String> {
    let root_dir = state.plugins.root_dir().to_path_buf();
    dbx_core::cc_switch::install_cc_switch_plugin(&root_dir).await
}

#[tauri::command]
pub async fn install_cc_switch_plugin_local(
    state: tauri::State<'_, Arc<AppState>>,
    path: String,
) -> Result<CcSwitchPluginStatus, String> {
    let root_dir = state.plugins.root_dir().to_path_buf();
    dbx_core::cc_switch::install_cc_switch_plugin_from_file(&root_dir, &path).await
}

#[tauri::command]
pub async fn uninstall_cc_switch_plugin(state: tauri::State<'_, Arc<AppState>>) -> Result<CcSwitchPluginStatus, String> {
    let root_dir = state.plugins.root_dir().to_path_buf();
    tauri::async_runtime::spawn_blocking(move || dbx_core::cc_switch::uninstall_cc_switch_plugin(&root_dir))
        .await
        .map_err(|error| error.to_string())?
}
