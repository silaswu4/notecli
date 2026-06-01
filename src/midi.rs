use anyhow::Result;
use midir::{MidiInput, MidiInputConnection};
use ringbuf::traits::Producer;
use ringbuf::HeapProd;
use std::sync::{Arc, Mutex};

use crate::command::Command;
use crate::model::ChannelId;

/// open the first available midi input port. notes get routed to a single
/// "armed" channel for now (selected in the channel rack). returns a
/// connection guard that must stay alive for the duration of midi reception.
pub fn open_default(
    cmd_tx: Arc<Mutex<HeapProd<Command>>>,
    armed_channel: Arc<std::sync::atomic::AtomicU16>,
) -> Result<Option<MidiInputConnection<()>>> {
    let input = MidiInput::new("tek")?;
    let ports = input.ports();
    if ports.is_empty() {
        return Ok(None);
    }
    let port = &ports[0];
    let _port_name = input.port_name(port).unwrap_or_default();
    let conn = input.connect(
        port,
        "tek-input",
        move |_stamp, message, _| {
            if message.len() < 3 {
                return;
            }
            let status = message[0] & 0xf0;
            let pitch = message[1];
            let velocity = message[2] as f32 / 127.0;
            let channel = armed_channel.load(std::sync::atomic::Ordering::Relaxed) as ChannelId;
            let cmd = match status {
                0x90 if velocity > 0.0 => Command::Trigger { channel, pitch, velocity },
                0x80 | 0x90 => Command::Release { channel, pitch },
                _ => return,
            };
            if let Ok(mut tx) = cmd_tx.lock() {
                let _ = tx.try_push(cmd);
            }
        },
        (),
    )?;
    Ok(Some(conn))
}
