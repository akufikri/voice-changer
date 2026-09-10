// Simple energy-based VAD. Replace with webrtc-vad or silero for production.
pub struct Vad {
    threshold: f32,
    pre_roll: usize,
    post_roll: usize,
    silence_counter: usize,
    speech_counter: usize,
    is_speech: bool,
}

impl Vad {
    pub fn new(threshold: f32, pre_roll: usize, post_roll: usize) -> Self {
        Self {
            threshold,
            pre_roll,
            post_roll,
            silence_counter: 0,
            speech_counter: 0,
            is_speech: false,
        }
    }

    pub fn process(&mut self, frame: &[f32]) -> bool {
        let energy: f32 = frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32;
        let energy_db = 10.0 * energy.log10().max(-60.0);

        if energy_db > self.threshold {
            self.silence_counter = 0;
            self.speech_counter += 1;
            if self.speech_counter >= self.pre_roll {
                self.is_speech = true;
            }
        } else {
            self.speech_counter = 0;
            self.silence_counter += 1;
            if self.silence_counter >= self.post_roll {
                self.is_speech = false;
            }
        }

        self.is_speech
    }
}
