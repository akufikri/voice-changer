use anyhow::Result;
use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::multipart;
use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};
use serde::Deserialize;
use tokio::sync::mpsc;
use tracing::info;
use std::sync::atomic::Ordering;

use super::types::{Voice, VoiceProvider};

pub struct ElevenLabsProvider {
    api_key: String,
    voice_id: String,
    client: reqwest::Client,
    /// Remembers whether pcm_44100 was rejected (403) so later requests skip it.
    /// pcm_44100 requires a Pro-tier account; most tiers only get MP3.
    pcm_denied: std::sync::atomic::AtomicBool,
}

// Input encoding for STS uploads.
// `pcm_s16le_16` = raw 16-bit LE mono 16kHz PCM with file_format=pcm_s16le_16 —
// documented by ElevenLabs as LOWER LATENCY than passing an encoded waveform
// (no WAV/MP3 decode step server-side). Falls back to WAV 48kHz when disabled.
// NOTE: ElevenLabs has NO public WebSocket endpoint for speech-to-speech yet —
// their stream-input WS is Text-to-Speech only. When one ships, this provider is
// the swap point (VoiceProvider::convert_stream stays the engine interface).
const LOW_LATENCY_PCM16_INPUT: bool = true;

#[derive(Deserialize)]
struct VoiceListResponse {
    voices: Vec<ElevenLabsVoice>,
}

#[derive(Deserialize)]
struct ElevenLabsVoice {
    voice_id: String,
    name: String,
}

impl ElevenLabsProvider {
    pub fn new(api_key: String, voice_id: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()
            .expect("reqwest client");
        Self {
            api_key,
            voice_id,
            client,
            pcm_denied: std::sync::atomic::AtomicBool::new(false),
        }
    }
}

#[async_trait]
impl VoiceProvider for ElevenLabsProvider {
    async fn list_voices(&self) -> Result<Vec<Voice>> {
        let resp = self
            .client
            .get("https://api.elevenlabs.io/v1/voices")
            .header("xi-api-key", &self.api_key)
            .send()
            .await?
            .error_for_status()?
            .json::<VoiceListResponse>()
            .await?;

        Ok(resp
            .voices
            .into_iter()
            .map(|v| Voice {
                id: v.voice_id,
                name: v.name,
                language: None,
                provider: "elevenlabs".into(),
            })
            .collect())
    }

    async fn convert(&self, audio: Vec<f32>) -> Result<Vec<f32>> {
        let (tx, mut rx) = mpsc::channel(32);
        self.convert_stream(audio, tx).await?;
        let mut out = Vec::new();
        while let Some(chunk) = rx.recv().await {
            out.extend_from_slice(&chunk);
        }
        Ok(out)
    }

    /// Streams MP3 frames to `tx` as HTTP response bytes arrive.
    // ponytail: HTTP per utterance. True realtime needs ElevenLabs WebSocket STS when available.
    async fn convert_stream(&self, audio: Vec<f32>, tx: mpsc::Sender<Vec<f32>>) -> Result<()> {
        // Input encoding: raw 16kHz PCM S16LE (lower latency, documented) or WAV 48kHz.
        let (audio_bytes, mime, file_format) = if LOW_LATENCY_PCM16_INPUT {
            let mono_16k = resample_fft(audio.clone(), 48_000, 16_000)?;
            let pcm16: Vec<u8> = mono_16k
                .iter()
                .flat_map(|&s| ((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes())
                .collect();
            (pcm16, "application/octet-stream", "pcm_s16le_16")
        } else {
            (pcm_f32_to_wav(&audio, 48_000, 1), "audio/wav", "other")
        };

        // Request PCM first (best quality). Some account tiers get HTTP 403 on pcm_44100
        // (Pro-tier only); on denial we flip pcm_denied and retry with mp3_44100_128,
        // which is available on every tier.
        let mut format = if self.pcm_denied.load(Ordering::Relaxed) {
            "mp3_44100_128"
        } else {
            "pcm_44100"
        };
        let url = |fmt: &str| {
            format!(
                "https://api.elevenlabs.io/v1/speech-to-speech/{}/stream?output_format={fmt}",
                self.voice_id
            )
        };

        let voice_settings = serde_json::json!({
            "stability": 0.5,
            "similarity_boost": 0.5,
            "style": 0.0,
            "use_speaker_boost": true
        })
        .to_string();

        let build_form = || -> Result<multipart::Form> {
            Ok(multipart::Form::new()
                .part(
                    "audio",
                    multipart::Part::bytes(audio_bytes.clone())
                        .file_name("audio.pcm")
                        .mime_str(mime)?,
                )
                .text("model_id", "eleven_multilingual_sts_v2")
                .text("file_format", file_format)
                .text("voice_settings", voice_settings.clone()))
        };

        // NOTE: do NOT send `Accept: audio/mpeg` here — it nudges the endpoint toward
        // MP3 even when PCM was requested.
        let mut response = self
            .client
            .post(url(&format))
            .header("xi-api-key", &self.api_key)
            .multipart(build_form()?)
            .send()
            .await?;

        if response.status() == reqwest::StatusCode::FORBIDDEN && format == "pcm_44100" {
            info!("pcm_44100 forbidden for this account tier — falling back to mp3_44100_128");
            self.pcm_denied.store(true, Ordering::Relaxed);
            format = "mp3_44100_128";
            response = self
                .client
                .post(url(&format))
                .header("xi-api-key", &self.api_key)
                .multipart(build_form()?)
                .send()
                .await?;
        }

        let response = response.error_for_status()?;

        // Collect response bytes (pcm_44100 = 16-bit signed LE, mono, 44100 Hz)
        let mut pcm_buf: Vec<u8> = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            pcm_buf.extend_from_slice(&chunk?);
        }

        info!("STS response: {} bytes", pcm_buf.len());

        // Detect the actual container. Defaults to raw S16LE PCM at 44.1kHz.
        let (mono, in_rate): (Vec<f32>, u32) =
            if pcm_buf.starts_with(b"RIFF") && pcm_buf.len() > 44 {
                // WAV wrapper: sample rate lives at byte offset 24 (fmt chunk)
                let rate = u32::from_le_bytes([
                    pcm_buf[24], pcm_buf[25], pcm_buf[26], pcm_buf[27],
                ]);
                let samples: Vec<f32> = pcm_buf[44..]
                    .chunks_exact(2)
                    .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0)
                    .collect();
                (samples, rate)
            } else if pcm_buf.starts_with(b"ID3")
                || (pcm_buf.len() > 2
                    && pcm_buf[0] == 0xFF
                    && (pcm_buf[1] & 0xE0) == 0xE0)
            {
                // MP3 (ID3 tag or MPEG frame sync) — account tier without PCM access.
                let (samples, rate) = decode_mp3(&pcm_buf)?;
                info!("Response was MP3 — decoded {} samples @ {}Hz", samples.len(), rate);
                (samples, rate)
            } else {
                let samples: Vec<f32> = pcm_buf
                    .chunks_exact(2)
                    .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0)
                    .collect();
                (samples, 44_100)
            };

        info!("PCM decoded: {}Hz 1ch {} samples", in_rate, mono.len());

        // Resample to engine rate (FFT resampler for voice quality)
        let out = resample_fft(mono, in_rate, 48_000)?;

        tx.send(out).await.ok();
        Ok(())
    }

    fn name(&self) -> &str {
        "elevenlabs"
    }
}


fn pcm_f32_to_wav(samples: &[f32], sample_rate: u32, channels: u16) -> Vec<u8> {
    let pcm: Vec<u8> = samples
        .iter()
        .flat_map(|&s| ((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes())
        .collect();
    let data_len = pcm.len() as u32;
    let byte_rate = sample_rate * channels as u32 * 2;
    let mut wav = Vec::with_capacity(44 + pcm.len());
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&channels.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&byte_rate.to_le_bytes());
    wav.extend_from_slice(&(channels * 2).to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    wav.extend_from_slice(&pcm);
    wav
}

/// Decode MP3 bytes → mono f32 + sample rate, using minimp3.
fn decode_mp3(data: &[u8]) -> Result<(Vec<f32>, u32)> {
    let mut decoder = minimp3::Decoder::new(std::io::Cursor::new(data.to_vec()));
    let mut samples: Vec<f32> = Vec::new();
    let mut rate = 44_100u32;
    loop {
        match decoder.next_frame() {
            Ok(frame) => {
                rate = frame.sample_rate as u32;
                let ch = frame.channels as usize;
                if ch == 1 {
                    samples.extend(frame.data.iter().map(|&s| s as f32 / 32768.0));
                } else {
                    // Downmix: average channel pairs
                    for c in frame.data.chunks(ch) {
                        let sum: f32 = c.iter().map(|&s| s as f32).sum::<f32>() / ch as f32;
                        samples.push(sum / 32768.0);
                    }
                }
            }
            Err(minimp3::Error::Eof) => break,
            Err(e) => return Err(e.into()),
        }
    }
    Ok((samples, rate))
}

/// High-quality Sinc resampling (rubato). Better than linear for voice.
fn resample_fft(input: Vec<f32>, in_rate: u32, out_rate: u32) -> Result<Vec<f32>> {
    if in_rate == out_rate { return Ok(input); }
    let params = SincInterpolationParameters {
        sinc_len: 128,
        f_cutoff: 0.95,
        interpolation: SincInterpolationType::Linear,
        oversampling_factor: 128,
        window: WindowFunction::BlackmanHarris2,
    };
    let ratio = out_rate as f64 / in_rate as f64;
    let chunk = input.len();
    let mut resampler = SincFixedIn::<f32>::new(ratio, 2.0, params, chunk, 1)?;
    let out = resampler.process(&[input], None)?;
    Ok(out.into_iter().next().unwrap_or_default())
}

