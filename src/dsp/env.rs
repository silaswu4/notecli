/// adsr envelope. attack / decay / sustain / release in seconds and amplitude.
/// process() should be called once per sample frame. note_on() starts the
/// attack stage, note_off() jumps to release.
#[derive(Clone, Copy, Debug)]
pub struct Adsr {
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    pub sample_rate: f32,
    stage: Stage,
    level: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

impl Adsr {
    pub fn new(a: f32, d: f32, s: f32, r: f32, sample_rate: f32) -> Self {
        Self {
            attack: a.max(0.001),
            decay: d.max(0.001),
            sustain: s.clamp(0.0, 1.0),
            release: r.max(0.001),
            sample_rate,
            stage: Stage::Idle,
            level: 0.0,
        }
    }

    pub fn note_on(&mut self) {
        self.stage = Stage::Attack;
    }

    pub fn note_off(&mut self) {
        if self.stage != Stage::Idle {
            self.stage = Stage::Release;
        }
    }

    pub fn is_active(&self) -> bool {
        self.stage != Stage::Idle
    }

    pub fn level(&self) -> f32 {
        self.level
    }

    pub fn process(&mut self) -> f32 {
        let sr = self.sample_rate;
        match self.stage {
            Stage::Idle => {
                self.level = 0.0;
            }
            Stage::Attack => {
                let step = 1.0 / (self.attack * sr);
                self.level += step;
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => {
                let step = (1.0 - self.sustain) / (self.decay * sr);
                self.level -= step;
                if self.level <= self.sustain {
                    self.level = self.sustain;
                    self.stage = Stage::Sustain;
                }
            }
            Stage::Sustain => {
                self.level = self.sustain;
            }
            Stage::Release => {
                // exponential decay so the tail sounds natural regardless of
                // what level we were sitting at when note_off hit.
                let tau = self.release.max(0.001) * sr;
                let factor = (-6.9_f32 / tau).exp();
                self.level *= factor;
                if self.level < 0.0005 {
                    self.level = 0.0;
                    self.stage = Stage::Idle;
                }
            }
        }
        self.level
    }

    /// quick one-shot envelope for drums: attack to peak, then exponential
    /// decay over the given seconds. is_active stays true until the decay
    /// drops below an audible floor.
    pub fn one_shot(&mut self, decay: f32) {
        self.attack = 0.001;
        self.decay = decay.max(0.005);
        self.sustain = 0.0;
        self.release = 0.0;
        self.stage = Stage::Attack;
        self.level = 0.0;
    }
}
