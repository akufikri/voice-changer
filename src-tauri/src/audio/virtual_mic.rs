use anyhow::{anyhow, Result};
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    StreamConfig,
};
use ringbuf::traits::Consumer;
use std::sync::{
    mpsc::{self, Sender},
    Arc, Mutex,
};
use tracing::{info, warn};

use super::ring_buffer::AudioRingBuffer;

/// Known virtual audio devices, checked in priority order.
const VIRTUAL_DEVICE_NAMES: &[&str] = &[
    "BlackHole 2ch",
    "BlackHole 16ch",
    "BlackHole",
    "Loopback Audio",
    "Loopback",
    "Voice.ai Virtual Microphone",
    "JACK",
    "SoundFlower (2ch)",
    "SoundFlower (64ch)",
];

pub struct VirtualMic {
    _stop: Sender<()>,
    pub device_name: String,
    pub sample_rate: u32,
    pub channels: u16,
}

impl VirtualMic {
    pub fn start(out_buf: Arc<Mutex<AudioRingBuffer>>) -> Result<Self> {
        Self::start_with_device(out_buf, None)
    }

    pub fn start_with_device(
        out_buf: Arc<Mutex<AudioRingBuffer>>,
        preferred: Option<&str>,
    ) -> Result<Self> {
        let host = cpal::default_host();

        let device = if let Some(name) = preferred {
            // Exact match first, then fuzzy
            host.output_devices()?.find(|d| {
                d.name().map(|n| n == name).unwrap_or(false)
            })
            .or_else(|| {
                host.output_devices().ok()?.find(|d| {
                    d.name().map(|n| n.contains(name)).unwrap_or(false)
                })
            })
        } else {
            // Auto-detect first known virtual device
            let mut found = None;
            'outer: for &candidate in VIRTUAL_DEVICE_NAMES {
                for d in host.output_devices()?.collect::<Vec<_>>() {
                    if d.name().map(|n| n.contains(candidate)).unwrap_or(false) {
                        found = Some(d);
                        break 'outer;
                    }
                }
            }
            found
        };

        let device = device.ok_or_else(|| {
            let installed: Vec<String> = host
                .output_devices()
                .map(|it| it.filter_map(|d| d.name().ok()).collect())
                .unwrap_or_default();
            anyhow!(
                "No virtual audio device found.\n\
                 Install one of:\n\
                 • BlackHole 2ch: brew install blackhole-2ch\n\
                 • Loopback: https://rogueamoeba.com/loopback/\n\n\
                 Available output devices: {}",
                installed.join(", ")
            )
        })?;

        let device_name = device.name().unwrap_or_else(|_| "BlackHole".into());
        let config = device.default_output_config()?;
        let sample_rate = config.sample_rate().0;
        let channels = config.channels();
        let stream_config: StreamConfig = config.into();

        info!(
            "Virtual mic → {} @ {}Hz {}ch",
            device_name, sample_rate, channels
        );

        let (stop_tx, stop_rx) = mpsc::channel::<()>();

        std::thread::spawn(move || {
            let buf = out_buf;
            let stream = device
                .build_output_stream(
                    &stream_config,
                    move |output: &mut [f32], _| {
                        if let Ok(mut rb) = buf.lock() {
                            let n = rb.consumer.pop_slice(output);
                            output[n..].fill(0.0);
                        }
                    },
                    |err| warn!("Virtual mic output error: {}", err),
                    None,
                )
                .expect("build_output_stream for BlackHole failed");

            stream.play().expect("BlackHole stream play failed");
            let _ = stop_rx.recv();
        });

        Ok(Self {
            _stop: stop_tx,
            device_name,
            sample_rate,
            channels,
        })
    }
}
