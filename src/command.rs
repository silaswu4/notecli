use crate::model::ChannelId;

/// commands ui sends to the audio thread for things that need to fire RIGHT
/// NOW rather than at the next pattern-snapshot read. transport hits, midi
/// notes, sample auditions, etc.
#[derive(Clone, Debug)]
pub enum Command {
    /// trigger one shot on the given channel immediately (audition, midi).
    Trigger {
        channel: ChannelId,
        pitch: u8,
        velocity: f32,
    },
    /// release any sustaining voices on the given channel + pitch.
    Release {
        channel: ChannelId,
        pitch: u8,
    },
    /// play / pause toggle. the engine reads this and flips state.
    PlayToggle,
    /// hard stop and reset playhead.
    Stop,
    /// reset position without affecting playing state.
    Rewind,
    /// audition a sample buffer not tied to any channel.
    AuditionSample {
        sample_ix: u32,
    },
}
