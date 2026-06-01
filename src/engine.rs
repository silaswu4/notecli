use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use arc_swap::ArcSwap;
use ringbuf::traits::{Consumer, Producer};
use ringbuf::{HeapCons, HeapProd};

use crate::command::Command;
use crate::dsp::reverb::Reverb;
use crate::dsp::soft_clip;
use crate::midi_out::MidiOutMessage;
use crate::model::{ChannelId, ChannelKind, Project, TICKS_PER_STEP};
use crate::transport::Transport;
use crate::voice::Voice;

const MAX_VOICES: usize = 64;

pub struct Engine {
    pub project: Arc<ArcSwap<Project>>,
    pub transport: Arc<Transport>,
    pub cmd_rx: HeapCons<Command>,
    pub sample_rate: f32,
    voices: Vec<Voice>,
    reverb: Reverb,
    reverb_send: f32,
    midi_tx: Option<HeapProd<MidiOutMessage>>,
    /// for midi-out channels we remember the last note we sent so we can
    /// release it cleanly before sending the next one.
    last_midi: HashMap<ChannelId, (u8, u8)>,
    /// most recent pattern-relative tick we processed. if a new tick
    /// comes in lower than this, the playhead just wrapped, and that's
    /// when we cut every ringing voice so loops never stack on top of
    /// each other.
    last_pat_tick: u64,
}

impl Engine {
    pub fn new(
        project: Arc<ArcSwap<Project>>,
        transport: Arc<Transport>,
        cmd_rx: HeapCons<Command>,
        sample_rate: f32,
        midi_tx: Option<HeapProd<MidiOutMessage>>,
    ) -> Self {
        let voices = (0..MAX_VOICES).map(|_| Voice::Idle).collect();
        Self {
            project,
            transport,
            cmd_rx,
            sample_rate,
            voices,
            reverb: Reverb::new(sample_rate),
            reverb_send: 0.18,
            midi_tx,
            last_midi: HashMap::new(),
            last_pat_tick: 0,
        }
    }

    /// silence every voice and send note-off for every midi note we left
    /// hanging. used when the pattern wraps so the next loop iteration
    /// starts from a clean slate.
    fn cut_all(&mut self) {
        for v in self.voices.iter_mut() {
            *v = Voice::Idle;
        }
        let entries: Vec<(ChannelId, (u8, u8))> = self.last_midi.drain().collect();
        for (_, (ch, pitch)) in entries {
            self.send_midi(MidiOutMessage::NoteOff { channel: ch, pitch });
        }
    }

    fn send_midi(&mut self, msg: MidiOutMessage) {
        if let Some(tx) = &mut self.midi_tx {
            let _ = tx.try_push(msg);
        }
    }

    /// stereo interleaved f32. expects `buffer.len()` to be a multiple of 2.
    pub fn render(&mut self, buffer: &mut [f32]) {
        let frames = buffer.len() / 2;

        // drain incoming commands so all triggers register before the first frame.
        while let Some(cmd) = self.cmd_rx.try_pop() {
            self.handle_command(cmd);
        }

        let project = self.project.load_full();
        let bpm = self.transport.bpm().max(20.0);
        let samples_per_tick = (self.sample_rate as f64 * 60.0) / (bpm as f64 * 96.0);

        let active_pattern_ix = project.active_pattern as usize;
        let pattern = project.patterns.get(active_pattern_ix);
        let pattern_length_ticks: u64 = pattern
            .map(|p| p.length as u64 * TICKS_PER_STEP as u64)
            .unwrap_or(16 * TICKS_PER_STEP as u64);

        let solo_any = project.channels.iter().any(|c| c.solo);
        let master = project.master_volume;
        let playing = self.transport.is_playing();
        let mut clock = self.transport.sample_position();

        for frame in 0..frames {
            let mut tick_event = false;
            let mut step_index: usize = 0;

            if playing {
                if let Some(p) = pattern {
                    let cur_tick = (clock as f64 / samples_per_tick) as u64;
                    let prev_tick = if clock == 0 {
                        u64::MAX
                    } else {
                        ((clock - 1) as f64 / samples_per_tick) as u64
                    };
                    if cur_tick != prev_tick {
                        let pat_tick = cur_tick % pattern_length_ticks;
                        if pat_tick % TICKS_PER_STEP as u64 == 0 {
                            tick_event = true;
                            step_index = (pat_tick / TICKS_PER_STEP as u64) as usize;
                            // if the pattern wrapped back to an earlier
                            // position, kill every voice + midi note so the
                            // new loop iteration starts clean.
                            if pat_tick < self.last_pat_tick {
                                self.cut_all();
                            }
                            self.last_pat_tick = pat_tick;
                        }
                    }
                    if tick_event {
                        for (ch_id, track) in &p.tracks {
                            let ch_index = *ch_id as usize;
                            let Some(channel) = project.channels.get(ch_index) else { continue };
                            if channel.mute || (solo_any && !channel.solo) {
                                continue;
                            }
                            let Some(step) = track.steps.get(step_index) else { continue };
                            if !step.active {
                                continue;
                            }
                            let velocity = step.velocity.max(0.1);
                            self.fire_step(*ch_id, channel, velocity, step.pitch_offset, bpm);
                        }
                    }
                }
                clock += 1;
            }

            // render all voices into a stereo accumulator for this frame
            let mut l = 0.0_f32;
            let mut r = 0.0_f32;
            for voice in self.voices.iter_mut() {
                voice.render(&mut l, &mut r);
            }
            let send = (l + r) * 0.5 * self.reverb_send;
            let wet = self.reverb.process(send);
            l += wet;
            r += wet;

            let l_out = soft_clip(l * master);
            let r_out = soft_clip(r * master);

            buffer[frame * 2] = l_out;
            buffer[frame * 2 + 1] = r_out;
        }

        if playing {
            self.transport.sample_clock.store(clock, Ordering::Relaxed);
        }

        let active = self.voices.iter().filter(|v| v.is_active()).count() as u32;
        self.transport.voice_count.store(active, Ordering::Relaxed);
    }

    fn fire_step(
        &mut self,
        channel_id: u16,
        channel: &crate::model::Channel,
        velocity: f32,
        pitch_offset: i8,
        bpm: f32,
    ) {
        let v = velocity * channel.volume;
        match &channel.kind {
            ChannelKind::DrumSynth(kind) => {
                self.trigger_drum(channel_id, *kind, v, channel.pan);
            }
            ChannelKind::Synth(params) => {
                let pitch = (60i32 + pitch_offset as i32).clamp(0, 127) as u8;
                let step_samples = ((self.sample_rate * 60.0) / (bpm * 4.0)) as u32;
                self.trigger_synth(channel_id, params, pitch, v, channel.pan, step_samples);
            }
            ChannelKind::Sampler(s) => {
                if let Some(data) = &s.data {
                    self.trigger_sampler(
                        channel_id,
                        data.clone(),
                        s.pitch_semitones + pitch_offset,
                        v,
                        channel.pan,
                    );
                }
            }
            ChannelKind::MidiOut(params) => {
                let pitch = (params.pitch as i32 + pitch_offset as i32).clamp(0, 127) as u8;
                let vel = ((v * 127.0).clamp(1.0, 127.0)) as u8;
                let midi_ch = params.channel;
                // release any prior note we sent on this channel so the next
                // hit doesn't stack on top of an unreleased note.
                if let Some(&(prev_ch, prev_pitch)) = self.last_midi.get(&channel_id) {
                    self.send_midi(MidiOutMessage::NoteOff {
                        channel: prev_ch,
                        pitch: prev_pitch,
                    });
                }
                self.send_midi(MidiOutMessage::NoteOn {
                    channel: midi_ch,
                    pitch,
                    velocity: vel,
                });
                self.last_midi.insert(channel_id, (midi_ch, pitch));
            }
        }
    }

    fn handle_command(&mut self, cmd: Command) {
        match cmd {
            Command::PlayToggle => self.transport.toggle_playing(),
            Command::Stop => {
                self.transport.stop();
                // hush every midi note we may have left ringing.
                let entries: Vec<(ChannelId, (u8, u8))> = self.last_midi.drain().collect();
                for (_, (ch, pitch)) in entries {
                    self.send_midi(MidiOutMessage::NoteOff { channel: ch, pitch });
                }
            }
            Command::Rewind => self.transport.sample_clock.store(0, Ordering::Relaxed),
            Command::Trigger { channel, pitch, velocity } => {
                let project = self.project.load_full();
                let Some(ch) = project.channels.get(channel as usize) else { return };
                let v = velocity * ch.volume;
                match &ch.kind {
                    ChannelKind::DrumSynth(kind) => self.trigger_drum(channel, *kind, v, ch.pan),
                    ChannelKind::Synth(params) => {
                        // midi-triggered notes get no auto-release; a matching
                        // Release command will fire env.note_off().
                        self.trigger_synth(channel, params, pitch, v, ch.pan, 0);
                    }
                    ChannelKind::Sampler(s) => {
                        if let Some(data) = &s.data {
                            self.trigger_sampler(channel, data.clone(), s.pitch_semitones, v, ch.pan);
                        }
                    }
                    ChannelKind::MidiOut(params) => {
                        let vel = ((v * 127.0).clamp(1.0, 127.0)) as u8;
                        self.send_midi(MidiOutMessage::NoteOn {
                            channel: params.channel,
                            pitch,
                            velocity: vel,
                        });
                        self.last_midi.insert(channel, (params.channel, pitch));
                    }
                }
            }
            Command::Release { channel, pitch } => {
                for v in self.voices.iter_mut() {
                    if v.channel() == Some(channel) {
                        v.release(pitch);
                    }
                }
                // also release any midi-out note that matches.
                if let Some(&(midi_ch, p)) = self.last_midi.get(&channel) {
                    if p == pitch {
                        self.send_midi(MidiOutMessage::NoteOff {
                            channel: midi_ch,
                            pitch,
                        });
                        self.last_midi.remove(&channel);
                    }
                }
            }
            Command::AuditionSample { .. } => {
                // reserved for browser audition once a sample-list is in.
            }
        }
    }

    fn allocate_voice(&mut self) -> &mut Voice {
        if let Some(idx) = self.voices.iter().position(|v| matches!(v, Voice::Idle)) {
            return &mut self.voices[idx];
        }
        // steal the first inactive
        if let Some(idx) = self.voices.iter().position(|v| !v.is_active()) {
            return &mut self.voices[idx];
        }
        // steal voice 0 as last resort
        &mut self.voices[0]
    }

    fn trigger_drum(&mut self, channel: u16, kind: crate::model::DrumKind, velocity: f32, pan: f32) {
        let sr = self.sample_rate;
        let slot = self.allocate_voice();
        *slot = Voice::make_drum(channel, kind, velocity, pan, sr);
    }

    fn trigger_synth(
        &mut self,
        channel: u16,
        params: &crate::model::SynthParams,
        pitch: u8,
        velocity: f32,
        pan: f32,
        auto_release_samples: u32,
    ) {
        let sr = self.sample_rate;
        let slot = self.allocate_voice();
        *slot = Voice::make_synth(channel, params, pitch, velocity, pan, sr, auto_release_samples);
    }

    fn trigger_sampler(
        &mut self,
        channel: u16,
        data: Arc<crate::sample::SampleData>,
        pitch_semitones: i8,
        velocity: f32,
        pan: f32,
    ) {
        let sr = self.sample_rate;
        let slot = self.allocate_voice();
        *slot = Voice::make_sampler(channel, data, pitch_semitones, velocity, pan, sr);
    }
}

/// helper to compute the current step index for the UI's playhead.
pub fn current_step_index(
    sample_clock: u64,
    bpm: f32,
    sample_rate: f32,
    pattern_length_steps: u32,
) -> usize {
    if pattern_length_steps == 0 {
        return 0;
    }
    let samples_per_tick = (sample_rate as f64 * 60.0) / (bpm as f64 * 96.0);
    let tick = (sample_clock as f64 / samples_per_tick) as u64;
    let pattern_length_ticks = pattern_length_steps as u64 * TICKS_PER_STEP as u64;
    let pat_tick = tick % pattern_length_ticks;
    (pat_tick / TICKS_PER_STEP as u64) as usize
}

