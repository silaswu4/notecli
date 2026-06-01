use std::f32::consts::TAU;

/// minimal band-limited-ish polyblep saw / square. plain phase math for sine
/// and triangle. noise is just an xorshift.
#[derive(Clone, Copy)]
pub struct PhaseOsc {
    pub phase: f32,
    pub inc: f32,
    rng: u32,
}

impl PhaseOsc {
    pub fn new() -> Self {
        Self {
            phase: 0.0,
            inc: 0.0,
            rng: 0xdeadbeef,
        }
    }

    pub fn set_freq(&mut self, hz: f32, sample_rate: f32) {
        self.inc = hz / sample_rate;
    }

    fn advance(&mut self) {
        self.phase += self.inc;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
    }

    pub fn sine(&mut self) -> f32 {
        let s = (TAU * self.phase).sin();
        self.advance();
        s
    }

    pub fn triangle(&mut self) -> f32 {
        let p = self.phase;
        let s = if p < 0.5 {
            4.0 * p - 1.0
        } else {
            3.0 - 4.0 * p
        };
        self.advance();
        s
    }

    pub fn saw(&mut self) -> f32 {
        let p = self.phase;
        let s = 2.0 * p - 1.0;
        let blep = poly_blep(p, self.inc);
        self.advance();
        s - blep
    }

    pub fn square(&mut self) -> f32 {
        let p = self.phase;
        let s = if p < 0.5 { 1.0 } else { -1.0 };
        let blep_a = poly_blep(p, self.inc);
        let p2 = (p + 0.5).fract();
        let blep_b = poly_blep(p2, self.inc);
        self.advance();
        s + blep_a - blep_b
    }

    pub fn noise(&mut self) -> f32 {
        // xorshift32
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

fn poly_blep(t: f32, dt: f32) -> f32 {
    if dt <= 0.0 {
        return 0.0;
    }
    if t < dt {
        let t = t / dt;
        return t + t - t * t - 1.0;
    }
    if t > 1.0 - dt {
        let t = (t - 1.0) / dt;
        return t * t + t + t + 1.0;
    }
    0.0
}
