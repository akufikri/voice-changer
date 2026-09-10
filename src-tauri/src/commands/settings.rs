use serde::{Deserialize, Serialize};
use std::fs;
use tauri::State;

use crate::AppState;

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct AppSettings {
    pub selected_device_id: Option<String>,
    pub selected_voice_id: Option<String>,
}

fn settings_path() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("voice-changer")
        .join("settings.json")
}

#[tauri::command]
pub async fn load_settings(_state: State<'_, AppState>) -> Result<AppSettings, String> {
    let path = settings_path();
    if !path.exists() {
        return Ok(AppSettings::default());
    }
    let data = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&data).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_settings(
    settings: AppSettings,
    _state: State<'_, AppState>,
) -> Result<(), String> {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let data = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    fs::write(&path, data).map_err(|e| e.to_string())
}
