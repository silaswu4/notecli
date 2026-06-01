pub mod osc;
pub mod env;
pub mod filter;
pub mod drums;
pub mod reverb;

/// 12-tet midi pitch to hz. midi 69 is a4 = 440 hz.
pub fn midi_to_hz(pitch: f32) -> f32 {
    440.0 * 2.0_f32.powf((pitch - 69.0) / 12.0)
}

/// equal-power pan from -1 (left) to +1 (right). returns (gain_l, gain_r).
pub fn pan_law(pan: f32) -> (f32, f32) {
    let p = (pan.clamp(-1.0, 1.0) + 1.0) * 0.5;
    let theta = p * std::f32::consts::FRAC_PI_2;
    (theta.cos(), theta.sin())
}

/// fast tanh approximation for soft clip / saturation.
pub fn soft_clip(x: f32) -> f32 {
    let x2 = x * x;
    x * (27.0 + x2) / (27.0 + 9.0 * x2)
}
