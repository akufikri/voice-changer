use serde::Serialize;
use std::sync::Arc;
use tauri::State;

use crate::{
    db,
    provider::{elevenlabs::ElevenLabsProvider, types::Voice},
    AppState,
};

#[derive(Serialize)]
pub struct VoiceDto {
    pub id: String,
    pub name: String,
    pub language: Option<String>,
    pub provider: String,
}

impl From<Voice> for VoiceDto {
    fn from(v: Voice) -> Self {
        Self {
            id: v.id,
            name: v.name,
            language: v.language,
            provider: v.provider,
        }
    }
}

#[tauri::command]
pub async fn get_voices(state: State<'_, AppState>) -> Result<Vec<VoiceDto>, String> {
    let provider = state.provider.lock().await.clone();
    let provider = provider.ok_or("No provider configured.")?;
    provider
        .list_voices()
        .await
        .map(|vs| vs.into_iter().map(VoiceDto::from).collect())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_elevenlabs_key(
    api_key: String,
    voice_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    db::set("elevenlabs_api_key", &api_key).map_err(|e| e.to_string())?;
    db::set("elevenlabs_voice_id", &voice_id).map_err(|e| e.to_string())?;

    let provider = Arc::new(ElevenLabsProvider::new(api_key, voice_id));
    *state.provider.lock().await = Some(provider);
    Ok(())
}

/// Returns true if credentials are stored. Also initializes the provider so
/// the engine is ready immediately after app restart without re-entering the key.
#[tauri::command]
pub async fn get_saved_api_key(state: State<'_, AppState>) -> Result<bool, String> {
    let api_key = db::get("elevenlabs_api_key").map_err(|e| e.to_string())?;
    let voice_id = db::get("elevenlabs_voice_id").map_err(|e| e.to_string())?;

    match (api_key, voice_id) {
        (Some(key), Some(vid)) => {
            let provider = Arc::new(ElevenLabsProvider::new(key, vid));
            *state.provider.lock().await = Some(provider);
            Ok(true)
        }
        _ => Ok(false),
    }
}
