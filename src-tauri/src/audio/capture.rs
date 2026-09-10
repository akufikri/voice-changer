use anyhow::Result;
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    SampleRate, StreamConfig,
};
use ringbuf::traits::Producer;
use std::sync::{
    mpsc::{self, Sender},
    Arc, Mutex,
};
use tracing::{error, info};

use super::ring_buffer::AudioRingBuffer;

pub struct AudioCapture {
    _stop: Sender<()>,
    pub sample_rate: u32,
    pub channels: u16,
    /// Device label (kept for diagnostics / future echo-gate risk checks).
    #[allow(dead_code)]
    pub device_name: String,
}

impl AudioCapture {
    pub fn start(device_name: Option<&str>, ring_buf: Arc<Mutex<AudioRingBuffer>>) -> Result<Self> {
        let host = cpal::default_host();

        let device = if let Some(name) = device_name {
            host.input_devices()?
                .find(|d| d.name().map(|n| n == name).unwrap_or(false))
                .ok_or_else(|| anyhow::anyhow!("Device not found: {}", name))?
        } else {
            // Avoid capturing from virtual output devices (BlackHole, Loopback) as default.
            // If system default is one of these, AI output would feed back into the pipeline.
            let default = host.default_input_device()
                .ok_or_else(|| anyhow::anyhow!("No default input device"))?;
            let default_name = default.name().unwrap_or_default().to_lowercase();
            let is_virtual = |n: &str| {
                let nl = n.to_lowercase();
                nl.contains("blackhole") || nl.contains("loopback")
                    || nl.contains("voice.ai") || nl.contains("nomachine")
                    || nl.contains("microsoft teams audio")
            };
            if is_virtual(&default_name) {
                // Default is virtual output device — fall back to first real physical mic
                host.input_devices()?
                    .find(|d| d.name().map(|n| !is_virtual(&n)).unwrap_or(false))
                    .unwrap_or(default)
            } else {
                default
            }
        };

        let default_config = device.default_input_config()?;
        let channels = default_config.channels();

        // Prefer 48kHz to match BlackHole. CoreAudio does hw resampling transparently.
        let target = SampleRate(48_000);
        let sample_rate = device
            .supported_input_configs()
            .ok()
            .and_then(|mut it| {
                it.find(|c| c.min_sample_rate() <= target && c.max_sample_rate() >= target)
                    .map(|_| target)
            })
            .unwrap_or(default_config.sample_rate());

        let config = StreamConfig {
            channels,
            sample_rate,
            buffer_size: cpal::BufferSize::Default,
        };

        let device_name = device.name().unwrap_or_else(|_| "unknown".into());

        info!(
            "Capture: {:?} @ {}Hz {}ch",
            device_name,
            sample_rate.0,
            channels
        );

        let (stop_tx, stop_rx) = mpsc::channel::<()>();

        std::thread::spawn(move || {
            let buf = ring_buf;
            let stream = device
                .build_input_stream(
                    &config,
                    move |data: &[f32], _| {
                        if let Ok(mut rb) = buf.lock() {
                            rb.producer.push_slice(data);
                        }
                    },
                    move |err| error!("Capture error: {}", err),
                    None,
                )
                .expect("build_input_stream failed");

            stream.play().expect("stream.play failed");
            let _ = stop_rx.recv();
        });

        Ok(Self {
            _stop: stop_tx,
            sample_rate: sample_rate.0,
            channels,
            device_name,
        })
    }
}
