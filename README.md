# AI Voice Changer for Google Meet

Real-time AI voice changer for macOS that transforms microphone audio using ElevenLabs Voice Conversion and exposes the processed audio as a virtual microphone for Google Meet.

## Overview

The application sits between the user's physical microphone and Google Meet:

```text
Physical Microphone
        │
        ▼
   Audio Capture
        │
        ▼
 Audio Processing
        │
        ▼
 Voice Activity Detection
        │
        ▼
 ElevenLabs Voice Conversion
        │
        ▼
 Audio Buffer
        │
        ▼
 Virtual Microphone
        │
        ▼
    Google Meet
```

Google Meet only sees the application's virtual microphone as a normal audio input device.

## Goals

- Real-time voice transformation.
- Native macOS experience.
- ElevenLabs integration.
- Low perceived latency.
- Virtual microphone output.
- Reliable operation during meetings.
- Automatic fallback to original microphone audio.
- Simple desktop UI.
- Secure API key handling.

## Primary Target

Platform:

- macOS
- Apple Silicon first
- Intel compatibility where practical

Primary meeting platform:

- Google Meet

Secondary platforms may be supported later:

- Zoom
- Discord
- Microsoft Teams
- OBS
- Browser-based applications

## Core Technologies

- Tauri
- Rust
- React
- TypeScript
- ElevenLabs API
- macOS CoreAudio
- Virtual audio device

## Development Philosophy

The project should prioritize:

1. Audio reliability.
2. Low latency.
3. Graceful failure.
4. Clear separation between UI and audio engine.
5. Provider abstraction.
6. Security.
7. Testability.

Do not sacrifice audio stability for UI features.

## MVP

The MVP must support:

- Microphone selection.
- ElevenLabs voice selection.
- Start/stop voice conversion.
- Original microphone monitoring.
- Processed microphone output.
- Virtual microphone.
- Google Meet microphone selection.
- Latency monitoring.
- Connection status.
- API error handling.
- Original-voice fallback.

## Non-Goals for MVP

Do not implement initially:

- Voice cloning UI.
- Multi-user accounts.
- Cloud backend.
- Mobile applications.
- Browser extension.
- Advanced audio effects.
- Social features.
- Recording system.
- Streaming platform integrations.

## Success Criteria

A user should be able to:

1. Launch the application.
2. Select their physical microphone.
3. Select an ElevenLabs voice.
4. Enable voice conversion.
5. Select `VoiceChanger Virtual Microphone` inside Google Meet.
6. Speak normally.
7. Have meeting participants hear the transformed voice.
8. Disable conversion and immediately return to the original voice.

## Important Constraint

The project must not assume that a generic HTTP request/response API is suitable for low-latency voice conversion.

The ElevenLabs integration must be validated against the currently available real-time voice conversion capability before the final audio architecture is locked.