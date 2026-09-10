mod audio;
mod commands;
mod db;
mod provider;
mod tests;

use std::sync::Arc;
use tokio::sync::Mutex;

use audio::engine::AudioEngine;
use provider::types::VoiceProvider;

pub struct AppState {
    pub engine: Mutex<AudioEngine>,
    pub provider: Mutex<Option<Arc<dyn VoiceProvider>>>,
}

impl AppState {
    fn new() -> Self {
        let (engine, _rx) = AudioEngine::new();
        Self {
            engine: Mutex::new(engine),
            provider: Mutex::new(None),
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "voice_changer_lib=info".parse().unwrap()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::new())
        .setup(|app| {
            audio::device_watcher::watch(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::audio::get_audio_devices,
            commands::audio::start_voice_changer,
            commands::audio::stop_voice_changer,
            commands::audio::set_processing_enabled,
            commands::audio::get_engine_status,
            commands::audio::set_monitor_enabled,
            commands::provider::get_voices,
            commands::provider::set_elevenlabs_key,
            commands::provider::get_saved_api_key,
            commands::settings::load_settings,
            commands::settings::save_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
