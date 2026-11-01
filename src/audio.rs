//! Sound playback (the `love.audio` equivalent).
//!
//! Sounds are decoded once into memory and mixed on the audio thread. Every
//! control (`set_volume`, `set_pitch`, `pause`, ...) is lock-free and takes
//! effect immediately, even while the sound is playing.

// `as f32` casts on samples are deliberate: rodio's `64bit` feature (which any
// crate in the dependency graph may enable) makes `rodio::Sample` an `f64`.
#![allow(clippy::unnecessary_cast)]

use std::io::Cursor;
use std::num::NonZero;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::Duration;

use rodio::Source as _;

use crate::{Error, Result};

/// Decoded, interleaved `f32` samples. Cheap to clone.
#[derive(Clone)]
pub struct SoundData {
    samples: Arc<[f32]>,
    channels: u16,
    sample_rate: u32,
}

impl std::fmt::Debug for SoundData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SoundData")
            .field("channels", &self.channels)
            .field("sample_rate", &self.sample_rate)
            .field("duration", &self.duration())
            .finish()
    }
}

impl SoundData {
    /// Wraps interleaved samples in `-1.0..=1.0`.
    pub fn from_samples(sample_rate: u32, channels: u16, samples: Vec<f32>) -> Self {
        SoundData {
            samples: samples.into(),
            channels: channels.max(1),
            sample_rate: sample_rate.max(1),
        }
    }

    /// Decodes a wav / ogg vorbis / mp3 / flac file held in memory.
    pub fn decode(bytes: Vec<u8>) -> Result<Self> {
        let decoder =
            rodio::Decoder::new(Cursor::new(bytes)).map_err(|e| Error::Audio(e.to_string()))?;
        let channels = decoder.channels().get();
        let sample_rate = decoder.sample_rate().get();
        let samples: Vec<f32> = decoder.map(|s| s as f32).collect();
        Ok(Self::from_samples(sample_rate, channels, samples))
    }

    /// Decodes an audio file from disk.
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let bytes = std::fs::read(path.as_ref()).map_err(|e| Error::io(path.as_ref(), e))?;
        Self::decode(bytes)
    }

    /// Synthesizes mono audio from `f(t) -> sample`, with `t` in seconds.
    /// Great for quick sound effects without asset files.
    pub fn generate(sample_rate: u32, seconds: f32, mut f: impl FnMut(f32) -> f32) -> Self {
        let n = (sample_rate as f32 * seconds) as usize;
        let samples = (0..n)
            .map(|i| f(i as f32 / sample_rate as f32).clamp(-1.0, 1.0))
            .collect();
        Self::from_samples(sample_rate, 1, samples)
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn frames(&self) -> usize {
        self.samples.len() / self.channels as usize
    }

    /// Length in seconds.
    pub fn duration(&self) -> f32 {
        self.frames() as f32 / self.sample_rate as f32
    }
}

/// Lock-free controls shared between a [`Source`] and its voice on the audio thread.
struct Controls {
    volume: AtomicU32,
    pitch: AtomicU32,
    looping: AtomicBool,
    paused: AtomicBool,
    playing: AtomicBool,
    /// Bumped on stop/restart; a voice whose generation is stale ends itself.
    generation: AtomicU64,
    /// Playback position in frames.
    position: AtomicU64,
    /// Requested seek in frames (`u64::MAX` = none).
    seek: AtomicU64,
}

fn load_f32(a: &AtomicU32) -> f32 {
    f32::from_bits(a.load(Ordering::Relaxed))
}

fn store_f32(a: &AtomicU32, v: f32) {
    a.store(v.to_bits(), Ordering::Relaxed);
}

/// The sound iterator handed to the rodio mixer.
struct Voice {
    data: SoundData,
    controls: Arc<Controls>,
    master: Arc<AtomicU32>,
    generation: u64,
    pos: f64,
    channel: usize,
    frame: [f32; 8],
    gain: f32,
}

impl Voice {
    /// Computes the next interleaved frame into `self.frame`. Returns false at the end.
    fn advance(&mut self) -> bool {
        let c = &self.controls;
        if c.generation.load(Ordering::Relaxed) != self.generation {
            return false;
        }
        let seek = c.seek.swap(u64::MAX, Ordering::Relaxed);
        if seek != u64::MAX {
            self.pos = seek as f64;
        }
        let channels = self.data.channels as usize;
        if c.paused.load(Ordering::Relaxed) {
            self.frame[..channels.min(8)].fill(0.0);
            return true;
        }
        let frames = self.data.frames();
        if frames == 0 {
            c.playing.store(false, Ordering::Relaxed);
            return false;
        }
        if self.pos >= frames as f64 {
            if c.looping.load(Ordering::Relaxed) {
                self.pos %= frames as f64;
            } else {
                c.playing.store(false, Ordering::Relaxed);
                c.position.store(0, Ordering::Relaxed);
                return false;
            }
        }
        let i0 = self.pos as usize;
        let frac = (self.pos - i0 as f64) as f32;
        let i1 = if i0 + 1 < frames {
            i0 + 1
        } else if c.looping.load(Ordering::Relaxed) {
            0
        } else {
            i0
        };
        self.gain = load_f32(&c.volume) * load_f32(&self.master);
        let s = &self.data.samples;
        for ch in 0..channels.min(8) {
            let a = s[i0 * channels + ch];
            let b = s[i1 * channels + ch];
            self.frame[ch] = (a + (b - a) * frac) * self.gain;
        }
        self.pos += load_f32(&c.pitch).max(0.0) as f64;
        c.position.store(self.pos as u64, Ordering::Relaxed);
        true
    }
}

impl Iterator for Voice {
    type Item = rodio::Sample;

    fn next(&mut self) -> Option<rodio::Sample> {
        if self.channel == 0 && !self.advance() {
            return None;
        }
        let v = self.frame[self.channel.min(7)];
        self.channel = (self.channel + 1) % self.data.channels as usize;
        Some(v as rodio::Sample)
    }
}

impl rodio::Source for Voice {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> rodio::ChannelCount {
        NonZero::new(self.data.channels).unwrap()
    }

    fn sample_rate(&self) -> rodio::SampleRate {
        NonZero::new(self.data.sample_rate).unwrap()
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

/// A playable sound with its own volume, pitch and looping (`love.audio.newSource`).
///
/// Like in LÖVE, a `Source` is a single voice: calling [`play`](Source::play)
/// while it is already playing does nothing. For overlapping sound effects use
/// [`play_overlapping`](Source::play_overlapping), or [`duplicate`](Source::duplicate)
/// it. Cloning a `Source` gives another handle to the *same* voice.
#[derive(Clone)]
pub struct Source {
    data: SoundData,
    controls: Arc<Controls>,
    mixer: Option<rodio::mixer::Mixer>,
    master: Arc<AtomicU32>,
}

impl std::fmt::Debug for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Source")
            .field("data", &self.data)
            .field("playing", &self.is_playing())
            .finish()
    }
}

impl Source {
    fn new(data: SoundData, mixer: Option<rodio::mixer::Mixer>, master: Arc<AtomicU32>) -> Self {
        Source {
            data,
            controls: Arc::new(Controls {
                volume: AtomicU32::new(1.0f32.to_bits()),
                pitch: AtomicU32::new(1.0f32.to_bits()),
                looping: AtomicBool::new(false),
                paused: AtomicBool::new(false),
                playing: AtomicBool::new(false),
                generation: AtomicU64::new(0),
                position: AtomicU64::new(0),
                seek: AtomicU64::new(u64::MAX),
            }),
            mixer,
            master,
        }
    }

    /// Starts playing (or resumes if paused). Does nothing if already playing.
    pub fn play(&self) {
        let c = &self.controls;
        if c.playing.load(Ordering::Relaxed) {
            c.paused.store(false, Ordering::Relaxed);
            return;
        }
        let Some(mixer) = &self.mixer else { return };
        let generation = c.generation.fetch_add(1, Ordering::Relaxed) + 1;
        let start = c.seek.swap(u64::MAX, Ordering::Relaxed);
        let start = if start == u64::MAX {
            c.position.load(Ordering::Relaxed)
        } else {
            start
        };
        c.paused.store(false, Ordering::Relaxed);
        c.playing.store(true, Ordering::Relaxed);
        mixer.add(Voice {
            data: self.data.clone(),
            controls: c.clone(),
            master: self.master.clone(),
            generation,
            pos: start as f64,
            channel: 0,
            frame: [0.0; 8],
            gain: 1.0,
        });
    }

    /// Plays an independent copy with this source's current volume and pitch.
    /// Ideal for sound effects that may overlap (shots, footsteps, coins).
    pub fn play_overlapping(&self) {
        let copy = self.duplicate();
        copy.play();
    }

    /// An independent source sharing the same decoded data (`source:clone()`).
    pub fn duplicate(&self) -> Source {
        let s = Source::new(self.data.clone(), self.mixer.clone(), self.master.clone());
        s.set_volume(self.volume());
        s.set_pitch(self.pitch());
        s.set_looping(self.is_looping());
        s
    }

    /// Pauses; [`play`](Source::play) resumes from the same position.
    pub fn pause(&self) {
        if self.is_playing() {
            self.controls.paused.store(true, Ordering::Relaxed);
        }
    }

    /// Stops and rewinds to the start.
    pub fn stop(&self) {
        let c = &self.controls;
        c.generation.fetch_add(1, Ordering::Relaxed);
        c.playing.store(false, Ordering::Relaxed);
        c.paused.store(false, Ordering::Relaxed);
        c.position.store(0, Ordering::Relaxed);
        c.seek.store(u64::MAX, Ordering::Relaxed);
    }

    /// Whether the source is playing (paused counts as not playing).
    pub fn is_playing(&self) -> bool {
        self.controls.playing.load(Ordering::Relaxed)
            && !self.controls.paused.load(Ordering::Relaxed)
    }

    pub fn is_paused(&self) -> bool {
        self.controls.playing.load(Ordering::Relaxed)
            && self.controls.paused.load(Ordering::Relaxed)
    }

    /// Volume multiplier (default `1.0`).
    pub fn set_volume(&self, volume: f32) {
        store_f32(&self.controls.volume, volume.max(0.0));
    }

    pub fn volume(&self) -> f32 {
        load_f32(&self.controls.volume)
    }

    /// Playback speed / pitch multiplier (default `1.0`; `2.0` = one octave up).
    pub fn set_pitch(&self, pitch: f32) {
        store_f32(&self.controls.pitch, pitch.max(0.0));
    }

    pub fn pitch(&self) -> f32 {
        load_f32(&self.controls.pitch)
    }

    pub fn set_looping(&self, looping: bool) {
        self.controls.looping.store(looping, Ordering::Relaxed);
    }

    pub fn is_looping(&self) -> bool {
        self.controls.looping.load(Ordering::Relaxed)
    }

    /// Jumps to `seconds` from the start.
    pub fn seek(&self, seconds: f32) {
        let frame = (seconds.max(0.0) * self.data.sample_rate as f32) as u64;
        self.controls.seek.store(frame, Ordering::Relaxed);
        if !self.controls.playing.load(Ordering::Relaxed) {
            self.controls.position.store(frame, Ordering::Relaxed);
        }
    }

    /// Current playback position in seconds.
    pub fn tell(&self) -> f32 {
        self.controls.position.load(Ordering::Relaxed) as f32 / self.data.sample_rate as f32
    }

    /// Length in seconds.
    pub fn duration(&self) -> f32 {
        self.data.duration()
    }

    pub fn data(&self) -> &SoundData {
        &self.data
    }
}

/// The audio device. Reach it through `ctx.audio`.
///
/// If no output device is available the game still runs; sources are silent.
pub struct Audio {
    sink: Option<rodio::MixerDeviceSink>,
    master: Arc<AtomicU32>,
}

impl Audio {
    pub(crate) fn new() -> Self {
        let sink = match rodio::DeviceSinkBuilder::open_default_sink() {
            Ok(mut s) => {
                s.log_on_drop(false);
                Some(s)
            }
            Err(e) => {
                log::warn!("no audio output available ({e}); sounds will be silent");
                None
            }
        };
        Audio {
            sink,
            master: Arc::new(AtomicU32::new(1.0f32.to_bits())),
        }
    }

    /// Whether an audio device was opened.
    pub fn is_available(&self) -> bool {
        self.sink.is_some()
    }

    fn mixer(&self) -> Option<rodio::mixer::Mixer> {
        self.sink.as_ref().map(|s| s.mixer().clone())
    }

    /// Loads and decodes an audio file (wav, ogg, mp3, flac).
    pub fn new_source(&self, path: impl AsRef<Path>) -> Result<Source> {
        Ok(self.new_source_from_data(SoundData::load(path)?))
    }

    /// Decodes an audio file held in memory, e.g. from `include_bytes!`.
    pub fn new_source_from_bytes(&self, bytes: &[u8]) -> Result<Source> {
        Ok(self.new_source_from_data(SoundData::decode(bytes.to_vec())?))
    }

    /// Creates a source from already decoded or generated samples.
    pub fn new_source_from_data(&self, data: SoundData) -> Source {
        Source::new(data, self.mixer(), self.master.clone())
    }

    /// Master volume applied to every source (`love.audio.setVolume`).
    pub fn set_volume(&self, volume: f32) {
        store_f32(&self.master, volume.max(0.0));
    }

    pub fn volume(&self) -> f32 {
        load_f32(&self.master)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn voice(source: &Source, pos: f64) -> Voice {
        Voice {
            data: source.data.clone(),
            controls: source.controls.clone(),
            master: source.master.clone(),
            generation: source.controls.generation.load(Ordering::Relaxed),
            pos,
            channel: 0,
            frame: [0.0; 8],
            gain: 1.0,
        }
    }

    fn source(samples: Vec<f32>, channels: u16) -> Source {
        Source::new(
            SoundData::from_samples(10, channels, samples),
            None,
            Arc::new(AtomicU32::new(1f32.to_bits())),
        )
    }

    #[test]
    fn plays_all_samples_then_ends() {
        let s = source(vec![0.1, 0.2, 0.3, 0.4], 1);
        let out: Vec<f32> = voice(&s, 0.0).map(|v| v as f32).collect();
        assert_eq!(out, vec![0.1, 0.2, 0.3, 0.4]);
    }

    #[test]
    fn stereo_frames_stay_interleaved() {
        let s = source(vec![1.0, -1.0, 0.5, -0.5], 2);
        let out: Vec<f32> = voice(&s, 0.0).map(|v| v as f32).collect();
        assert_eq!(out, vec![1.0, -1.0, 0.5, -0.5]);
    }

    #[test]
    fn volume_and_master_scale_output() {
        let s = source(vec![1.0, 1.0], 1);
        s.set_volume(0.5);
        store_f32(&s.master, 0.5);
        let out: Vec<f32> = voice(&s, 0.0).map(|v| v as f32).collect();
        assert_eq!(out, vec![0.25, 0.25]);
    }

    #[test]
    fn pitch_two_skips_every_other_frame() {
        let s = source(vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0], 1);
        s.set_pitch(2.0);
        let out: Vec<f32> = voice(&s, 0.0).map(|v| v as f32).collect();
        assert_eq!(out, vec![0.0, 2.0, 4.0]);
    }

    #[test]
    fn looping_wraps_and_stop_ends_voice() {
        let s = source(vec![0.1, 0.2], 1);
        s.set_looping(true);
        let mut v = voice(&s, 0.0);
        let first: Vec<f32> = v.by_ref().take(5).map(|x| x as f32).collect();
        assert_eq!(first, vec![0.1, 0.2, 0.1, 0.2, 0.1]);
        s.stop();
        assert!(v.next().is_none(), "stopped voice must end");
    }

    #[test]
    fn generate_has_expected_length() {
        let d = SoundData::generate(1000, 0.5, |t| t);
        assert_eq!(d.frames(), 500);
        assert!((d.duration() - 0.5).abs() < 1e-6);
    }
}
