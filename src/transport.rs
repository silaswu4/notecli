use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

/// shared transport state. all atomics so audio + ui both touch it lock-free.
pub struct Transport {
    pub playing: AtomicBool,
    pub recording: AtomicBool,
    pub sample_clock: AtomicU64,
    pub voice_count: AtomicU32,
    pub bpm_x100: AtomicU32,
    pub master_db_x100: AtomicU32,
}

impl Transport {
    pub fn new(bpm: f32) -> Self {
        Self {
            playing: AtomicBool::new(false),
            recording: AtomicBool::new(false),
            sample_clock: AtomicU64::new(0),
            voice_count: AtomicU32::new(0),
            bpm_x100: AtomicU32::new((bpm * 100.0) as u32),
            master_db_x100: AtomicU32::new(0),
        }
    }

    pub fn bpm(&self) -> f32 {
        self.bpm_x100.load(Ordering::Relaxed) as f32 / 100.0
    }

    pub fn set_bpm(&self, bpm: f32) {
        self.bpm_x100
            .store((bpm.clamp(20.0, 999.0) * 100.0) as u32, Ordering::Relaxed);
    }

    pub fn is_playing(&self) -> bool {
        self.playing.load(Ordering::Relaxed)
    }

    pub fn toggle_playing(&self) {
        let p = self.playing.load(Ordering::Relaxed);
        self.playing.store(!p, Ordering::Relaxed);
    }

    pub fn stop(&self) {
        self.playing.store(false, Ordering::Relaxed);
        self.sample_clock.store(0, Ordering::Relaxed);
    }

    pub fn voices(&self) -> u32 {
        self.voice_count.load(Ordering::Relaxed)
    }

    pub fn sample_position(&self) -> u64 {
        self.sample_clock.load(Ordering::Relaxed)
    }
}
