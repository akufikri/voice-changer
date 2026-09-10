use anyhow::Result;
use ringbuf::traits::{Consumer, Observer, Producer};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};
use tauri::async_runtime::JoinHandle;
use tokio::sync::{mpsc, watch};
use tracing::{info, warn};

use super::{
    capture::AudioCapture, monitor::OutputMonitor, ring_buffer::AudioRingBuffer,
    vad::Vad, virtual_mic::VirtualMic,
};
use crate::provider::types::VoiceProvider;

const RING_BUF_CAPACITY: usize = 384_000;
// Recorder-style buffering: capture a full utterance (until 600ms of quiet), convert
// it as ONE request, then play it out. 1.5s rolling cap for continuous speech.
const ACCUMULATE_MS: usize = 1_500;
// Minimum speech worth converting — smaller bursts are treated as noise.
const MIN_FLUSH_MS: usize = 200;
const AI_QUEUE_SIZE: usize = 3; // in-flight + queued; bounded post-stop audio

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum EngineState {
    Idle,
    Initializing,
    Ready,
    Processing,
    Degraded,
    Error(String),
}

pub struct AudioEngine {
    state: Arc<Mutex<EngineState>>,
    processing_enabled: Arc<Mutex<bool>>,
    monitor_enabled: Arc<Mutex<bool>>,
    state_tx: watch::Sender<EngineState>,
    _capture: Option<AudioCapture>,
    _virtual_mic: Option<VirtualMic>,
    _monitor: Option<OutputMonitor>,
    virtual_mic_name: Option<String>,
    monitor_buf: Option<Arc<Mutex<AudioRingBuffer>>>,
    out_buf_ref: Option<Arc<Mutex<AudioRingBuffer>>>,
    _task_handles: Vec<JoinHandle<()>>,
}

unsafe impl Send for AudioEngine {}

impl AudioEngine {
    pub fn new() -> (Self, watch::Receiver<EngineState>) {
        let (state_tx, state_rx) = watch::channel(EngineState::Idle);
        (
            Self {
                state: Arc::new(Mutex::new(EngineState::Idle)),
                processing_enabled: Arc::new(Mutex::new(false)),
                monitor_enabled: Arc::new(Mutex::new(false)),
                state_tx,
                _capture: None,
                _virtual_mic: None,
                _monitor: None,
                virtual_mic_name: None,
                monitor_buf: None,
                out_buf_ref: None,
                _task_handles: Vec::new(),
            },
            state_rx,
        )
    }

    pub fn start(
        &mut self,
        device_name: Option<String>,
        provider: Arc<dyn VoiceProvider>,
    ) -> Result<()> {
        // Abort all tasks from any previous start() before spawning new ones.
        for h in self._task_handles.drain(..) { h.abort(); }
        self._capture = None;
        self._virtual_mic = None;
        self._monitor = None;

        self.set_state(EngineState::Initializing);
        info!("Starting audio engine");

        let in_buf = Arc::new(Mutex::new(AudioRingBuffer::new(RING_BUF_CAPACITY)));
        let out_buf = Arc::new(Mutex::new(AudioRingBuffer::new(RING_BUF_CAPACITY)));
        let monitor_buf = Arc::new(Mutex::new(AudioRingBuffer::new(RING_BUF_CAPACITY)));

        // Single output path: bypass AND AI audio both write straight into out_buf.
        // No timer-mixer: tokio sleeps overshoot → sample deficit → robotic crackle.
        self.out_buf_ref = Some(out_buf.clone());
        self.monitor_buf = Some(monitor_buf.clone());

        // Echo gate deadline: ms-since-engine-start until which our own AI output is
        // still playing. Task 1 extends it whenever it pushes audio; Task 3 refuses to
        // record while now < deadline. NOTE: no level-based release — a release lets
        // the AI voice re-enter the mic and re-dispatch, which loops the output.
        let ai_playing_until = Arc::new(AtomicU64::new(0));
        let engine_started = std::time::Instant::now();

        let capture = AudioCapture::start(device_name.as_deref(), in_buf.clone())?;
        let capture_channels = capture.channels;
        let capture_rate = capture.sample_rate;
        self._capture = Some(capture);

        let virtual_mic = VirtualMic::start(out_buf.clone())?;
        let out_channels = virtual_mic.channels;
        let out_rate = virtual_mic.sample_rate;
        self.virtual_mic_name = Some(virtual_mic.device_name.clone());
        self._virtual_mic = Some(virtual_mic);

        info!(
            "Audio pipeline: capture={}Hz/{}ch → output={}Hz/{}ch",
            capture_rate, capture_channels, out_rate, out_channels
        );

        let processing_enabled = self.processing_enabled.clone();
        let monitor_enabled = self.monitor_enabled.clone();
        let state = self.state.clone();
        let state_tx = self.state_tx.clone();

        let accumulate_samples = (capture_rate as usize * ACCUMULATE_MS) / 1000;

        // AI result channel: elevenlabs → Task 1 → out_buf
        let (ai_tx, mut ai_rx) = mpsc::channel::<Vec<f32>>(64);

        // Chunk queue: accumulator → sequential AI processor
        let (chunk_tx, mut chunk_rx) = mpsc::channel::<Vec<f32>>(AI_QUEUE_SIZE);

        // ── Processor: sequential AI requests (queued, never overlapping) ─────
        {
            let ai_tx = ai_tx.clone();
            let provider = provider.clone();
            let h = tauri::async_runtime::spawn(async move {
                while let Some(chunk) = chunk_rx.recv().await {
                    if let Err(e) = provider.convert_stream(chunk, ai_tx.clone()).await {
                        if e.to_string().contains("429") {
                            warn!("Voice conversion rate limited (429) — chunk dropped.");
                        } else {
                            warn!("Voice conversion failed: {e}. DEGRADED → bypass.");
                            let _ = state_tx.send(EngineState::Degraded);
                            *state.lock().unwrap() = EngineState::Degraded;
                        }
                    }
                }
            });
            self._task_handles.push(h);
        }

        // ── Task 1: AI results → out_buf (direct write, no mixer hop) ────────
        {
            let out_buf = out_buf.clone();
            let monitor_buf = monitor_buf.clone();
            let monitor_enabled = monitor_enabled.clone();
            let ai_playing_until = ai_playing_until.clone();
            let t0 = engine_started;
            let out_ch = out_channels;
            let out_rate_ = out_rate;
            let h = tauri::async_runtime::spawn(async move {
                // Short edge fades per chunk kill clicks at MP3 chunk boundaries.
                let edge = (out_rate_ as f32 * 5.0 / 1000.0) as usize; // 5ms
                while let Some(ai_mono) = ai_rx.recv().await {
                    if ai_mono.is_empty() { continue; }
                    let mut out_chunk = ai_mono;

                    let fi = edge.min(out_chunk.len());
                    for i in 0..fi {
                        out_chunk[i] *= i as f32 / fi as f32;
                    }
                    let fo = edge.min(out_chunk.len());
                    for i in 0..fo {
                        let idx = out_chunk.len() - 1 - i;
                        out_chunk[idx] *= i as f32 / fo as f32;
                    }

                    let stereo = expand_channels(&out_chunk, 1, out_ch);
                    // Provider output is already 48kHz mono from ElevenLabsProvider.
                    let stereo = if out_rate_ != 48_000 {
                        resample_linear(&stereo, out_ch, 48_000, out_rate_)
                    } else { stereo };

                    if let Ok(mut rb) = out_buf.lock() {
                        rb.producer.push_slice(&stereo);
                    }
                    if *monitor_enabled.lock().unwrap_or_else(|e| e.into_inner()) {
                        if let Ok(mut rb) = monitor_buf.lock() {
                            rb.producer.push_slice(&stereo);
                        }
                    }

                    // Extend echo gate: this chunk just entered playout. Deadline =
                    // estimated moment its tail leaves the speakers, +200ms margin.
                    let chunk_ms = (stereo.len() as u64 * 1000)
                        / (out_rate_ as u64 * out_ch as u64);
                    let now_ms = t0.elapsed().as_millis() as u64;
                    ai_playing_until.fetch_max(now_ms + chunk_ms + 200, Ordering::Relaxed);
                }
            });
            self._task_handles.push(h);
        }

        // ── Task 3: Capture → frame → VAD → accumulate → dispatch ───────────
        // Recorder mechanism: buffer one utterance, convert it whole, play it out.
        let ai_playing_until = ai_playing_until.clone();
        let t0_capture = engine_started;
        let h = tauri::async_runtime::spawn(async move {
            // VAD frames are 100ms. Feeding the VAD one 10ms loop iteration per call
            // flickered mid-sentence and split speech into tiny chunks — each chunk
            // then became a separate STS request with no context (robotic, choppy).
            let frame_len = (capture_rate as usize * 100) / 1000;
            let mut frame_buf: Vec<f32> = Vec::with_capacity(frame_len);
            let mut chunk_buf: Vec<f32> = Vec::with_capacity(accumulate_samples * 2);
            // -40dB threshold (rejects headset bleed). pre_roll=2 (200ms),
            // post_roll=6 (600ms): an utterance only ends after 600ms of quiet, so
            // natural pauses between words don't split it into separate conversions.
            let mut vad = Vad::new(-40.0, 2, 6);
            let min_flush_samples = (capture_rate as usize * MIN_FLUSH_MS) / 1000;
            let mut chunk_has_speech = false;

            // Drift correction: the 48kHz sample clock runs slightly off vs the host
            // wall clock (ppm-level). A fixed 10ms poll then loses track over minutes
            // and backlogs in_buf. Measure captured duration vs wall time every ~1s
            // and nudge the sleep within [-5ms, +10ms].
            let mut captured_total: usize = 0;
            let mut window_start = t0_capture.elapsed();
            let mut sleep_adjust_ms: i64 = 0;

            loop {
                tokio::time::sleep(std::time::Duration::from_millis(
                    (10 + sleep_adjust_ms).max(1) as u64,
                ))
                .await;

                let captured: Vec<f32> = {
                    let Ok(mut rb) = in_buf.lock() else { continue };
                    let n = rb.consumer.occupied_len();
                    if n == 0 { continue; }
                    let mut buf = vec![0f32; n];
                    rb.consumer.pop_slice(&mut buf);
                    buf
                };
                captured_total += captured.len();

                if captured_total >= capture_rate as usize {
                    let capture_secs = captured_total as f64 / capture_rate as f64;
                    let wall_secs = t0_capture.elapsed().as_secs_f64()
                        - window_start.as_secs_f64();
                    let drift = capture_secs - wall_secs;
                    if drift > 0.005 {
                        sleep_adjust_ms = (sleep_adjust_ms - 1).max(-5);
                    } else if drift < -0.005 {
                        sleep_adjust_ms = (sleep_adjust_ms + 1).min(10);
                    }
                    captured_total = 0;
                    window_start = t0_capture.elapsed();
                }

                // Downmix to mono
                let mono: Vec<f32> = if capture_channels == 2 {
                    captured.chunks(2).map(|c| (c[0] + c[1]) * 0.5).collect()
                } else {
                    captured
                };

                let enabled = *processing_enabled.lock().unwrap_or_else(|e| e.into_inner());

                if !enabled {
                    // Bypass mode: straight into out_buf for passthrough.
                    let bypass = expand_channels(&mono, 1, out_channels);
                    let bypass = if capture_rate != out_rate {
                        resample_linear(&bypass, out_channels, capture_rate, out_rate)
                    } else { bypass };
                    if let Ok(mut rb) = out_buf.lock() {
                        rb.producer.push_slice(&bypass);
                    }
                    if *monitor_enabled.lock().unwrap_or_else(|e| e.into_inner()) {
                        if let Ok(mut rb) = monitor_buf.lock() {
                            rb.producer.push_slice(&bypass);
                        }
                    }
                    chunk_buf.clear();
                    frame_buf.clear();
                    chunk_has_speech = false;
                } else {
                    let now_ms = t0_capture.elapsed().as_millis() as u64;
                    let gated = now_ms < ai_playing_until.load(Ordering::Relaxed);

                    if gated {
                        // Our own AI output is still playing — don't record it again.
                        frame_buf.clear();
                    } else {
                        frame_buf.extend_from_slice(&mono);
                        if frame_buf.len() >= frame_len {
                            let frame: Vec<f32> = frame_buf.drain(..frame_len).collect();

                            let frame_is_speech = vad.process(&frame);
                            if frame_is_speech { chunk_has_speech = true; }
                            chunk_buf.extend_from_slice(&frame);

                            // Dispatch when the accumulate window fills (continuous
                            // speech) or the utterance just ended — trailing silence
                            // is never shipped to the AI.
                            let speech_stopped = chunk_has_speech
                                && !frame_is_speech
                                && chunk_buf.len() >= min_flush_samples;
                            if chunk_buf.len() >= accumulate_samples || speech_stopped {
                                let to_send = std::mem::take(&mut chunk_buf);
                                if chunk_has_speech {
                                    info!(
                                        "Dispatching {}ms chunk to AI",
                                        to_send.len() * 1000 / capture_rate as usize
                                    );
                                    if chunk_tx.try_send(to_send).is_err() {
                                        warn!("AI queue full — chunk dropped.");
                                    }
                                } else {
                                    info!("Skipping silent chunk (VAD)");
                                }
                                chunk_has_speech = false;
                            }
                        }
                    }
                }
            }
        });
        self._task_handles.push(h);

        self.set_state(EngineState::Ready);
        Ok(())
    }

    pub fn set_processing(&self, enabled: bool) {
        if let Ok(mut g) = self.processing_enabled.lock() { *g = enabled; }
        // Drain the output buffer on mode switch to prevent audio bleed-through.
        if let Some(buf) = &self.out_buf_ref {
            if let Ok(mut rb) = buf.lock() {
                let n = rb.consumer.occupied_len();
                rb.consumer.skip(n);
            }
        }
        self.set_state(if enabled { EngineState::Processing } else { EngineState::Ready });
    }

    pub fn set_monitor(&mut self, enabled: bool) -> Result<()> {
        if let Ok(mut g) = self.monitor_enabled.lock() { *g = enabled; }
        if enabled {
            if self._monitor.is_none() {
                let buf = self.monitor_buf.clone()
                    .ok_or_else(|| anyhow::anyhow!("Engine not started"))?;
                self._monitor = Some(OutputMonitor::start(buf)?);
            }
        } else {
            self._monitor = None;
        }
        Ok(())
    }

    pub fn monitor_active(&self) -> bool { self._monitor.is_some() }

    pub fn stop(&mut self) {
        for h in self._task_handles.drain(..) { h.abort(); }
        self._capture = None;
        self._virtual_mic = None;
        self._monitor = None;
        self.monitor_buf = None;
        self.out_buf_ref = None;
        self.virtual_mic_name = None;
        self.set_state(EngineState::Idle);
    }

    pub fn virtual_mic_name(&self) -> Option<&str> { self.virtual_mic_name.as_deref() }

    pub fn current_state(&self) -> EngineState {
        self.state.lock().map(|g| g.clone()).unwrap_or(EngineState::Idle)
    }

    fn set_state(&self, s: EngineState) {
        let _ = self.state_tx.send(s.clone());
        if let Ok(mut g) = self.state.lock() { *g = s; }
    }
}

fn expand_channels(input: &[f32], in_ch: u16, out_ch: u16) -> Vec<f32> {
    if in_ch == out_ch { return input.to_vec(); }
    match (in_ch, out_ch) {
        (1, 2) => {
            let mut out = Vec::with_capacity(input.len() * 2);
            for &s in input { out.push(s); out.push(s); }
            out
        }
        (2, 1) => input.chunks(2).map(|c| (c[0] + c[1]) * 0.5).collect(),
        _ => input.to_vec(),
    }
}

fn resample_linear(input: &[f32], channels: u16, in_rate: u32, out_rate: u32) -> Vec<f32> {
    if in_rate == out_rate { return input.to_vec(); }
    let ch = channels as usize;
    let in_frames = input.len() / ch;
    let out_frames = (in_frames as f64 * out_rate as f64 / in_rate as f64) as usize;
    let mut output = Vec::with_capacity(out_frames * ch);
    for i in 0..out_frames {
        let pos = i as f64 * in_rate as f64 / out_rate as f64;
        let idx = pos as usize;
        let frac = (pos - idx as f64) as f32;
        for c in 0..ch {
            let s0 = input.get(idx * ch + c).copied().unwrap_or(0.0);
            let s1 = input.get((idx + 1) * ch + c).copied().unwrap_or(s0);
            output.push(s0 + (s1 - s0) * frac);
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_mono_to_stereo_doubles() {
        let stereo = expand_channels(&[0.1, 0.2], 1, 2);
        assert_eq!(stereo, vec![0.1, 0.1, 0.2, 0.2]);
    }

    #[test]
    fn resample_passthrough_same_rate() {
        let input = vec![0.1f32, 0.2, 0.3];
        assert_eq!(resample_linear(&input, 1, 48_000, 48_000), input);
    }
}
