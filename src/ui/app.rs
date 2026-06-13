use anyhow::Result;
use arc_swap::ArcSwap;
use crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Terminal;
use ratatui::prelude::Backend;
use ringbuf::traits::Producer;
use ringbuf::HeapProd;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::command::Command;
use crate::model::{Channel, ChannelKind, Note, OscKind, PatternTrack, Project, Step, TICKS_PER_STEP};
use crate::sample::{is_audio_file, load_wav};
use crate::transport::Transport;
use crate::ui::theme;
use crate::ui::views::browser::{BrowserEntry, BrowserView};
use crate::ui::views::channels::{ChannelsView, ChannelsViewState};
use crate::ui::views::mixer::MixerView;
use crate::ui::views::piano::PianoView;
use crate::ui::views::playlist::PlaylistView;
use crate::ui::views::View;

pub struct App {
    pub project: Project,
    pub project_handle: Arc<ArcSwap<Project>>,
    pub transport: Arc<Transport>,
    pub cmd_tx: Arc<Mutex<HeapProd<Command>>>,
    pub sample_rate: f32,
    pub view: View,
    pub cursor_channel: usize,
    pub cursor_step: usize,
    pub cursor_tick: u32,
    pub cursor_pitch: u8,
    pub cursor_bar: usize,
    pub mixer_cursor: usize,
    pub browser_cwd: PathBuf,
    pub browser_entries: Vec<BrowserEntry>,
    pub browser_cursor: usize,
    pub browser_message: Option<String>,
    pub should_quit: bool,
    pub status: String,
    last_render: Instant,
    cached_chunks: Option<[Rect; 4]>,
    pub input_mode: InputMode,
    pub input_buffer: String,
    pub agent_rx: Option<std::sync::mpsc::Receiver<crate::agent::AgentResult>>,
    pub agent_inflight: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputMode {
    Idle,
    AgentPrompt,
    AgentVariation,
}

impl App {
    pub fn new(
        project: Project,
        project_handle: Arc<ArcSwap<Project>>,
        transport: Arc<Transport>,
        cmd_tx: Arc<Mutex<HeapProd<Command>>>,
        sample_rate: f32,
    ) -> Self {
        let browser_cwd = dirs_home().unwrap_or_else(|| PathBuf::from("/"));
        let mut app = Self {
            project,
            project_handle,
            transport,
            cmd_tx,
            sample_rate,
            view: View::Channels,
            cursor_channel: 0,
            cursor_step: 0,
            cursor_tick: 0,
            cursor_pitch: 60,
            cursor_bar: 0,
            mixer_cursor: 0,
            browser_cwd,
            browser_entries: Vec::new(),
            browser_cursor: 0,
            browser_message: None,
            should_quit: false,
            status: String::new(),
            last_render: Instant::now(),
            cached_chunks: None,
            input_mode: InputMode::Idle,
            input_buffer: String::new(),
            agent_rx: None,
            agent_inflight: false,
        };
        app.reload_browser();
        app
    }

    fn send(&self, cmd: Command) {
        if let Ok(mut tx) = self.cmd_tx.lock() {
            let _ = tx.try_push(cmd);
        }
    }

    pub fn run<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<()> {
        loop {
            self.poll_agent();
            self.draw(terminal)?;
            if event::poll(Duration::from_millis(16))? {
                match event::read()? {
                    Event::Key(k) if k.kind == KeyEventKind::Press => {
                        if self.input_mode != InputMode::Idle {
                            self.handle_input_key(k);
                        } else {
                            self.handle_key(k);
                        }
                    }
                    Event::Mouse(m) if self.input_mode == InputMode::Idle => {
                        self.handle_mouse(m);
                    }
                    Event::Resize(_, _) => {}
                    _ => {}
                }
            }
            if self.should_quit {
                break;
            }
        }
        Ok(())
    }

    fn poll_agent(&mut self) {
        let Some(rx) = self.agent_rx.as_ref() else { return };
        match rx.try_recv() {
            Ok(crate::agent::AgentResult::Pattern(p)) => {
                self.apply_agent_pattern(p);
                self.agent_inflight = false;
                self.agent_rx = None;
                self.status = "pattern generated".into();
            }
            Ok(crate::agent::AgentResult::Variations(v)) => {
                self.apply_agent_variations(v);
                self.agent_inflight = false;
                self.agent_rx = None;
                self.status = "variations added (use [ ] to flip through them)".into();
            }
            Ok(crate::agent::AgentResult::Error(e)) => {
                self.agent_inflight = false;
                self.agent_rx = None;
                self.status = format!("agent error: {e}");
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.agent_inflight = false;
                self.agent_rx = None;
            }
        }
    }

    fn handle_input_key(&mut self, k: KeyEvent) {
        match k.code {
            KeyCode::Esc => {
                self.input_mode = InputMode::Idle;
                self.input_buffer.clear();
            }
            KeyCode::Enter => self.submit_input(),
            KeyCode::Backspace => {
                self.input_buffer.pop();
            }
            KeyCode::Char(c) => self.input_buffer.push(c),
            _ => {}
        }
    }

    fn submit_input(&mut self) {
        let mode = self.input_mode;
        let prompt = std::mem::take(&mut self.input_buffer);
        self.input_mode = InputMode::Idle;
        if mode == InputMode::AgentVariation || !prompt.trim().is_empty() {
            self.start_agent(mode, prompt);
        }
    }

    fn start_agent(&mut self, mode: InputMode, prompt: String) {
        if std::env::var("ANTHROPIC_API_KEY").is_err() {
            self.status = "set ANTHROPIC_API_KEY in your env to use the agent".into();
            return;
        }
        let channels = self.channels_brief();
        let (tx, rx) = std::sync::mpsc::channel();
        match mode {
            InputMode::AgentPrompt => {
                crate::agent::spawn_pattern(prompt, channels, tx);
                self.status = "thinking…".into();
            }
            InputMode::AgentVariation => {
                let current = self.pattern_brief();
                crate::agent::spawn_variations(prompt, channels, current, tx);
                self.status = "generating variations…".into();
            }
            InputMode::Idle => return,
        }
        self.agent_rx = Some(rx);
        self.agent_inflight = true;
    }

    fn channels_brief(&self) -> String {
        let mut out = String::new();
        for (idx, ch) in self.project.channels.iter().enumerate() {
            let kind = match &ch.kind {
                ChannelKind::DrumSynth(k) => format!("drum {}", k.label()),
                ChannelKind::Synth(p) => format!("synth {}", p.osc.label()),
                ChannelKind::Sampler(_) => "sampler".into(),
                ChannelKind::MidiOut(_) => "midi-out".into(),
            };
            out.push_str(&format!("{idx}: {} ({kind})\n", ch.name));
        }
        out
    }

    fn pattern_brief(&self) -> String {
        let Some(pattern) = self.project.patterns.get(self.project.active_pattern as usize)
        else {
            return String::new();
        };
        let mut out = String::new();
        for (idx, ch) in self.project.channels.iter().enumerate() {
            let key = idx as u16;
            let steps: Vec<u8> = (0..pattern.length)
                .map(|i| {
                    pattern
                        .tracks
                        .get(&key)
                        .and_then(|t| t.steps.get(i as usize))
                        .map(|s| if s.active { 1 } else { 0 })
                        .unwrap_or(0)
                })
                .collect();
            let s: Vec<String> = steps.iter().map(|b| b.to_string()).collect();
            out.push_str(&format!("{idx}: {}: {}\n", ch.name, s.join(",")));
        }
        out
    }

    fn apply_agent_pattern(&mut self, agent: crate::agent::AgentPattern) {
        let pat_id = self.project.active_pattern as usize;
        let Some(pattern) = self.project.patterns.get_mut(pat_id) else { return };
        for ch in agent.channels {
            let track = pattern
                .tracks
                .entry(ch.index)
                .or_insert_with(|| crate::model::PatternTrack::with_steps(pattern.length));
            for (i, v) in ch.steps.iter().enumerate().take(track.steps.len()) {
                track.steps[i].active = *v != 0;
                if track.steps[i].active && track.steps[i].velocity == 0.0 {
                    track.steps[i].velocity = 1.0;
                }
            }
        }
        if let Some(bpm) = agent.bpm {
            if bpm > 20.0 && bpm < 999.0 {
                self.project.bpm = bpm;
                self.transport.set_bpm(bpm);
            }
        }
        self.commit();
    }

    fn apply_agent_variations(&mut self, agent: crate::agent::AgentVariations) {
        let base_length = self
            .project
            .patterns
            .get(self.project.active_pattern as usize)
            .map(|p| p.length)
            .unwrap_or(16);
        for (i, variant) in agent.variations.into_iter().enumerate() {
            let mut pattern = crate::model::Pattern::empty(
                &format!("agent {:02}", i + 1),
                base_length,
            );
            for ch in variant.channels {
                let mut track = crate::model::PatternTrack::with_steps(base_length);
                for (j, v) in ch.steps.iter().enumerate().take(track.steps.len()) {
                    track.steps[j].active = *v != 0;
                    if track.steps[j].active {
                        track.steps[j].velocity = 1.0;
                    }
                }
                pattern.tracks.insert(ch.index, track);
            }
            self.project.patterns.push(pattern);
        }
        self.commit();
    }

    fn draw<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<()> {
        let mut cache: Option<[Rect; 4]> = None;
        terminal.draw(|f| {
            let area = f.size();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Length(2),
                    Constraint::Min(8),
                    Constraint::Length(1),
                ])
                .split(area);

            self.draw_transport(f, chunks[0]);
            self.draw_tabs(f, chunks[1]);
            self.draw_view(f, chunks[2]);
            self.draw_status(f, chunks[3]);

            if self.input_mode != InputMode::Idle {
                self.draw_modal(f, area);
            }

            cache = Some([chunks[0], chunks[1], chunks[2], chunks[3]]);
        })?;
        self.cached_chunks = cache;
        Ok(())
    }

    fn draw_transport(&self, f: &mut ratatui::Frame, area: Rect) {
        let bpm = self.transport.bpm();
        let playing = self.transport.is_playing();
        let recording = self.transport.recording.load(std::sync::atomic::Ordering::Relaxed);
        let voices = self.transport.voices();
        let clock = self.transport.sample_position();
        let samples_per_step =
            self.sample_rate * 60.0 / (bpm * TICKS_PER_STEP as f32 * 96.0 / TICKS_PER_STEP as f32);
        let samples_per_step = (self.sample_rate * 60.0 / (bpm * 4.0)).max(1.0);
        let step_pos = (clock as f32 / samples_per_step) as u64;
        let bar = step_pos / 16 + 1;
        let beat = (step_pos / 4) % 4 + 1;
        let sub = step_pos % 4 + 1;

        let brand = Span::styled(
            " ✷ noteCLI ",
            Style::default().fg(theme::ACCENT_HI).add_modifier(Modifier::BOLD),
        );
        let transport_icon = Span::styled(
            if playing { " ▶ " } else { " ■ " },
            Style::default().fg(if playing { theme::GREEN } else { theme::TEXT_DIM }),
        );
        let bpm_text = Span::styled(
            format!(" {:.1} bpm ", bpm),
            Style::default().fg(theme::TEXT),
        );
        let pos_text = Span::styled(
            format!(" bar {}.{}.{} ", bar, beat, sub),
            Style::default().fg(theme::TEXT_DIM),
        );
        let vox_text = Span::styled(
            format!(" ◉ {} voices ", voices),
            Style::default().fg(theme::COOL),
        );
        let rec_text = if recording {
            Span::styled(" ● rec ", Style::default().fg(theme::HOT).add_modifier(Modifier::BOLD))
        } else {
            Span::styled(" · ", Style::default().fg(theme::DIM))
        };
        let project_name = Span::styled(
            format!(" {}  ", self.project.name),
            Style::default().fg(theme::MUTED),
        );

        let line = Line::from(vec![brand, transport_icon, bpm_text, pos_text, vox_text, rec_text, project_name]);
        let p = Paragraph::new(line)
            .style(Style::default().bg(theme::SURFACE));
        f.render_widget(p, Rect::new(area.x, area.y + 1, area.width, 1));

        // top + bottom border lines
        let top: String = std::iter::repeat('─').take(area.width as usize).collect();
        let border = Paragraph::new(top.clone()).style(Style::default().fg(theme::BORDER));
        f.render_widget(border.clone(), Rect::new(area.x, area.y, area.width, 1));
        f.render_widget(border, Rect::new(area.x, area.y + 2, area.width, 1));
    }

    fn draw_tabs(&self, f: &mut ratatui::Frame, area: Rect) {
        let views = [
            (View::Channels, '1'),
            (View::Piano, '2'),
            (View::Playlist, '3'),
            (View::Mixer, '4'),
            (View::Browser, '5'),
        ];
        let mut spans: Vec<Span> = Vec::new();
        for (v, k) in views {
            let active = v == self.view;
            let key_style = Style::default().fg(theme::ACCENT_DIM);
            let name_style = if active {
                Style::default()
                    .fg(theme::ACCENT_HI)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
            } else {
                Style::default().fg(theme::TEXT_DIM)
            };
            spans.push(Span::styled(format!("  {} ", k), key_style));
            spans.push(Span::styled(format!("{}  ", v.label()), name_style));
        }
        let line = Line::from(spans);
        f.render_widget(Paragraph::new(line), Rect::new(area.x, area.y, area.width, 1));
    }

    fn draw_view(&mut self, f: &mut ratatui::Frame, area: Rect) {
        match self.view {
            View::Channels => {
                let state = ChannelsViewState {
                    project: &self.project,
                    cursor_channel: self.cursor_channel,
                    cursor_step: self.cursor_step,
                    playing: self.transport.is_playing(),
                    sample_clock: self.transport.sample_position(),
                    sample_rate: self.sample_rate,
                };
                f.render_widget(ChannelsView { state }, area);
            }
            View::Piano => {
                let v = PianoView {
                    project: &self.project,
                    cursor_channel: self.cursor_channel,
                    cursor_tick: self.cursor_tick,
                    cursor_pitch: self.cursor_pitch,
                };
                f.render_widget(v, area);
            }
            View::Playlist => {
                let v = PlaylistView {
                    project: &self.project,
                    cursor_bar: self.cursor_bar,
                };
                f.render_widget(v, area);
            }
            View::Mixer => {
                let v = MixerView {
                    project: &self.project,
                    cursor: self.mixer_cursor.min(self.project.channels.len()),
                    voice_count: self.transport.voices(),
                };
                f.render_widget(v, area);
            }
            View::Browser => {
                let v = BrowserView {
                    cwd: &self.browser_cwd,
                    entries: &self.browser_entries,
                    cursor: self.browser_cursor,
                    message: self.browser_message.as_deref(),
                };
                f.render_widget(v, area);
            }
        }
    }

    fn draw_modal(&self, f: &mut ratatui::Frame, area: Rect) {
        use ratatui::widgets::{Block, Borders, Clear};
        let w = area.width.saturating_sub(8).min(72).max(40);
        let h = 7u16;
        let x = area.x + (area.width.saturating_sub(w)) / 2;
        let y = area.y + (area.height.saturating_sub(h)) / 2;
        let rect = Rect::new(x, y, w, h);
        f.render_widget(Clear, rect);
        let (title, hint) = match self.input_mode {
            InputMode::AgentPrompt => (
                " generate pattern ",
                "describe the vibe. enter to send, esc to cancel.",
            ),
            InputMode::AgentVariation => (
                " variations ",
                "optional direction (e.g. \"busier hats\"). enter to send, esc to cancel.",
            ),
            InputMode::Idle => return,
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::ACCENT))
            .title(Span::styled(
                title,
                Style::default().fg(theme::ACCENT_HI).add_modifier(Modifier::BOLD),
            ))
            .style(Style::default().bg(theme::SURFACE_HI));
        let inner = block.inner(rect);
        f.render_widget(block, rect);

        let hint_line = Paragraph::new(hint).style(Style::default().fg(theme::MUTED));
        f.render_widget(hint_line, Rect::new(inner.x, inner.y, inner.width, 1));

        let prompt = format!("> {}_", self.input_buffer);
        let body = Paragraph::new(prompt).style(Style::default().fg(theme::TEXT));
        f.render_widget(body, Rect::new(inner.x, inner.y + 2, inner.width, 1));
    }

    fn draw_status(&self, f: &mut ratatui::Frame, area: Rect) {
        let hint = match self.view {
            View::Channels => "[space] play  [x] toggle  [hjkl] nav  [[/]] pattern  [+/-] vol  [,/.] bpm  [?] help  [q] quit",
            View::Piano => "[space] play  [a] add note  [d] delete  [hjkl] nav  [q] quit",
            View::Playlist => "[space] play  [hjkl] nav  [enter] place  [q] quit",
            View::Mixer => "[space] play  [hl] strip  [+/-] vol  [m] mute  [o] solo  [q] quit",
            View::Browser => "[space] audition  [enter] load  [hjkl] nav  [q] quit",
        };
        let status = if self.status.is_empty() {
            hint.to_string()
        } else {
            format!("{}  ·  {}", self.status, hint)
        };
        let p = Paragraph::new(status).style(Style::default().fg(theme::MUTED));
        f.render_widget(p, area);
    }

    fn handle_key(&mut self, k: KeyEvent) {
        let mods = k.modifiers;
        let now = Instant::now();
        let _ = std::mem::replace(&mut self.last_render, now);
        match k.code {
            KeyCode::Char('q') => {
                self.should_quit = true;
                return;
            }
            KeyCode::Char(c) if matches!(c, '1'..='5') => {
                if let Some(v) = View::from_key(c) {
                    self.view = v;
                }
                return;
            }
            KeyCode::Char(' ') => {
                self.send(Command::PlayToggle);
                return;
            }
            KeyCode::Char('S') => {
                self.send(Command::Stop);
                return;
            }
            KeyCode::Char(',') => {
                self.project.bpm = (self.project.bpm - 1.0).max(20.0);
                self.transport.set_bpm(self.project.bpm);
                self.commit();
                return;
            }
            KeyCode::Char('.') => {
                self.project.bpm = (self.project.bpm + 1.0).min(999.0);
                self.transport.set_bpm(self.project.bpm);
                self.commit();
                return;
            }
            _ => {}
        }
        if mods.contains(KeyModifiers::CONTROL) {
            if let KeyCode::Char('s') = k.code {
                self.save_project();
                return;
            }
        }
        match self.view {
            View::Channels => self.handle_channels_key(k),
            View::Piano => self.handle_piano_key(k),
            View::Playlist => self.handle_playlist_key(k),
            View::Mixer => self.handle_mixer_key(k),
            View::Browser => self.handle_browser_key(k),
        }
    }

    fn handle_channels_key(&mut self, k: KeyEvent) {
        match k.code {
            KeyCode::Char('h') | KeyCode::Left => {
                self.cursor_step = self.cursor_step.saturating_sub(1);
            }
            KeyCode::Char('l') | KeyCode::Right => {
                let len = self
                    .project
                    .patterns
                    .get(self.project.active_pattern as usize)
                    .map(|p| p.length as usize)
                    .unwrap_or(16);
                self.cursor_step = (self.cursor_step + 1).min(len.saturating_sub(1));
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.cursor_channel = (self.cursor_channel + 1).min(self.project.channels.len().saturating_sub(1));
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.cursor_channel = self.cursor_channel.saturating_sub(1);
            }
            KeyCode::Char('x') | KeyCode::Enter => {
                self.toggle_current_step();
            }
            KeyCode::Char('c') => {
                self.clear_current_row();
            }
            KeyCode::Char('m') => {
                if let Some(ch) = self.project.channels.get_mut(self.cursor_channel) {
                    ch.mute = !ch.mute;
                    self.commit();
                }
            }
            KeyCode::Char('o') => {
                if let Some(ch) = self.project.channels.get_mut(self.cursor_channel) {
                    ch.solo = !ch.solo;
                    self.commit();
                }
            }
            KeyCode::Char('[') => {
                self.project.active_pattern = self.project.active_pattern.saturating_sub(1);
                self.commit();
            }
            KeyCode::Char(']') => {
                let max = self.project.patterns.len() as u16 - 1;
                self.project.active_pattern = (self.project.active_pattern + 1).min(max);
                self.commit();
            }
            KeyCode::Char('n') => {
                self.add_pattern();
            }
            KeyCode::Char('a') => {
                self.add_channel();
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                if let Some(ch) = self.project.channels.get_mut(self.cursor_channel) {
                    ch.volume = (ch.volume + 0.05).clamp(0.0, 1.5);
                    self.commit();
                }
            }
            KeyCode::Char('-') | KeyCode::Char('_') => {
                if let Some(ch) = self.project.channels.get_mut(self.cursor_channel) {
                    ch.volume = (ch.volume - 0.05).clamp(0.0, 1.5);
                    self.commit();
                }
            }
            KeyCode::Tab => {
                // cycle channel kind: drum -> synth -> sampler -> drum
                if let Some(ch) = self.project.channels.get_mut(self.cursor_channel) {
                    ch.kind = match &ch.kind {
                        ChannelKind::DrumSynth(_) => ChannelKind::Synth(crate::model::SynthParams {
                            osc: OscKind::Saw,
                            attack: 0.005,
                            decay: 0.15,
                            sustain: 0.6,
                            release: 0.25,
                            filter_cutoff: 0.55,
                            filter_resonance: 0.2,
                            filter_env: 0.3,
                        }),
                        ChannelKind::Synth(_) => {
                            ChannelKind::MidiOut(crate::model::MidiOutParams::default())
                        }
                        ChannelKind::MidiOut(_) => {
                            ChannelKind::DrumSynth(crate::model::DrumKind::Kick)
                        }
                        ChannelKind::Sampler(_) => ChannelKind::DrumSynth(crate::model::DrumKind::Kick),
                    };
                    self.commit();
                }
            }
            KeyCode::Char('p') => {
                // preview / trigger the focused channel
                self.send(Command::Trigger {
                    channel: self.cursor_channel as u16,
                    pitch: 60,
                    velocity: 1.0,
                });
            }
            KeyCode::Char('g') => {
                self.input_mode = InputMode::AgentPrompt;
                self.input_buffer.clear();
            }
            KeyCode::Char('G') => {
                self.input_mode = InputMode::AgentVariation;
                self.input_buffer.clear();
            }
            _ => {}
        }
    }

    fn handle_piano_key(&mut self, k: KeyEvent) {
        match k.code {
            KeyCode::Char('h') | KeyCode::Left => {
                self.cursor_tick = self.cursor_tick.saturating_sub(TICKS_PER_STEP);
            }
            KeyCode::Char('l') | KeyCode::Right => {
                let len = self
                    .project
                    .patterns
                    .get(self.project.active_pattern as usize)
                    .map(|p| p.length * TICKS_PER_STEP)
                    .unwrap_or(256);
                self.cursor_tick = (self.cursor_tick + TICKS_PER_STEP).min(len.saturating_sub(1));
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.cursor_pitch = self.cursor_pitch.saturating_sub(1);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.cursor_pitch = (self.cursor_pitch + 1).min(127);
            }
            KeyCode::Char('a') => {
                self.add_note();
            }
            KeyCode::Char('d') => {
                self.delete_note();
            }
            _ => {}
        }
    }

    fn handle_playlist_key(&mut self, k: KeyEvent) {
        match k.code {
            KeyCode::Char('h') | KeyCode::Left => {
                self.cursor_bar = self.cursor_bar.saturating_sub(1);
            }
            KeyCode::Char('l') | KeyCode::Right => {
                self.cursor_bar = (self.cursor_bar + 1).min(15);
            }
            KeyCode::Enter => {
                let active = self.project.active_pattern;
                if self.cursor_bar >= self.project.playlist.len() {
                    self.project.playlist.push(active);
                } else {
                    self.project.playlist[self.cursor_bar] = active;
                }
                self.commit();
            }
            KeyCode::Char('d') => {
                if self.cursor_bar < self.project.playlist.len() {
                    self.project.playlist.remove(self.cursor_bar);
                    self.commit();
                }
            }
            _ => {}
        }
    }

    fn handle_mixer_key(&mut self, k: KeyEvent) {
        let max_cursor = self.project.channels.len();
        match k.code {
            KeyCode::Char('h') | KeyCode::Left => {
                self.mixer_cursor = self.mixer_cursor.saturating_sub(1);
            }
            KeyCode::Char('l') | KeyCode::Right => {
                self.mixer_cursor = (self.mixer_cursor + 1).min(max_cursor);
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                if self.mixer_cursor == max_cursor {
                    self.project.master_volume = (self.project.master_volume + 0.05).clamp(0.0, 1.5);
                } else if let Some(ch) = self.project.channels.get_mut(self.mixer_cursor) {
                    ch.volume = (ch.volume + 0.05).clamp(0.0, 1.5);
                }
                self.commit();
            }
            KeyCode::Char('-') | KeyCode::Char('_') => {
                if self.mixer_cursor == max_cursor {
                    self.project.master_volume = (self.project.master_volume - 0.05).clamp(0.0, 1.5);
                } else if let Some(ch) = self.project.channels.get_mut(self.mixer_cursor) {
                    ch.volume = (ch.volume - 0.05).clamp(0.0, 1.5);
                }
                self.commit();
            }
            KeyCode::Char('<') => {
                if let Some(ch) = self.project.channels.get_mut(self.mixer_cursor) {
                    ch.pan = (ch.pan - 0.1).clamp(-1.0, 1.0);
                    self.commit();
                }
            }
            KeyCode::Char('>') => {
                if let Some(ch) = self.project.channels.get_mut(self.mixer_cursor) {
                    ch.pan = (ch.pan + 0.1).clamp(-1.0, 1.0);
                    self.commit();
                }
            }
            KeyCode::Char('m') => {
                if let Some(ch) = self.project.channels.get_mut(self.mixer_cursor) {
                    ch.mute = !ch.mute;
                    self.commit();
                }
            }
            KeyCode::Char('o') => {
                if let Some(ch) = self.project.channels.get_mut(self.mixer_cursor) {
                    ch.solo = !ch.solo;
                    self.commit();
                }
            }
            _ => {}
        }
    }

    fn handle_mouse(&mut self, m: MouseEvent) {
        let Some(chunks) = self.cached_chunks else { return };
        let transport = chunks[0];
        let tabs = chunks[1];
        let view_area = chunks[2];
        let col = m.column;
        let row = m.row;

        let in_rect = |r: Rect| -> bool {
            col >= r.x && col < r.x + r.width && row >= r.y && row < r.y + r.height
        };

        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if in_rect(transport) {
                    // transport bar has play / pause icon ~ chars 8-10 on the middle line
                    if row == transport.y + 1 && col >= transport.x + 8 && col <= transport.x + 11 {
                        self.send(Command::PlayToggle);
                        return;
                    }
                }
                if in_rect(tabs) {
                    self.hit_tab(col, tabs);
                    return;
                }
                if in_rect(view_area) {
                    match self.view {
                        View::Channels => self.hit_channels(col, row, view_area, false),
                        View::Piano => self.hit_piano(col, row, view_area),
                        View::Playlist => self.hit_playlist(col, row, view_area),
                        View::Mixer => self.hit_mixer(col, row, view_area, 0),
                        View::Browser => self.hit_browser(col, row, view_area),
                    }
                }
            }
            MouseEventKind::Down(MouseButton::Right) => {
                if in_rect(view_area) && self.view == View::Channels {
                    self.hit_channels(col, row, view_area, true);
                }
                if in_rect(view_area) && self.view == View::Piano {
                    // right click in piano: delete note under cursor
                    self.hit_piano(col, row, view_area);
                    self.delete_note();
                }
            }
            MouseEventKind::ScrollUp => {
                if in_rect(view_area) {
                    match self.view {
                        View::Channels => {
                            self.cursor_channel = self.cursor_channel.saturating_sub(1);
                        }
                        View::Mixer => self.hit_mixer(col, row, view_area, 1),
                        View::Browser => {
                            self.browser_cursor = self.browser_cursor.saturating_sub(1);
                        }
                        View::Piano => {
                            self.cursor_pitch = (self.cursor_pitch + 1).min(127);
                        }
                        View::Playlist => {
                            self.cursor_bar = self.cursor_bar.saturating_sub(1);
                        }
                    }
                }
            }
            MouseEventKind::ScrollDown => {
                if in_rect(view_area) {
                    match self.view {
                        View::Channels => {
                            self.cursor_channel = (self.cursor_channel + 1)
                                .min(self.project.channels.len().saturating_sub(1));
                        }
                        View::Mixer => self.hit_mixer(col, row, view_area, -1),
                        View::Browser => {
                            self.browser_cursor = (self.browser_cursor + 1)
                                .min(self.browser_entries.len().saturating_sub(1));
                        }
                        View::Piano => {
                            self.cursor_pitch = self.cursor_pitch.saturating_sub(1);
                        }
                        View::Playlist => {
                            self.cursor_bar = (self.cursor_bar + 1).min(15);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn hit_tab(&mut self, col: u16, tabs: Rect) {
        // tab labels rendered as "  1 channels  " etc. compute approximate spans.
        let labels = [
            (View::Channels, "channels"),
            (View::Piano, "piano"),
            (View::Playlist, "playlist"),
            (View::Mixer, "mixer"),
            (View::Browser, "browser"),
        ];
        let mut x = tabs.x;
        for (v, label) in labels.iter() {
            let width = 2 + 1 + 1 + 1 + label.chars().count() as u16 + 2;
            if col >= x && col < x + width {
                self.view = *v;
                return;
            }
            x += width;
        }
    }

    fn hit_channels(&mut self, col: u16, row: u16, area: Rect, right: bool) {
        let inner = Rect::new(area.x + 1, area.y + 1, area.width.saturating_sub(2), area.height.saturating_sub(2));
        let body_y = inner.y + 2;
        let body_h = inner.height.saturating_sub(3);
        if row < body_y || row >= body_y + body_h {
            return;
        }
        // mirror the render math so click maps to the right channel
        let n_channels = self.project.channels.len().max(1);
        let row_h = ((body_h as usize) / n_channels).clamp(1, 3) as u16;
        let visible_count = (body_h / row_h.max(1)) as usize;
        let local_row = (row - body_y) / row_h.max(1);
        let scroll = self.cursor_channel.saturating_sub(visible_count.saturating_sub(1));
        let channel_idx = scroll + local_row as usize;
        if channel_idx >= self.project.channels.len() {
            return;
        }

        let num_w = 4u16;
        let name_w = 14u16;
        let vol_w = 5u16;
        let pan_w = 4u16;
        let ms_w = 5u16;
        let kind_w = 7u16;
        let meta_w = num_w + name_w + vol_w + pan_w + ms_w + kind_w;
        let col_off = col.saturating_sub(inner.x);

        if col_off < meta_w {
            // meta columns
            let mute_start = num_w + name_w + vol_w + pan_w + 1;
            let solo_start = mute_start + 2;
            let kind_start = num_w + name_w + vol_w + pan_w + ms_w;
            self.cursor_channel = channel_idx;
            if col_off >= mute_start && col_off < mute_start + 1 {
                if let Some(ch) = self.project.channels.get_mut(channel_idx) {
                    ch.mute = !ch.mute;
                    self.commit();
                }
            } else if col_off >= solo_start && col_off < solo_start + 1 {
                if let Some(ch) = self.project.channels.get_mut(channel_idx) {
                    ch.solo = !ch.solo;
                    self.commit();
                }
            } else if col_off >= kind_start && col_off < kind_start + kind_w {
                // click on kind cycles it
                if let Some(ch) = self.project.channels.get_mut(channel_idx) {
                    ch.kind = match &ch.kind {
                        ChannelKind::DrumSynth(_) => ChannelKind::Synth(crate::model::SynthParams {
                            osc: OscKind::Saw,
                            attack: 0.005,
                            decay: 0.15,
                            sustain: 0.6,
                            release: 0.25,
                            filter_cutoff: 0.55,
                            filter_resonance: 0.2,
                            filter_env: 0.3,
                        }),
                        ChannelKind::Synth(_) => {
                            ChannelKind::MidiOut(crate::model::MidiOutParams::default())
                        }
                        ChannelKind::MidiOut(_) => {
                            ChannelKind::DrumSynth(crate::model::DrumKind::Kick)
                        }
                        ChannelKind::Sampler(_) => ChannelKind::DrumSynth(crate::model::DrumKind::Kick),
                    };
                    self.commit();
                }
            } else if !right {
                // preview / trigger
                self.send(Command::Trigger {
                    channel: channel_idx as u16,
                    pitch: 60,
                    velocity: 1.0,
                });
            }
            return;
        }

        // step area
        let steps_off = (col_off - meta_w) as usize;
        let steps_area_w = (inner.width as usize).saturating_sub(meta_w as usize);
        let step_w = ((steps_area_w.saturating_sub(3)) / 16).max(2);
        // group gaps after step indices 3, 7, 11 (gap of 1 char)
        let mut x = 0usize;
        for i in 0..16 {
            let end = x + step_w;
            if steps_off >= x && steps_off < end {
                self.cursor_channel = channel_idx;
                self.cursor_step = i;
                self.toggle_current_step();
                return;
            }
            x = end;
            if matches!(i, 3 | 7 | 11) {
                x += 1;
            }
        }
    }

    fn hit_piano(&mut self, col: u16, row: u16, area: Rect) {
        let inner = Rect::new(area.x + 1, area.y + 1, area.width.saturating_sub(2), area.height.saturating_sub(2));
        let grid_y = inner.y + 2;
        let grid_h = inner.height.saturating_sub(3);
        let key_w = 4u16;
        let body_w = inner.width.saturating_sub(key_w);

        if row < grid_y || row >= grid_y + grid_h {
            return;
        }
        if col < inner.x + key_w {
            // clicked the keyboard side. pick that row's pitch.
            let row_off = row - grid_y;
            let pitch_lo = (self.cursor_pitch as i32 - grid_h as i32 / 2).max(0) as i32;
            let pitch = (pitch_lo + (grid_h as i32 - 1 - row_off as i32)).clamp(0, 127);
            self.cursor_pitch = pitch as u8;
            return;
        }
        let col_off = col - inner.x - key_w;
        let length = self
            .project
            .patterns
            .get(self.project.active_pattern as usize)
            .map(|p| p.length)
            .unwrap_or(16);
        let total_ticks = length * TICKS_PER_STEP;
        let tick_per_col = (total_ticks as f32 / body_w as f32).max(1.0);
        let tick = (col_off as f32 * tick_per_col) as u32;
        self.cursor_tick = tick.min(total_ticks.saturating_sub(1));

        let row_off = row - grid_y;
        let pitch_lo = (self.cursor_pitch as i32 - grid_h as i32 / 2).max(0) as i32;
        let pitch = (pitch_lo + (grid_h as i32 - 1 - row_off as i32)).clamp(0, 127);
        self.cursor_pitch = pitch as u8;
        self.add_note();
    }

    fn hit_playlist(&mut self, col: u16, row: u16, area: Rect) {
        let inner = Rect::new(area.x + 1, area.y + 1, area.width.saturating_sub(2), area.height.saturating_sub(2));
        let lane_y = inner.y + 4;
        if row != lane_y {
            return;
        }
        let label_w = 8u16;
        let body_w = inner.width.saturating_sub(label_w);
        let bar_w = (body_w / 16).max(2);
        let col_off = col.saturating_sub(inner.x + label_w);
        let bar = (col_off / bar_w) as usize;
        if bar >= 16 {
            return;
        }
        self.cursor_bar = bar;
        let active = self.project.active_pattern;
        if bar >= self.project.playlist.len() {
            self.project.playlist.push(active);
        } else {
            self.project.playlist[bar] = active;
        }
        self.commit();
    }

    fn hit_mixer(&mut self, col: u16, row: u16, area: Rect, scroll_dir: i32) {
        let inner = Rect::new(area.x + 1, area.y + 1, area.width.saturating_sub(2), area.height.saturating_sub(2));
        let strip_w = 8u16;
        if row < inner.y + 2 {
            return;
        }
        let col_off = col.saturating_sub(inner.x);
        let strip_idx = (col_off / strip_w) as usize;
        let max = self.project.channels.len();
        if strip_idx > max {
            return;
        }
        self.mixer_cursor = strip_idx;

        if scroll_dir != 0 {
            let delta = scroll_dir as f32 * 0.05;
            if strip_idx == max {
                self.project.master_volume = (self.project.master_volume + delta).clamp(0.0, 1.5);
            } else if let Some(ch) = self.project.channels.get_mut(strip_idx) {
                ch.volume = (ch.volume + delta).clamp(0.0, 1.5);
            }
            self.commit();
        }
    }

    fn hit_browser(&mut self, col: u16, row: u16, area: Rect) {
        let _ = col;
        let inner = Rect::new(area.x + 1, area.y + 1, area.width.saturating_sub(2), area.height.saturating_sub(2));
        let list_y = inner.y + 3;
        if row < list_y {
            return;
        }
        let row_off = (row - list_y) as usize;
        let list_h = inner.height.saturating_sub(5) as usize;
        let scroll = self.browser_cursor.saturating_sub(list_h.saturating_sub(1));
        let idx = scroll + row_off;
        if idx >= self.browser_entries.len() {
            return;
        }
        // if same row clicked twice quickly we'd open. for v1, single click selects,
        // and we open if it's already focused (poor man's double click).
        if self.browser_cursor == idx {
            let Some(entry) = self.browser_entries.get(idx).cloned() else { return };
            if entry.is_dir {
                self.browser_cwd = entry.path.clone();
                self.reload_browser();
            } else if entry.is_audio {
                self.load_sample_into_channel(entry.path);
            }
        } else {
            self.browser_cursor = idx;
        }
    }

    fn handle_browser_key(&mut self, k: KeyEvent) {
        match k.code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.browser_cursor = (self.browser_cursor + 1).min(self.browser_entries.len().saturating_sub(1));
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.browser_cursor = self.browser_cursor.saturating_sub(1);
            }
            KeyCode::Char('h') | KeyCode::Left => {
                if let Some(parent) = self.browser_cwd.parent() {
                    self.browser_cwd = parent.to_path_buf();
                    self.reload_browser();
                }
            }
            KeyCode::Char('l') | KeyCode::Right | KeyCode::Enter => {
                let Some(entry) = self.browser_entries.get(self.browser_cursor).cloned() else { return };
                if entry.is_dir {
                    self.browser_cwd = entry.path.clone();
                    self.reload_browser();
                } else if entry.is_audio {
                    self.load_sample_into_channel(entry.path);
                }
            }
            KeyCode::Char(' ') => {
                // audition: trigger current channel with this sample temporarily skipped for v1.
                // for now, just mark a message.
                if let Some(entry) = self.browser_entries.get(self.browser_cursor) {
                    self.browser_message = Some(format!("preview: {}", entry.name));
                }
            }
            _ => {}
        }
    }

    fn toggle_current_step(&mut self) {
        let pat_id = self.project.active_pattern as usize;
        let ch_id = self.cursor_channel as u16;
        if let Some(pattern) = self.project.patterns.get_mut(pat_id) {
            let track = pattern
                .tracks
                .entry(ch_id)
                .or_insert_with(|| crate::model::PatternTrack::with_steps(pattern.length));
            if track.steps.is_empty() {
                track.steps = (0..pattern.length).map(|_| Step::default()).collect();
            }
            if let Some(step) = track.steps.get_mut(self.cursor_step) {
                step.toggle();
            }
        }
        self.commit();
    }

    fn clear_current_row(&mut self) {
        let pat_id = self.project.active_pattern as usize;
        let ch_id = self.cursor_channel as u16;
        if let Some(pattern) = self.project.patterns.get_mut(pat_id) {
            if let Some(track) = pattern.tracks.get_mut(&ch_id) {
                for s in &mut track.steps {
                    s.active = false;
                }
                track.notes.clear();
            }
        }
        self.commit();
    }

    fn add_pattern(&mut self) {
        let n = self.project.patterns.len();
        let pattern = crate::model::Pattern::empty(&format!("{:02}", n + 1), 16);
        self.project.patterns.push(pattern);
        self.project.active_pattern = n as u16;
        self.commit();
    }

    fn add_channel(&mut self) {
        let n = self.project.channels.len();
        self.project.channels.push(Channel::new_synth(
            &format!("synth {}", n + 1),
            OscKind::Saw,
        ));
        self.commit();
    }

    fn add_note(&mut self) {
        let pat_id = self.project.active_pattern as usize;
        let ch_id = self.cursor_channel as u16;
        let pitch = self.cursor_pitch;
        let start = self.cursor_tick;
        if let Some(pattern) = self.project.patterns.get_mut(pat_id) {
            let length = pattern.length;
            let track = pattern
                .tracks
                .entry(ch_id)
                .or_insert_with(|| PatternTrack::with_steps(length));
            track.notes.push(Note {
                pitch,
                start,
                length: TICKS_PER_STEP,
                velocity: 1.0,
            });
            // mirror the note onto the channel rack so it lights up while the
            // pattern plays. the step also drives the sequencer, so the note
            // actually sounds at its pitch.
            mirror_note_onto_step(track, length, pitch, start, 1.0);
        }
        self.commit();
    }

    fn delete_note(&mut self) {
        let pat_id = self.project.active_pattern as usize;
        let ch_id = self.cursor_channel as u16;
        let cp = self.cursor_pitch;
        let ct = self.cursor_tick;
        if let Some(pattern) = self.project.patterns.get_mut(pat_id) {
            if let Some(track) = pattern.tracks.get_mut(&ch_id) {
                track.notes.retain(|n| {
                    !(n.pitch == cp && ct >= n.start && ct < n.start + n.length.max(1))
                });
                // clear the mirrored step only if no other note still lands on
                // it, so deleting one note doesn't wipe a stacked neighbor.
                let idx = (ct / TICKS_PER_STEP) as usize;
                let still_used = track
                    .notes
                    .iter()
                    .any(|n| (n.start / TICKS_PER_STEP) as usize == idx);
                if !still_used {
                    if let Some(step) = track.steps.get_mut(idx) {
                        step.active = false;
                    }
                }
            }
        }
        self.commit();
    }

    fn load_sample_into_channel(&mut self, path: PathBuf) {
        let result = load_wav(&path);
        match result {
            Ok(sample) => {
                let name = sample.name.clone();
                let new_channel = Channel::new_sampler(&name, path.clone(), Some(sample));
                // replace focused channel if it's a sampler, otherwise append
                let idx = self.cursor_channel;
                if let Some(ch) = self.project.channels.get_mut(idx) {
                    if matches!(ch.kind, ChannelKind::Sampler(_)) {
                        *ch = new_channel;
                    } else {
                        self.project.channels.push(new_channel);
                        self.cursor_channel = self.project.channels.len() - 1;
                    }
                } else {
                    self.project.channels.push(new_channel);
                    self.cursor_channel = self.project.channels.len() - 1;
                }
                self.browser_message = Some(format!("loaded: {}", name));
                self.commit();
            }
            Err(e) => {
                self.browser_message = Some(format!("error: {}", e));
            }
        }
    }

    fn reload_browser(&mut self) {
        self.browser_entries.clear();
        self.browser_cursor = 0;
        if let Ok(read) = std::fs::read_dir(&self.browser_cwd) {
            let mut entries: Vec<BrowserEntry> = read
                .filter_map(|r| r.ok())
                .filter_map(|e| {
                    let path = e.path();
                    let name = e.file_name().to_string_lossy().to_string();
                    if name.starts_with('.') {
                        return None;
                    }
                    let is_dir = path.is_dir();
                    let is_audio = !is_dir && is_audio_file(&path);
                    Some(BrowserEntry { name, path, is_dir, is_audio })
                })
                .collect();
            entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            });
            self.browser_entries = entries;
        }
    }

    fn commit(&self) {
        self.project_handle.store(Arc::new(self.project.clone()));
    }

    fn save_project(&mut self) {
        let path = PathBuf::from(format!("{}.notecli.toml", self.project.name));
        match toml::to_string_pretty(&self.project) {
            Ok(s) => {
                if std::fs::write(&path, s).is_ok() {
                    self.status = format!("saved {}", path.display());
                } else {
                    self.status = format!("failed to write {}", path.display());
                }
            }
            Err(e) => {
                self.status = format!("serialize error: {}", e);
            }
        }
    }
}

/// light the channel-rack step that a piano-roll note starts on, carrying its
/// pitch (as an offset from c4 = 60) and velocity. this keeps the sequencer
/// grid in sync with what you draw in the piano roll, so notes are visible as
/// the pattern plays and the step engine triggers them at the right pitch.
fn mirror_note_onto_step(track: &mut PatternTrack, length: u32, pitch: u8, start: u32, velocity: f32) {
    if track.steps.len() != length as usize {
        track.steps = (0..length).map(|_| Step::default()).collect();
    }
    let idx = (start / TICKS_PER_STEP) as usize;
    if let Some(step) = track.steps.get_mut(idx) {
        step.active = true;
        step.pitch_offset = (pitch as i32 - 60).clamp(i8::MIN as i32, i8::MAX as i32) as i8;
        step.velocity = velocity.max(0.1);
    }
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

// silence unused import warnings for unused crate items reached only via UI
#[allow(dead_code)]
fn _consume_project(_p: &Project) {}
