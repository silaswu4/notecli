mod agent;
mod command;
mod dsp;
mod engine;
mod midi;
mod midi_out;
mod model;
mod sample;
mod transport;
mod ui;
mod voice;

use std::sync::atomic::AtomicU16;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Context, Result};
use arc_swap::ArcSwap;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use ringbuf::traits::Split;
use ringbuf::HeapRb;

use crate::command::Command;
use crate::engine::Engine;
use crate::midi_out::MidiOutMessage;
use crate::model::{ChannelKind, Project};
use crate::transport::Transport;
use crate::ui::app::App;

fn main() -> Result<()> {
    // ---- project ----
    let project = Project::default();
    let project_handle = Arc::new(ArcSwap::new(Arc::new(project.clone())));
    let transport = Arc::new(Transport::new(project.bpm));

    // ---- launch any standalone synths the project wants alongside tek ----
    launch_external_apps(&project);

    // ---- ringbuf for ui -> audio commands ----
    let rb = HeapRb::<Command>::new(512);
    let (cmd_tx, cmd_rx) = rb.split();

    // ---- ringbuf for audio -> midi-out worker ----
    let midi_rb = HeapRb::<MidiOutMessage>::new(512);
    let (midi_tx, midi_rx) = midi_rb.split();
    // spawn the worker. if midi isn't available the worker just won't exist,
    // and the producer's pushes are harmless (they fill the queue but the
    // ringbuf throws away when full).
    let _midi_worker = midi_out::spawn_worker(midi_rx).ok().flatten();

    // ---- audio ----
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| anyhow!("no default audio output device"))?;
    let supported = device
        .default_output_config()
        .context("query default output config")?;
    let sample_format = supported.sample_format();
    let config: StreamConfig = supported.config();
    let sample_rate = config.sample_rate.0 as f32;
    let channels = config.channels as usize;

    let mut engine = Engine::new(
        project_handle.clone(),
        transport.clone(),
        cmd_rx,
        sample_rate,
        Some(midi_tx),
    );

    let err_fn = |err| eprintln!("cpal stream error: {err}");
    let stream = match sample_format {
        SampleFormat::F32 => device.build_output_stream(
            &config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                render(&mut engine, data, channels);
            },
            err_fn,
            None,
        )?,
        SampleFormat::I16 => {
            let mut engine = engine;
            let mut scratch: Vec<f32> = Vec::new();
            device.build_output_stream(
                &config,
                move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                    scratch.resize(data.len(), 0.0);
                    render(&mut engine, &mut scratch, channels);
                    for (out, &s) in data.iter_mut().zip(scratch.iter()) {
                        let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
                        *out = v;
                    }
                },
                err_fn,
                None,
            )?
        }
        SampleFormat::U16 => {
            let mut engine = engine;
            let mut scratch: Vec<f32> = Vec::new();
            device.build_output_stream(
                &config,
                move |data: &mut [u16], _: &cpal::OutputCallbackInfo| {
                    scratch.resize(data.len(), 0.0);
                    render(&mut engine, &mut scratch, channels);
                    for (out, &s) in data.iter_mut().zip(scratch.iter()) {
                        let v = ((s.clamp(-1.0, 1.0) * 0.5 + 0.5) * u16::MAX as f32) as u16;
                        *out = v;
                    }
                },
                err_fn,
                None,
            )?
        }
        other => return Err(anyhow!("unsupported sample format: {other}")),
    };
    stream.play()?;

    // ---- midi input (best-effort) ----
    let armed_channel = Arc::new(AtomicU16::new(0));
    let cmd_tx_shared = Arc::new(Mutex::new(cmd_tx));
    let _midi_guard = midi::open_default(cmd_tx_shared.clone(), armed_channel.clone()).ok();
    let cmd_tx_for_ui = cmd_tx_shared.clone();

    // ---- terminal ----
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // panic hook to restore terminal
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(std::io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
        original_hook(info);
    }));

    let mut app = App::new(project, project_handle, transport, cmd_tx_for_ui, sample_rate);
    let result = app.run(&mut terminal);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    drop(stream);

    result
}

/// walks every channel and tries to open any standalone .app it asks for.
/// uses macos `open -a` so any installed app name resolves. silently ignores
/// failures so a missing install doesn't block tek from starting.
fn launch_external_apps(project: &Project) {
    use std::collections::HashSet;
    use std::process::Command;
    let mut launched = HashSet::new();
    for ch in &project.channels {
        if let ChannelKind::MidiOut(p) = &ch.kind {
            if let Some(app) = &p.launch_app {
                if launched.insert(app.clone()) {
                    let _ = Command::new("open").arg("-a").arg(app).spawn();
                }
            }
        }
    }
}

fn render(engine: &mut Engine, data: &mut [f32], channels: usize) {
    if channels == 2 {
        engine.render(data);
        return;
    }
    if channels == 1 {
        // produce stereo into a scratch, then collapse to mono.
        let frames = data.len();
        let mut stereo = vec![0.0_f32; frames * 2];
        engine.render(&mut stereo);
        for (i, out) in data.iter_mut().enumerate() {
            let l = stereo[i * 2];
            let r = stereo[i * 2 + 1];
            *out = (l + r) * 0.5;
        }
        return;
    }
    // for >2 channels, render stereo and place into first two channels of each frame.
    let frames = data.len() / channels;
    let mut stereo = vec![0.0_f32; frames * 2];
    engine.render(&mut stereo);
    for f in 0..frames {
        let base = f * channels;
        data[base] = stereo[f * 2];
        if channels >= 2 {
            data[base + 1] = stereo[f * 2 + 1];
        }
        for c in 2..channels {
            data[base + c] = 0.0;
        }
    }
}
