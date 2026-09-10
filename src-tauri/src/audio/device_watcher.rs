use std::collections::HashSet;
use tauri::{AppHandle, Emitter};
use tracing::info;

use super::device::list_input_devices;

#[derive(Clone, serde::Serialize)]
pub struct DevicesChangedPayload {
    pub added: Vec<String>,
    pub removed: Vec<String>,
}

/// Spawns background task that polls input devices every 2s.
/// Emits `devices-changed` Tauri event when list changes.
pub fn watch(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut known: HashSet<String> = list_input_devices()
            .map(|ds| ds.into_iter().map(|d| d.id).collect())
            .unwrap_or_default();

        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

            let current: HashSet<String> = list_input_devices()
                .map(|ds| ds.into_iter().map(|d| d.id).collect())
                .unwrap_or_default();

            if current != known {
                let added: Vec<String> = current.difference(&known).cloned().collect();
                let removed: Vec<String> = known.difference(&current).cloned().collect();

                if !added.is_empty() {
                    info!("Audio devices added: {:?}", added);
                }
                if !removed.is_empty() {
                    info!("Audio devices removed: {:?}", removed);
                }

                let _ = app.emit("devices-changed", DevicesChangedPayload {
                    added,
                    removed,
                });

                known = current;
            }
        }
    });
}
