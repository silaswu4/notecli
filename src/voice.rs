use std::sync::Arc;

use crate::dsp::drums::DrumVoice;
use crate::dsp::env::Adsr;
use crate::dsp::filter::Biquad;
use crate::dsp::osc::PhaseOsc;
use crate::dsp::{midi_to_hz, pan_law};
use crate::model::{ChannelId, DrumKind, OscKind, SynthParams};
use crate::sample::SampleData;

/// enum-dispatched voice. cheaper than dyn dispatch in tight audio loops.
pub enum Voice {
    Idle,
    Drum(DrumState),
    Synth(SynthState),
    Sampler(SamplerState),
}

pub struct DrumState {
    pub channel: ChannelId,
    pub voice: DrumVoice,
    pub gain_l: f32,
    pub gain_r: f32,
}

pub struct SynthState {
    pub channel: ChannelId,
    pub osc: PhaseOsc,
    pub kind: OscKind,
    pub env: Adsr,
    pub filter_l: Biquad,
    pub filter_r: Biquad,
    pub pitch: u8,
    pub gain_l: f32,
    pub gain_r: f32,
    pub base_cutoff: f32,
    pub filter_env: f32,
    pub sample_rate: f32,
    /// when triggered by a step (vs midi), the voice auto-releases after this
    /// many samples. 0 means manual release only.
    pub auto_release_in: u32,
    pub released: bool,
}

pub struct SamplerState {
    pub channel: ChannelId,
    pub data: Arc<SampleData>,
    /// fractional sample index (mono frames into the underlying buffer).
    pub pos: f64,
    pub pitch_ratio: f64,
    pub gain_l: f32,
    pub gain_r: f32,
    pub active: bool,
}

impl Voice {
    pub fn is_active(&self) -> bool {
        match self {
            Voice::Idle => false,
            Voice::Drum(d) => d.voice.is_active(),
            Voice::Synth(s) => s.env.is_active(),
            Voice::Sampler(s) => s.active,
        }
    }

    pub fn channel(&self) -> Option<ChannelId> {
        match self {
            Voice::Idle => None,
            Voice::Drum(d) => Some(d.channel),
            Voice::Synth(s) => Some(s.channel),
            Voice::Sampler(s) => Some(s.channel),
        }
    }

    pub fn release(&mut self, pitch: u8) {
        if let Voice::Synth(s) = self {
            if s.pitch == pitch {
                s.env.note_off();
            }
        }
    }

    pub fn render(&mut self, out_l: &mut f32, out_r: &mut f32) {
        match self {
            Voice::Idle => {}
            Voice::Drum(d) => {
                let (l, r) = d.voice.process();
                *out_l += l * d.gain_l;
                *out_r += r * d.gain_r;
            }
            Voice::Synth(s) => {
                if s.auto_release_in > 0 && !s.released {
                    s.auto_release_in -= 1;
                    if s.auto_release_in == 0 {
                        s.env.note_off();
                        s.released = true;
                    }
                }
                let osc_sample = match s.kind {
                    OscKind::Sine => s.osc.sine(),
                    OscKind::Triangle => s.osc.triangle(),
                    OscKind::Saw => s.osc.saw(),
                    OscKind::Square => s.osc.square(),
                    OscKind::Noise => s.osc.noise(),
                };
                let amp = s.env.process();
                let mod_cutoff = (s.base_cutoff + s.filter_env * amp).clamp(40.0, 18000.0);
                s.filter_l.set_lowpass(mod_cutoff, 0.9 + s.filter_env * 0.5, s.sample_rate);
                s.filter_r.set_lowpass(mod_cutoff, 0.9 + s.filter_env * 0.5, s.sample_rate);
                let sample = osc_sample * amp;
                let l = s.filter_l.process(sample);
                let r = s.filter_r.process(sample);
                *out_l += l * s.gain_l;
                *out_r += r * s.gain_r;
            }
            Voice::Sampler(s) => {
                if !s.active {
                    return;
                }
                let frames = s.data.frame_count();
                if frames == 0 {
                    s.active = false;
                    return;
                }
                let idx = s.pos as usize;
                if idx + 1 >= frames {
                    s.active = false;
                    return;
                }
                let frac = (s.pos - idx as f64) as f32;
                let (a_l, a_r) = s.data.read_frame(idx);
                let (b_l, b_r) = s.data.read_frame(idx + 1);
                let l = a_l + (b_l - a_l) * frac;
                let r = a_r + (b_r - a_r) * frac;
                *out_l += l * s.gain_l;
                *out_r += r * s.gain_r;
                s.pos += s.pitch_ratio;
            }
        }
    }

    pub fn make_drum(
        channel: ChannelId,
        kind: DrumKind,
        velocity: f32,
        pan: f32,
        sample_rate: f32,
    ) -> Self {
        let (gl, gr) = pan_law(pan);
        let mut voice = DrumVoice::new(kind, sample_rate);
        voice.trigger(velocity);
        Voice::Drum(DrumState {
            channel,
            voice,
            gain_l: gl,
            gain_r: gr,
        })
    }

    pub fn make_synth(
        channel: ChannelId,
        params: &SynthParams,
        pitch: u8,
        velocity: f32,
        pan: f32,
        sample_rate: f32,
        auto_release_samples: u32,
    ) -> Self {
        let (gl, gr) = pan_law(pan);
        let mut osc = PhaseOsc::new();
        osc.set_freq(midi_to_hz(pitch as f32), sample_rate);
        let mut env = Adsr::new(params.attack, params.decay, params.sustain, params.release, sample_rate);
        env.note_on();
        let base_cutoff = 60.0 * (params.filter_cutoff.clamp(0.0, 1.0) * 7.5).exp2();
        let filter_env = base_cutoff * params.filter_env * 4.0;
        let mut filter_l = Biquad::new();
        let mut filter_r = Biquad::new();
        filter_l.set_lowpass(base_cutoff, 0.9, sample_rate);
        filter_r.set_lowpass(base_cutoff, 0.9, sample_rate);
        Voice::Synth(SynthState {
            channel,
            osc,
            kind: params.osc,
            env,
            filter_l,
            filter_r,
            pitch,
            gain_l: gl * velocity,
            gain_r: gr * velocity,
            base_cutoff,
            filter_env,
            sample_rate,
            auto_release_in: auto_release_samples,
            released: false,
        })
    }

    pub fn make_sampler(
        channel: ChannelId,
        data: Arc<SampleData>,
        pitch_semitones: i8,
        velocity: f32,
        pan: f32,
        sample_rate: f32,
    ) -> Self {
        let (gl, gr) = pan_law(pan);
        let ratio_sr = data.sample_rate as f64 / sample_rate as f64;
        let pitch_ratio = ratio_sr * 2f64.powf(pitch_semitones as f64 / 12.0);
        Voice::Sampler(SamplerState {
            channel,
            data,
            pos: 0.0,
            pitch_ratio,
            gain_l: gl * velocity,
            gain_r: gr * velocity,
            active: true,
        })
    }
}
