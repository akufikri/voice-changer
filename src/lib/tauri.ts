import { invoke } from "@tauri-apps/api/core";

export interface AudioDevice {
  id: string;
  name: string;
  is_default: boolean;
}

export interface Voice {
  id: string;
  name: string;
  language: string | null;
  provider: string;
}

export interface AppSettings {
  selected_device_id: string | null;
  selected_voice_id: string | null;
}

export interface EngineStatus {
  state: string;
  virtual_mic_name: string | null;
  monitor_active: boolean;
}

export const api = {
  getAudioDevices: () => invoke<AudioDevice[]>("get_audio_devices"),
  startVoiceChanger: (deviceId?: string, voiceId?: string) =>
    invoke<void>("start_voice_changer", { deviceId, voiceId }),
  stopVoiceChanger: () => invoke<void>("stop_voice_changer"),
  setProcessingEnabled: (enabled: boolean) =>
    invoke<void>("set_processing_enabled", { enabled }),
  getEngineStatus: () => invoke<EngineStatus>("get_engine_status"),
  setMonitorEnabled: (enabled: boolean) =>
    invoke<void>("set_monitor_enabled", { enabled }),
  getVoices: () => invoke<Voice[]>("get_voices"),
  setElevenlabsKey: (apiKey: string, voiceId: string) =>
    invoke<void>("set_elevenlabs_key", { apiKey, voiceId }),
  getSavedApiKey: () => invoke<boolean>("get_saved_api_key"),
  loadSettings: () => invoke<AppSettings>("load_settings"),
  saveSettings: (settings: AppSettings) =>
    invoke<void>("save_settings", { settings }),
};
