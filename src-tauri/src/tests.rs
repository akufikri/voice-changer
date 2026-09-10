/// Unit tests — simulate speaking behavior without real audio hardware.
///
/// Run: cargo test -- --nocapture
#[cfg(test)]
mod tests {
    use std::f32::consts::PI;
    use std::sync::Arc;

    use anyhow::Result;
    use async_trait::async_trait;
    use ringbuf::traits::{Consumer, Observer, Producer};
    use tokio::sync::mpsc;

    use crate::audio::{ring_buffer::AudioRingBuffer, vad::Vad};
    use crate::provider::types::{Voice, VoiceProvider};

    // ── Helpers ──────────────────────────────────────────────────────────────

    /// Generate a 440Hz sine wave — simulates speech-like audio.
    fn sine_wave(n_samples: usize, sample_rate: u32, freq: f32, amplitude: f32) -> Vec<f32> {
        (0..n_samples)
            .map(|i| amplitude * (2.0 * PI * freq * i as f32 / sample_rate as f32).sin())
            .collect()
    }

    /// Generate silence (all zeros).
    fn silence(n_samples: usize) -> Vec<f32> {
        vec![0.0f32; n_samples]
    }

    /// Expand mono → stereo: duplicate each sample (L=R).
    fn expand_mono_to_stereo(input: &[f32]) -> Vec<f32> {
        let mut out = Vec::with_capacity(input.len() * 2);
        for &s in input {
            out.push(s);
            out.push(s);
        }
        out
    }

    /// Downmix stereo → mono: average L+R pairs.
    fn downmix_stereo_to_mono(input: &[f32]) -> Vec<f32> {
        input.chunks(2).map(|c| (c[0] + c[1]) * 0.5).collect()
    }

    /// Linear resample (mono).
    fn resample_linear(input: &[f32], in_rate: u32, out_rate: u32) -> Vec<f32> {
        if in_rate == out_rate {
            return input.to_vec();
        }
        let out_len = (input.len() as f64 * out_rate as f64 / in_rate as f64) as usize;
        let mut out = Vec::with_capacity(out_len);
        for i in 0..out_len {
            let pos = i as f64 * in_rate as f64 / out_rate as f64;
            let idx = pos as usize;
            let frac = (pos - idx as f64) as f32;
            let s0 = input.get(idx).copied().unwrap_or(0.0);
            let s1 = input.get(idx + 1).copied().unwrap_or(s0);
            out.push(s0 + (s1 - s0) * frac);
        }
        out
    }

    // ── Mock provider ────────────────────────────────────────────────────────

    /// Mock ElevenLabs — returns a 440Hz sine wave of same length as input.
    struct MockProvider;

    #[async_trait]
    impl VoiceProvider for MockProvider {
        async fn list_voices(&self) -> Result<Vec<Voice>> {
            Ok(vec![])
        }

        async fn convert(&self, audio: Vec<f32>) -> Result<Vec<f32>> {
            // Simulate ElevenLabs: return AI-converted voice (440Hz tone, same length)
            Ok(sine_wave(audio.len(), 48_000, 440.0, 0.5))
        }

        async fn convert_stream(&self, audio: Vec<f32>, tx: mpsc::Sender<Vec<f32>>) -> Result<()> {
            let result = self.convert(audio).await?;
            tx.send(result).await.ok();
            Ok(())
        }

        fn name(&self) -> &str {
            "mock"
        }
    }

    // ── VAD tests ────────────────────────────────────────────────────────────

    #[test]
    fn vad_silence_not_detected_as_speech() {
        let mut vad = Vad::new(-35.0, 2, 3);
        let frame = silence(4800); // 100ms at 48kHz
        for _ in 0..5 {
            assert!(!vad.process(&frame), "silence should not be speech");
        }
    }

    #[test]
    fn vad_loud_sine_detected_as_speech() {
        let mut vad = Vad::new(-35.0, 2, 3);
        // pre_roll=2 → need 2 frames above threshold
        let frame = sine_wave(4800, 48_000, 300.0, 0.8);
        let _ = vad.process(&frame); // frame 1
        let detected = vad.process(&frame); // frame 2 — should trigger
        assert!(detected, "loud sine should be detected as speech after pre_roll");
    }

    #[test]
    fn vad_speech_then_silence_clears_after_post_roll() {
        let mut vad = Vad::new(-35.0, 1, 3);
        let speech = sine_wave(4800, 48_000, 300.0, 0.8);
        let quiet = silence(4800);

        vad.process(&speech);
        assert!(vad.process(&speech)); // confirmed speech

        // Silence for post_roll frames
        vad.process(&quiet);
        vad.process(&quiet);
        let after = vad.process(&quiet);
        assert!(!after, "should clear speech after post_roll silence frames");
    }

    #[test]
    fn vad_threshold_boundary() {
        let mut vad = Vad::new(-35.0, 1, 1);
        // amplitude 0.018 → energy ≈ 0.018²/2 ≈ 1.6e-4 → 10*log10 ≈ -38dB (below threshold)
        let soft = sine_wave(4800, 48_000, 300.0, 0.018);
        // amplitude 0.08 → energy ≈ 3.2e-3 → 10*log10 ≈ -25dB (above threshold)
        let loud = sine_wave(4800, 48_000, 300.0, 0.08);

        assert!(!vad.process(&soft), "soft audio below threshold");
        assert!(vad.process(&loud), "loud audio above threshold");
    }

    // ── Channel expansion tests ───────────────────────────────────────────────

    #[test]
    fn mono_to_stereo_doubles_samples() {
        let mono = vec![0.1, 0.2, 0.3, 0.4];
        let stereo = expand_mono_to_stereo(&mono);
        assert_eq!(stereo.len(), mono.len() * 2);
        assert_eq!(stereo, vec![0.1, 0.1, 0.2, 0.2, 0.3, 0.3, 0.4, 0.4]);
    }

    #[test]
    fn stereo_to_mono_averages_pairs() {
        let stereo = vec![0.2, 0.4, 0.6, 0.8];
        let mono = downmix_stereo_to_mono(&stereo);
        assert_eq!(mono.len(), 2);
        assert!((mono[0] - 0.3).abs() < 1e-6);
        assert!((mono[1] - 0.7).abs() < 1e-6);
    }

    #[test]
    fn mono_to_stereo_then_back_to_mono_is_identity() {
        let original = sine_wave(100, 48_000, 440.0, 0.5);
        let stereo = expand_mono_to_stereo(&original);
        let back = downmix_stereo_to_mono(&stereo);
        for (a, b) in original.iter().zip(back.iter()) {
            assert!((a - b).abs() < 1e-6, "roundtrip failed: {} != {}", a, b);
        }
    }

    // ── Resample tests ────────────────────────────────────────────────────────

    #[test]
    fn resample_upsample_correct_length() {
        let input = sine_wave(4800, 48_000, 440.0, 0.5); // 100ms at 48kHz
        let output = resample_linear(&input, 48_000, 96_000);
        assert_eq!(output.len(), 9600, "2x upsample → double length");
    }

    #[test]
    fn resample_downsample_correct_length() {
        let input = sine_wave(48_000, 48_000, 440.0, 0.5); // 1s at 48kHz
        let output = resample_linear(&input, 48_000, 16_000);
        assert_eq!(output.len(), 16_000, "48k→16k → 1/3 length");
    }

    #[test]
    fn resample_same_rate_is_passthrough() {
        let input = sine_wave(1000, 48_000, 440.0, 0.5);
        let output = resample_linear(&input, 48_000, 48_000);
        assert_eq!(input, output);
    }

    #[test]
    fn resample_44100_to_48000_expected_length() {
        // ElevenLabs returns 44100Hz, engine needs 48000Hz
        let samples_44k = sine_wave(44_100, 44_100, 440.0, 0.5); // 1s
        let samples_48k = resample_linear(&samples_44k, 44_100, 48_000);
        // Expected: 48000 samples (1s at 48kHz)
        assert_eq!(samples_48k.len(), 48_000);
    }

    // ── Ring buffer tests ─────────────────────────────────────────────────────

    #[test]
    fn ring_buffer_push_pop_roundtrip() {
        let mut rb = AudioRingBuffer::new(1024);
        let data = sine_wave(100, 48_000, 440.0, 0.5);
        rb.producer.push_slice(&data);

        assert_eq!(rb.consumer.occupied_len(), 100);

        let mut out = vec![0f32; 100];
        rb.consumer.pop_slice(&mut out);
        for (a, b) in data.iter().zip(out.iter()) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn delay_buf_vs_ai_buf_mixer_logic() {
        // Simulate: delay_buf has bypass, ai_buf has AI audio.
        // Mixer should prefer ai_buf.
        let mut delay_buf = AudioRingBuffer::new(4096);
        let mut ai_buf = AudioRingBuffer::new(4096);

        let bypass_audio = vec![0.1f32; 480]; // 10ms bypass
        let ai_audio = vec![0.9f32; 480];     // 10ms AI audio (distinct value)

        delay_buf.producer.push_slice(&bypass_audio);
        ai_buf.producer.push_slice(&ai_audio);

        let mixer_frame = 480;

        // Mixer logic: prefer ai_buf
        let output = if ai_buf.consumer.occupied_len() >= mixer_frame {
            let mut f = vec![0f32; mixer_frame];
            ai_buf.consumer.pop_slice(&mut f);
            f
        } else {
            let mut f = vec![0f32; mixer_frame];
            delay_buf.consumer.pop_slice(&mut f);
            f
        };

        assert!(
            output.iter().all(|&s| (s - 0.9).abs() < 1e-6),
            "mixer should output AI audio, not bypass"
        );
        // delay_buf not consumed
        assert_eq!(delay_buf.consumer.occupied_len(), 480, "delay_buf untouched when AI available");
    }

    #[test]
    fn delay_buf_fallback_when_ai_empty() {
        let mut delay_buf = AudioRingBuffer::new(4096);
        let mut ai_buf = AudioRingBuffer::new(4096);

        let bypass_audio = vec![0.1f32; 480];
        delay_buf.producer.push_slice(&bypass_audio);
        // ai_buf empty

        let mixer_frame = 480;

        let output = if ai_buf.consumer.occupied_len() >= mixer_frame {
            let mut f = vec![0f32; mixer_frame];
            ai_buf.consumer.pop_slice(&mut f);
            f
        } else {
            let mut f = vec![0f32; mixer_frame];
            delay_buf.consumer.pop_slice(&mut f);
            f
        };

        assert!(
            output.iter().all(|&s| (s - 0.1).abs() < 1e-6),
            "mixer should fallback to bypass when AI empty"
        );
    }

    // ── Mock provider tests ───────────────────────────────────────────────────

    #[tokio::test]
    async fn mock_provider_returns_same_length_audio() {
        let provider = MockProvider;
        // Simulate 300ms of speech at 48kHz
        let speech = sine_wave(14_400, 48_000, 200.0, 0.8);
        let result = provider.convert(speech.clone()).await.unwrap();
        assert_eq!(result.len(), speech.len(), "provider output length matches input");
    }

    #[tokio::test]
    async fn mock_provider_stream_sends_chunks() {
        let provider = MockProvider;
        let speech = sine_wave(14_400, 48_000, 200.0, 0.8);

        let (tx, mut rx) = mpsc::channel(16);
        provider.convert_stream(speech, tx).await.unwrap();

        let chunk = rx.recv().await.expect("should receive at least one chunk");
        assert!(!chunk.is_empty(), "chunk must not be empty");
    }

    // ── Simulate speaking scenario ─────────────────────────────────────────────

    /// Full speaking simulation:
    ///   1. Generate audio frames (silence → speech → silence)
    ///   2. Run through VAD
    ///   3. Accumulate speech → send to mock provider
    ///   4. Verify output contains converted audio
    #[tokio::test]
    async fn simulate_speaking_abcd() {
        let provider = Arc::new(MockProvider);
        let sample_rate = 48_000u32;
        let vad_samples = (sample_rate as usize * 100) / 1000; // 100ms frames
        let dispatch_samples = (sample_rate as usize * 300) / 1000; // 300ms threshold

        let mut vad = Vad::new(-35.0, 2, 3);
        let mut speech_buf: Vec<f32> = Vec::new();
        let mut ai_results: Vec<Vec<f32>> = Vec::new();

        // Build audio scenario: 200ms silence, 500ms speech ("ABCD"), 300ms silence
        let silence_frames = 2; // 200ms
        let speech_frames = 5;  // 500ms
        let end_silence = 3;    // 300ms (triggers silence flush)

        let mut all_frames: Vec<Vec<f32>> = Vec::new();
        for _ in 0..silence_frames {
            all_frames.push(silence(vad_samples));
        }
        for _ in 0..speech_frames {
            all_frames.push(sine_wave(vad_samples, sample_rate, 200.0, 0.8));
        }
        for _ in 0..end_silence {
            all_frames.push(silence(vad_samples));
        }

        // Process frames through VAD + speech accumulator
        let mut silence_after_speech = 0usize;
        for frame in &all_frames {
            let is_speech = vad.process(frame);

            if is_speech {
                speech_buf.extend_from_slice(frame);
                silence_after_speech = 0;

                if speech_buf.len() >= dispatch_samples {
                    let to_send: Vec<f32> = speech_buf.drain(..dispatch_samples).collect();
                    let (tx, mut rx) = mpsc::channel(4);
                    provider.convert_stream(to_send, tx).await.unwrap();
                    while let Ok(chunk) = rx.try_recv() {
                        ai_results.push(chunk);
                    }
                }
            } else if !speech_buf.is_empty() {
                silence_after_speech += 1;
                if silence_after_speech >= 2 {
                    let to_send = std::mem::take(&mut speech_buf);
                    let (tx, mut rx) = mpsc::channel(4);
                    provider.convert_stream(to_send, tx).await.unwrap();
                    while let Ok(chunk) = rx.try_recv() {
                        ai_results.push(chunk);
                    }
                    silence_after_speech = 0;
                }
            }
        }

        assert!(!ai_results.is_empty(), "speaking should produce AI output");

        let total_ai_samples: usize = ai_results.iter().map(|v| v.len()).sum();
        // 500ms speech → at least 300ms (1 dispatch) of AI output
        assert!(
            total_ai_samples >= dispatch_samples,
            "got {} samples, expected >= {}",
            total_ai_samples,
            dispatch_samples
        );

        println!(
            "simulate_speaking_abcd: {} AI chunks, {} total samples ({:.0}ms at 48kHz)",
            ai_results.len(),
            total_ai_samples,
            total_ai_samples as f32 / sample_rate as f32 * 1000.0
        );
    }

    #[tokio::test]
    async fn continuous_speech_dispatches_multiple_chunks() {
        let provider = Arc::new(MockProvider);
        let sample_rate = 48_000u32;
        let vad_samples = (sample_rate as usize * 100) / 1000;
        let dispatch_samples = (sample_rate as usize * 300) / 1000;

        let mut vad = Vad::new(-35.0, 1, 1);
        let mut speech_buf: Vec<f32> = Vec::new();
        let mut dispatch_count = 0;

        // Simulate 2 seconds of continuous speech (20 * 100ms frames)
        for _ in 0..20 {
            let frame = sine_wave(vad_samples, sample_rate, 200.0, 0.8);
            if vad.process(&frame) {
                speech_buf.extend_from_slice(&frame);
                while speech_buf.len() >= dispatch_samples {
                    let to_send: Vec<f32> = speech_buf.drain(..dispatch_samples).collect();
                    let (tx, _rx) = mpsc::channel(4);
                    provider.convert_stream(to_send, tx).await.unwrap();
                    dispatch_count += 1;
                }
            }
        }

        // 2s / 300ms ≈ 6-7 dispatches (minus pre_roll frames)
        assert!(dispatch_count >= 5, "2s speech should dispatch multiple chunks, got {}", dispatch_count);
        println!("continuous_speech: {} dispatches for 2s of speech", dispatch_count);
    }
}
