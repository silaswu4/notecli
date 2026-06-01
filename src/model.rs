use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::sample::SampleData;

pub const STEPS_PER_BAR: u32 = 16;
pub const PPQ: u32 = 96;
pub const TICKS_PER_STEP: u32 = PPQ / 4;

pub type ChannelId = u16;
pub type PatternId = u16;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub bpm: f32,
    pub master_volume: f32,
    pub channels: Vec<Channel>,
    pub patterns: Vec<Pattern>,
    pub playlist: Vec<PatternId>,
    pub active_pattern: PatternId,
}

impl Default for Project {
    fn default() -> Self {
        let channels = vec![
            Channel::new_drum("kick", DrumKind::Kick),
            Channel::new_drum("snare", DrumKind::Snare),
            Channel::new_drum("hat", DrumKind::Hat),
            Channel::new_drum("clap", DrumKind::Clap),
            Channel::new_drum("tom", DrumKind::Tom),
            Channel::new_synth("bass", OscKind::Saw),
            Channel::new_synth("lead", OscKind::Square),
        ];
        let ch_count = channels.len();
        let mut pattern = Pattern::empty("01", 16);
        for i in 0..ch_count {
            pattern.tracks.insert(
                i as ChannelId,
                PatternTrack::with_steps(16),
            );
        }
        if ch_count > 0 {
            let track = pattern.tracks.get_mut(&0).unwrap();
            track.steps[0].active = true;
            track.steps[4].active = true;
            track.steps[8].active = true;
            track.steps[12].active = true;
        }
        if ch_count > 1 {
            let track = pattern.tracks.get_mut(&1).unwrap();
            track.steps[4].active = true;
            track.steps[12].active = true;
        }
        if ch_count > 2 {
            let track = pattern.tracks.get_mut(&2).unwrap();
            for i in (2..16).step_by(2) {
                track.steps[i].active = true;
            }
        }
        Self {
            name: "untitled".into(),
            bpm: 120.0,
            master_volume: 0.8,
            channels,
            patterns: vec![pattern, Pattern::empty("02", 16), Pattern::empty("03", 16), Pattern::empty("04", 16)],
            playlist: vec![0],
            active_pattern: 0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Channel {
    pub name: String,
    pub kind: ChannelKind,
    pub volume: f32,
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
}

impl Channel {
    pub fn new_drum(name: &str, kind: DrumKind) -> Self {
        Self {
            name: name.into(),
            kind: ChannelKind::DrumSynth(kind),
            volume: 0.8,
            pan: 0.0,
            mute: false,
            solo: false,
        }
    }

    pub fn new_synth(name: &str, osc: OscKind) -> Self {
        Self {
            name: name.into(),
            kind: ChannelKind::Synth(SynthParams {
                osc,
                attack: 0.005,
                decay: 0.15,
                sustain: 0.6,
                release: 0.25,
                filter_cutoff: 0.55,
                filter_resonance: 0.2,
                filter_env: 0.3,
            }),
            volume: 0.7,
            pan: 0.0,
            mute: false,
            solo: false,
        }
    }

    pub fn new_sampler(name: &str, path: PathBuf, data: Option<Arc<SampleData>>) -> Self {
        Self {
            name: name.into(),
            kind: ChannelKind::Sampler(SamplerParams {
                path,
                pitch_semitones: 0,
                data,
            }),
            volume: 0.8,
            pan: 0.0,
            mute: false,
            solo: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ChannelKind {
    DrumSynth(DrumKind),
    Synth(SynthParams),
    Sampler(SamplerParams),
    /// route step events to the virtual midi port instead of an internal voice.
    /// the user's daw (running serum / vital / any vst) receives these notes.
    MidiOut(MidiOutParams),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MidiOutParams {
    /// 1..=16. usually you set this on the daw side too so the routing matches.
    pub channel: u8,
    /// base pitch each step plays. defaults to c4 (60).
    pub pitch: u8,
    /// name of a standalone .app to launch alongside tek. typical values:
    /// "Vital", "Serum", "Massive X". omit if you'd rather launch yourself.
    /// looked up via macos `open -a`, so any installed app name works.
    #[serde(default)]
    pub launch_app: Option<String>,
}

impl Default for MidiOutParams {
    fn default() -> Self {
        Self {
            channel: 1,
            pitch: 60,
            launch_app: Some("Vital".into()),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum DrumKind {
    Kick,
    Snare,
    Hat,
    Clap,
    Tom,
}

impl DrumKind {
    pub fn label(&self) -> &'static str {
        match self {
            DrumKind::Kick => "kick",
            DrumKind::Snare => "snare",
            DrumKind::Hat => "hat",
            DrumKind::Clap => "clap",
            DrumKind::Tom => "tom",
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum OscKind {
    Sine,
    Triangle,
    Saw,
    Square,
    Noise,
}

impl OscKind {
    pub fn label(&self) -> &'static str {
        match self {
            OscKind::Sine => "sine",
            OscKind::Triangle => "tri",
            OscKind::Saw => "saw",
            OscKind::Square => "sqr",
            OscKind::Noise => "noise",
        }
    }

    pub fn next(&self) -> OscKind {
        match self {
            OscKind::Sine => OscKind::Triangle,
            OscKind::Triangle => OscKind::Saw,
            OscKind::Saw => OscKind::Square,
            OscKind::Square => OscKind::Noise,
            OscKind::Noise => OscKind::Sine,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SynthParams {
    pub osc: OscKind,
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    pub filter_cutoff: f32,
    pub filter_resonance: f32,
    pub filter_env: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SamplerParams {
    pub path: PathBuf,
    pub pitch_semitones: i8,
    #[serde(skip)]
    pub data: Option<Arc<SampleData>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Pattern {
    pub name: String,
    pub length: u32,
    pub tracks: HashMap<ChannelId, PatternTrack>,
}

impl Pattern {
    pub fn empty(name: &str, length: u32) -> Self {
        Self {
            name: name.into(),
            length,
            tracks: HashMap::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PatternTrack {
    pub steps: Vec<Step>,
    pub notes: Vec<Note>,
}

impl PatternTrack {
    pub fn with_steps(length: u32) -> Self {
        Self {
            steps: (0..length).map(|_| Step::default()).collect(),
            notes: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Step {
    pub active: bool,
    pub velocity: f32,
    pub pitch_offset: i8,
}

impl Step {
    pub fn toggle(&mut self) {
        self.active = !self.active;
        if self.active && self.velocity == 0.0 {
            self.velocity = 1.0;
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Note {
    pub pitch: u8,
    pub start: u32,
    pub length: u32,
    pub velocity: f32,
}
