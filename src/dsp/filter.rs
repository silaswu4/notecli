use std::f32::consts::TAU;

/// rbj biquad lowpass. fixed at lowpass for v1; can extend to highpass /
/// bandpass / notch by recomputing the coefficients.
#[derive(Clone, Copy, Debug)]
pub struct Biquad {
    a1: f32,
    a2: f32,
    b0: f32,
    b1: f32,
    b2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    pub fn new() -> Self {
        Self {
            a1: 0.0,
            a2: 0.0,
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    /// rbj cookbook lowpass. cutoff in hz, q is the resonance.
    pub fn set_lowpass(&mut self, cutoff: f32, q: f32, sample_rate: f32) {
        let f = cutoff.clamp(20.0, sample_rate * 0.45);
        let q = q.max(0.1);
        let omega = TAU * f / sample_rate;
        let s = omega.sin();
        let c = omega.cos();
        let alpha = s / (2.0 * q);
        let b0 = (1.0 - c) * 0.5;
        let b1 = 1.0 - c;
        let b2 = (1.0 - c) * 0.5;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * c;
        let a2 = 1.0 - alpha;
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    pub fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }

    pub fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}
