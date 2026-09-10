# voice-changer — Project Memo

AI Voice Changer for Google Meet (macOS). Tauri 2 + React 19 + TS + Rust. Bun package manager. Real-time-ish: mic → ElevenLabs STS → BlackHole virtual mic.

## Architecture

```
bypass: mic → out_buf langsung (zero mixer)
AI:     mic → VAD utterance buffer (≤1.5s rolling) → 1 STS request → edge-fade → out_buf → BlackHole
```

Key files:
- `src-tauri/src/audio/engine.rs` — pipeline core. 3 async tasks: processor (sequential STS requests), Task 1 (AI result → out_buf, edge fades, echo-gate deadline), Task 3 (capture → 100ms VAD frames → accumulate → dispatch). Wall-clock drift correction (±ms sleep adjust) vs sample clock.
- `src-tauri/src/audio/vad.rs` — energy VAD. Engine uses -40dB, pre_roll 2 (200ms), post_roll 6 (600ms).
- `src-tauri/src/audio/capture.rs` — cpal input, prefer 48kHz, rejects virtual devices as default.
- `src-tauri/src/audio/virtual_mic.rs` — auto-detect BlackHole/Loopback output.
- `src-tauri/src/provider/elevenlabs.rs` — STS `eleven_multilingual_sts_v2`, voice_settings 0.5/0.5.
- `src-tauri/src/db.rs` — SQLite credentials (API key, voice id).
- Tests: `src-tauri/src/tests.rs` (18) + engine unit tests (2). Run: `cargo test`.

## Critical gotchas (learned the hard way)

1. **`output_format` is a URL QUERY PARAM**, not a form field. As form field → ignored → EL returns default MP3 → decoded as raw PCM = white noise. Current: `?output_format=pcm_44100` in URL.
2. **`pcm_44100` needs Pro tier** → 403 on lower tiers. Provider has `pcm_denied` AtomicBool → auto-fallback `mp3_44100_128`, remembered for session. MP3 decoded properly via minimp3 (container sniffing: RIFF→WAV, ID3/sync→MP3, else raw PCM).
3. **Never add level-based release to the echo gate.** Tried "gate release when mic loud" → speaker echo at -26dB read as speech → AI voice re-entered mic → looping output AGAIN. Gate = wall-clock deadline only (`ai_playing_until`, +200ms margin). Trade-off: live speech not recorded while AI audio plays (recorder design).
4. **No timer mixer.** 10ms tokio-timer mixer vs realtime VirtualMic pull = sample deficit = periodic underrun crackle. Tasks write directly to out_buf.
5. **VAD must be fed 100ms frames**, not 10ms loop iterations — else flicker → 200ms fragmented chunks → choppy/robotic + queue drops.
6. **Chunk 1.5s is the sweet spot** (stable + clear). 3.5s = too slow + trailing silence shipped to AI (hallucinated continuation). 2.5s attempt reverted.
7. Response byte count is diagnostic: 1500ms chunk → 25122 bytes ≈ MP3@128k; correct decode = ~66k samples per 1.5s. If ~12k samples → decode bug.
8. Monitor 🔊 through speakers + open mic = physical feedback loop possible — not a code bug. Headphones recommended.

## Known-good state (main)

User confirmed "jernih" on main: edge fade 5ms per chunk (no crossfade holdback — it duplicated tails + 50ms gaps), no chunk cancellation, direct out_buf writes, drift-corrected capture loop. Pushed as `dc84f19`.

## Branches

- `main` — stable, jernih, reverted to known-good after smart-gate experiment failed.
- `feat/websocket-sts` — `LOW_LATENCY_PCM16_INPUT` (raw 16kHz S16LE input, `file_format=pcm_s16le_16`, documented lower latency). WAV 48k fallback via flag. Research note: **ElevenLabs has NO public WS for speech-to-speech** (stream-input WS = TTS-only). When STS WS ships, swap point = provider's `convert_stream`; engine interface (`VoiceProvider`) unchanged. Commit `50b6fd9`.

## Open ideas

- Echo cancellation (speexdsp AEC) on capture to kill feedback physically.
- UI: latency/level meters, recorder-state indicator.
- WS STS when ElevenLabs releases it.
