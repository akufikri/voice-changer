use anyhow::Result;
use cpal::traits::{DeviceTrait, HostTrait};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

pub fn list_input_devices() -> Result<Vec<AudioDevice>> {
    let host = cpal::default_host();
    let default = host.default_input_device();
    let default_name = default.as_ref().and_then(|d| d.name().ok());

    let devices = host
        .input_devices()?
        .filter_map(|d| {
            let name = d.name().ok()?;
            // Exclude virtual output devices — selecting these as input causes feedback loops.
            let name_lower = name.to_lowercase();
            if name_lower.contains("blackhole")
                || name_lower.contains("loopback")
                || name_lower.contains("voice.ai")
                || name_lower.contains("microsoft teams audio")
                || name_lower.contains("nomachine")
            {
                return None;
            }
            let is_default = default_name.as_deref() == Some(&name);
            Some(AudioDevice {
                id: name.clone(),
                name,
                is_default,
            })
        })
        .collect();

    Ok(devices)
}
