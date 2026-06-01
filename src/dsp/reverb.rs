/// schroeder-style reverb. four comb filters in parallel followed by two
/// allpass filters. classic, cheap, sounds like a vintage tail.
pub struct Reverb {
    combs: [Comb; 4],
    aps: [AllPass; 2],
    mix: f32,
}

impl Reverb {
    pub fn new(sample_rate: f32) -> Self {
        let sr = sample_rate;
        let combs = [
            Comb::new(sr_ms(sr, 29.7), 0.805),
            Comb::new(sr_ms(sr, 37.1), 0.827),
            Comb::new(sr_ms(sr, 41.1), 0.783),
            Comb::new(sr_ms(sr, 43.7), 0.764),
        ];
        let aps = [
            AllPass::new(sr_ms(sr, 5.0), 0.7),
            AllPass::new(sr_ms(sr, 1.7), 0.7),
        ];
        Self { combs, aps, mix: 0.22 }
    }

    pub fn set_mix(&mut self, mix: f32) {
        self.mix = mix.clamp(0.0, 1.0);
    }

    pub fn process(&mut self, dry: f32) -> f32 {
        if self.mix <= 0.0 {
            return dry;
        }
        let mut wet = 0.0;
        for c in &mut self.combs {
            wet += c.process(dry);
        }
        for ap in &mut self.aps {
            wet = ap.process(wet);
        }
        dry * (1.0 - self.mix) + wet * self.mix
    }
}

fn sr_ms(sr: f32, ms: f32) -> usize {
    ((sr * ms * 0.001) as usize).max(1)
}

struct Comb {
    buffer: Vec<f32>,
    cursor: usize,
    feedback: f32,
}

impl Comb {
    fn new(delay_samples: usize, feedback: f32) -> Self {
        Self {
            buffer: vec![0.0; delay_samples],
            cursor: 0,
            feedback,
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        let out = self.buffer[self.cursor];
        self.buffer[self.cursor] = input + out * self.feedback;
        self.cursor = (self.cursor + 1) % self.buffer.len();
        out
    }
}

struct AllPass {
    buffer: Vec<f32>,
    cursor: usize,
    feedback: f32,
}

impl AllPass {
    fn new(delay_samples: usize, feedback: f32) -> Self {
        Self {
            buffer: vec![0.0; delay_samples],
            cursor: 0,
            feedback,
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        let buffered = self.buffer[self.cursor];
        let out = -input + buffered;
        self.buffer[self.cursor] = input + buffered * self.feedback;
        self.cursor = (self.cursor + 1) % self.buffer.len();
        out
    }
}
