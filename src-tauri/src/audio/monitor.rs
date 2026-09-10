use anyhow::Result;
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

/// Plays the monitor_buf through the default speaker so user hears converted voice.
/// Engine writes to monitor_buf in parallel with BlackHole out_buf.
pub struct OutputMonitor {
    _stop: Sender<()>,
}

impl OutputMonitor {
    pub fn start(monitor_buf: Arc<Mutex<AudioRingBuffer>>) -> Result<Self> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| anyhow::anyhow!("No default output device"))?;

        let config = device.default_output_config()?;
        let stream_config: StreamConfig = config.into();
        info!("Output monitor → {}", device.name().unwrap_or_default());

        let (stop_tx, stop_rx) = mpsc::channel::<()>();

        std::thread::spawn(move || {
            let buf = monitor_buf.clone();
            let stream = device
                .build_output_stream(
                    &stream_config,
                    move |output: &mut [f32], _| {
                        if let Ok(mut rb) = buf.lock() {
                            let n = rb.consumer.pop_slice(output);
                            output[n..].fill(0.0);
                        }
                    },
                    |err| warn!("Monitor output error: {}", err),
                    None,
                )
                .expect("build monitor output stream failed");
            stream.play().expect("monitor stream play failed");
            let _ = stop_rx.recv();
        });

        Ok(Self { _stop: stop_tx })
    }
}
