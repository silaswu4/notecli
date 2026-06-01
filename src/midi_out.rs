use anyhow::Result;
use midir::os::unix::VirtualOutput;
use midir::{MidiOutput, MidiOutputConnection};
use ringbuf::traits::Consumer;
use ringbuf::HeapCons;
use std::thread;
use std::time::Duration;

/// messages the audio thread pushes into the midi-out queue. the worker
/// thread pops them on a tight loop and serializes them out to the
/// virtual port. all variants encode the standard midi status byte +
/// data bytes so the worker just builds three bytes and sends.
#[derive(Clone, Copy, Debug)]
pub enum MidiOutMessage {
    NoteOn { channel: u8, pitch: u8, velocity: u8 },
    NoteOff { channel: u8, pitch: u8 },
    AllNotesOff { channel: u8 },
}

/// open a virtual midi output port called "tek-out". any daw on the system
/// can pick it as a midi input source. on macos this shows up under the
/// iac driver list. returns None gracefully if midi isn't available.
pub fn spawn_worker(mut rx: HeapCons<MidiOutMessage>) -> Result<Option<thread::JoinHandle<()>>> {
    let out = match MidiOutput::new("tek") {
        Ok(o) => o,
        Err(_) => return Ok(None),
    };
    let conn = match out.create_virtual("tek-out") {
        Ok(c) => c,
        Err(_) => return Ok(None),
    };

    let handle = thread::spawn(move || {
        let mut conn: MidiOutputConnection = conn;
        loop {
            let mut got_any = false;
            while let Some(msg) = rx.try_pop() {
                got_any = true;
                let bytes = encode(msg);
                let _ = conn.send(&bytes);
            }
            if !got_any {
                thread::sleep(Duration::from_millis(1));
            }
        }
    });
    Ok(Some(handle))
}

fn encode(msg: MidiOutMessage) -> [u8; 3] {
    match msg {
        MidiOutMessage::NoteOn { channel, pitch, velocity } => {
            let ch = channel.saturating_sub(1).min(15);
            [0x90 | ch, pitch & 0x7f, velocity & 0x7f]
        }
        MidiOutMessage::NoteOff { channel, pitch } => {
            let ch = channel.saturating_sub(1).min(15);
            [0x80 | ch, pitch & 0x7f, 0]
        }
        MidiOutMessage::AllNotesOff { channel } => {
            let ch = channel.saturating_sub(1).min(15);
            // controller 123 = all notes off
            [0xb0 | ch, 123, 0]
        }
    }
}
