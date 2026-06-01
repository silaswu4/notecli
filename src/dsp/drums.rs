use super::osc::PhaseOsc;
use crate::model::DrumKind;

/// procedural drum voices. each one is a tiny set of oscillators + envelopes
/// hand-tuned to land in a recognisable ballpark.
#[derive(Clone, Copy)]
pub struct DrumVoice {
    kind: DrumKind,
    osc_a: PhaseOsc,
    osc_b: PhaseOsc,
    noise: PhaseOsc,
    amp: f32,
    amp_decay: f32,
    pitch_env: f32,
    pitch_decay: f32,
    pitch_start_hz: f32,
    pitch_end_hz: f32,
    noise_amount: f32,
    noise_decay: f32,
    noise_amp: f32,
    age: f32,
    sample_rate: f32,
    active: bool,
    velocity: f32,
}

impl DrumVoice {
    pub fn new(kind: DrumKind, sample_rate: f32) -> Self {
        Self {
            kind,
            osc_a: PhaseOsc::new(),
            osc_b: PhaseOsc::new(),
            noise: PhaseOsc::new(),
            amp: 0.0,
            amp_decay: 0.2,
            pitch_env: 0.0,
            pitch_decay: 0.05,
            pitch_start_hz: 100.0,
            pitch_end_hz: 50.0,
            noise_amount: 0.0,
            noise_decay: 0.1,
            noise_amp: 0.0,
            age: 0.0,
            sample_rate,
            active: false,
            velocity: 1.0,
        }
    }

    pub fn trigger(&mut self, velocity: f32) {
        self.amp = 1.0;
        self.pitch_env = 1.0;
        self.noise_amp = 1.0;
        self.age = 0.0;
        self.active = true;
        self.velocity = velocity.clamp(0.0, 1.0);
        self.osc_a.phase = 0.0;
        self.osc_b.phase = 0.0;

        match self.kind {
            DrumKind::Kick => {
                self.amp_decay = 0.35;
                self.pitch_start_hz = 180.0;
                self.pitch_end_hz = 42.0;
                self.pitch_decay = 0.07;
                self.noise_amount = 0.08;
                self.noise_decay = 0.012;
            }
            DrumKind::Snare => {
                self.amp_decay = 0.18;
                self.pitch_start_hz = 230.0;
                self.pitch_end_hz = 180.0;
                self.pitch_decay = 0.04;
                self.noise_amount = 0.85;
                self.noise_decay = 0.16;
            }
            DrumKind::Hat => {
                self.amp_decay = 0.05;
                self.pitch_start_hz = 8200.0;
                self.pitch_end_hz = 8200.0;
                self.pitch_decay = 0.001;
                self.noise_amount = 1.0;
                self.noise_decay = 0.045;
            }
            DrumKind::Clap => {
                self.amp_decay = 0.20;
                self.pitch_start_hz = 1300.0;
                self.pitch_end_hz = 1300.0;
                self.pitch_decay = 0.001;
                self.noise_amount = 1.0;
                self.noise_decay = 0.18;
            }
            DrumKind::Tom => {
                self.amp_decay = 0.30;
                self.pitch_start_hz = 220.0;
                self.pitch_end_hz = 90.0;
                self.pitch_decay = 0.13;
                self.noise_amount = 0.12;
                self.noise_decay = 0.04;
            }
        }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn process(&mut self) -> (f32, f32) {
        if !self.active {
            return (0.0, 0.0);
        }
        let dt = 1.0 / self.sample_rate;
        self.age += dt;

        // amp envelope (exponential decay)
        self.amp -= self.amp / (self.amp_decay * self.sample_rate);
        // pitch envelope
        self.pitch_env -= self.pitch_env / (self.pitch_decay * self.sample_rate);
        // noise envelope
        self.noise_amp -= self.noise_amp / (self.noise_decay * self.sample_rate);

        let pitch_hz = self.pitch_end_hz
            + (self.pitch_start_hz - self.pitch_end_hz) * self.pitch_env;
        self.osc_a.set_freq(pitch_hz, self.sample_rate);

        let tone = match self.kind {
            DrumKind::Hat => self.noise.noise(),
            DrumKind::Clap => self.noise.noise(),
            DrumKind::Snare => {
                self.osc_a.triangle() * 0.6 + self.noise.noise() * self.noise_amount * self.noise_amp
            }
            DrumKind::Kick => {
                self.osc_a.sine() + self.noise.noise() * self.noise_amount * self.noise_amp
            }
            DrumKind::Tom => {
                self.osc_a.sine() + self.noise.noise() * self.noise_amount * self.noise_amp
            }
        };

        // basic clap "burst" simulation: amp pulses at the start
        let burst = if matches!(self.kind, DrumKind::Clap) {
            let t = self.age;
            (((t * 110.0).sin() * 0.5 + 0.5).powf(2.0) * (-t * 18.0).exp() + 0.3 * (-t * 4.5).exp())
                .min(1.5)
        } else {
            1.0
        };

        let level = self.amp * burst * self.velocity;
        if level < 0.0005 && self.age > 0.04 {
            self.active = false;
        }
        let s = tone * level;
        (s, s)
    }
}
