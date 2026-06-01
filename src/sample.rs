use anyhow::{Context, Result};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// pcm sample data, normalized to mono or stereo f32 at the source rate.
/// the engine resamples on the fly during playback.
pub struct SampleData {
    pub frames: Vec<f32>,
    pub channels: u16,
    pub sample_rate: u32,
    pub path: PathBuf,
    pub name: String,
}

impl fmt::Debug for SampleData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SampleData")
            .field("frames", &self.frames.len())
            .field("channels", &self.channels)
            .field("sample_rate", &self.sample_rate)
            .field("path", &self.path)
            .field("name", &self.name)
            .finish()
    }
}

impl SampleData {
    pub fn frame_count(&self) -> usize {
        self.frames.len() / self.channels.max(1) as usize
    }

    pub fn read_frame(&self, idx: usize) -> (f32, f32) {
        let ch = self.channels.max(1) as usize;
        let i = idx * ch;
        if i + ch > self.frames.len() {
            return (0.0, 0.0);
        }
        if ch == 1 {
            let s = self.frames[i];
            (s, s)
        } else {
            (self.frames[i], self.frames[i + 1])
        }
    }
}

pub fn load_wav<P: AsRef<Path>>(path: P) -> Result<Arc<SampleData>> {
    let path_ref = path.as_ref();
    let mut reader = hound::WavReader::open(path_ref)
        .with_context(|| format!("opening wav at {}", path_ref.display()))?;
    let spec = reader.spec();
    let channels = spec.channels;
    let sample_rate = spec.sample_rate;
    let frames: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .filter_map(|s| s.ok())
            .collect(),
        hound::SampleFormat::Int => {
            let scale = (1u64 << (spec.bits_per_sample as u64 - 1)) as f32;
            reader
                .samples::<i32>()
                .filter_map(|s| s.ok())
                .map(|s| s as f32 / scale)
                .collect()
        }
    };

    let name = path_ref
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("sample")
        .to_string();

    Ok(Arc::new(SampleData {
        frames,
        channels,
        sample_rate,
        path: path_ref.to_path_buf(),
        name,
    }))
}

pub fn is_audio_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()).map(|s| s.to_ascii_lowercase()),
        Some(ext) if ext == "wav"
    )
}
