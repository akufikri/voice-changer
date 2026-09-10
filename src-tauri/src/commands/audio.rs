use serde::Serialize;
use std::sync::Arc;
use tauri::State;

use crate::{audio::device::list_input_devices, db, provider::elevenlabs::ElevenLabsProvider, AppState};

#[derive(Serialize)]
pub struct AudioDeviceDto {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

#[derive(Serialize)]
pub struct EngineStatusDto {
    pub state: String,
    pub virtual_mic_name: Option<String>,
    pub monitor_active: bool,
}

#[tauri::command]
pub async fn get_audio_devices(_state: State<'_, AppState>) -> Result<Vec<AudioDeviceDto>, String> {
    list_input_devices()
        .map(|devs| {
            devs.into_iter()
                .map(|d| AudioDeviceDto {
                    id: d.id,
                    name: d.name,
                    is_default: d.is_default,
                })
                .collect()
        })
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn start_voice_changer(
    device_id: Option<String>,
    voice_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    // If user selected a voice, rebuild provider with that voice before starting
    if let Some(vid) = voice_id {
        let api_key = db::get("elevenlabs_api_key")
            .map_err(|e| e.to_string())?
            .ok_or("No API key stored. Re-enter credentials.")?;
        db::set("elevenlabs_voice_id", &vid).map_err(|e| e.to_string())?;
        let provider = Arc::new(ElevenLabsProvider::new(api_key, vid));
        *state.provider.lock().await = Some(provider);
    }

    let mut engine = state.engine.lock().await;
    let provider = state.provider.lock().await.clone();
    let provider = provider.ok_or("No provider configured. Set API key first.")?;
    engine.start(device_id, provider).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn stop_voice_changer(state: State<'_, AppState>) -> Result<(), String> {
    let mut engine = state.engine.lock().await;
    engine.stop();
    Ok(())
}

#[tauri::command]
pub async fn set_processing_enabled(
    enabled: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let engine = state.engine.lock().await;
    engine.set_processing(enabled);
    Ok(())
}

#[tauri::command]
pub async fn get_engine_status(state: State<'_, AppState>) -> Result<EngineStatusDto, String> {
    let engine = state.engine.lock().await;
    Ok(EngineStatusDto {
        state: format!("{:?}", engine.current_state()),
        virtual_mic_name: engine.virtual_mic_name().map(str::to_string),
        monitor_active: engine.monitor_active(),
    })
}

#[tauri::command]
pub async fn set_monitor_enabled(
    enabled: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut engine = state.engine.lock().await;
    engine.set_monitor(enabled).map_err(|e| e.to_string())
}
