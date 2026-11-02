//! Sound playback (the `love.audio` equivalent).
//!
//! Sounds are decoded once into memory and mixed on the audio thread. Every
//! control (`set_volume`, `set_pitch`, `pause`, ...) is lock-free and takes
//! effect immediately, even while the sound is playing.
//!
//! The stream to the audio device comes and goes while the game runs: it's closed
//! while the app is in the background or interrupted, which saves battery, and
//! reopened if the device goes away, such as when a headset is unplugged. Playing
//! sounds carry on from where they were each time, so games don't have to do
//! anything about it.

// `as f32` casts on samples are deliberate: rodio's `64bit` feature (which any
// crate in the dependency graph may enable) makes `rodio::Sample` an `f64`.
#![allow(clippy::unnecessary_cast)]

use std::io::Cursor;
use std::num::NonZero;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

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
        let bytes = crate::filesystem::read(path)?;
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

    /// Number of interleaved channels (1 = mono, 2 = stereo).
    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// Sample frames per second.
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
    engine: Arc<Engine>,
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
    fn new(data: SoundData, engine: Arc<Engine>, master: Arc<AtomicU32>) -> Self {
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
            engine,
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
        if !self.engine.can_start(c.looping.load(Ordering::Relaxed)) {
            return;
        }
        let start = c.seek.swap(u64::MAX, Ordering::Relaxed);
        let start = if start == u64::MAX {
            c.position.load(Ordering::Relaxed)
        } else {
            start
        };
        c.paused.store(false, Ordering::Relaxed);
        c.playing.store(true, Ordering::Relaxed);
        c.position.store(start, Ordering::Relaxed);
        self.engine.start(Playing {
            data: self.data.clone(),
            controls: c.clone(),
            master: self.master.clone(),
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
        let s = Source::new(self.data.clone(), self.engine.clone(), self.master.clone());
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

    /// Whether the source was paused with [`pause`](Source::pause) and not yet resumed.
    pub fn is_paused(&self) -> bool {
        self.controls.playing.load(Ordering::Relaxed)
            && self.controls.paused.load(Ordering::Relaxed)
    }

    /// Volume multiplier (default `1.0`).
    pub fn set_volume(&self, volume: f32) {
        store_f32(&self.controls.volume, volume.max(0.0));
    }

    /// The volume multiplier set with [`set_volume`](Source::set_volume).
    pub fn volume(&self) -> f32 {
        load_f32(&self.controls.volume)
    }

    /// Playback speed / pitch multiplier (default `1.0`; `2.0` = one octave up).
    pub fn set_pitch(&self, pitch: f32) {
        store_f32(&self.controls.pitch, pitch.max(0.0));
    }

    /// The pitch multiplier set with [`set_pitch`](Source::set_pitch).
    pub fn pitch(&self) -> f32 {
        load_f32(&self.controls.pitch)
    }

    /// Whether playback restarts from the beginning when it reaches the end.
    pub fn set_looping(&self, looping: bool) {
        self.controls.looping.store(looping, Ordering::Relaxed);
    }

    /// Whether the source loops.
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

    /// The decoded samples this source plays.
    pub fn data(&self) -> &SoundData {
        &self.data
    }
}

/// A sound that's playing, with what it takes to start it on a new stream.
struct Playing {
    data: SoundData,
    controls: Arc<Controls>,
    master: Arc<AtomicU32>,
}

impl Playing {
    /// Makes the voice that plays this from its current position. Any older one
    /// ends itself, because this takes its generation away.
    fn voice(&self) -> Voice {
        let c = &self.controls;
        Voice {
            data: self.data.clone(),
            controls: c.clone(),
            master: self.master.clone(),
            generation: c.generation.fetch_add(1, Ordering::Relaxed) + 1,
            pos: c.position.load(Ordering::Relaxed) as f64,
            channel: 0,
            frame: [0.0; 8],
            gain: 1.0,
        }
    }

    fn is_playing(&self) -> bool {
        self.controls.playing.load(Ordering::Relaxed)
    }
}

/// What sources share: the current stream's mixer, and what's playing so it can
/// move to the next stream. Only the game's threads lock these, never the audio
/// thread, so playback itself stays lock-free.
#[derive(Default)]
struct Engine {
    mixer: Mutex<Option<rodio::mixer::Mixer>>,
    playing: Mutex<Vec<Playing>>,
    /// The stream is closed on purpose and will be back.
    held: AtomicBool,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // Everything here stays valid if a holder panicked.
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

impl Engine {
    /// Whether a sound can start now. Without a stream, a sound waits for it if
    /// the stream is sure to come back or if it loops. A one-shot would be late
    /// by the time a missing device turns up, so it's dropped.
    fn can_start(&self, looping: bool) -> bool {
        looping || self.held.load(Ordering::Relaxed) || lock(&self.mixer).is_some()
    }

    fn start(&self, sound: Playing) {
        if let Some(mixer) = &*lock(&self.mixer) {
            mixer.add(sound.voice());
        }
        let mut playing = lock(&self.playing);
        playing.retain(|p| p.is_playing() && !Arc::ptr_eq(&p.controls, &sound.controls));
        playing.push(sound);
    }

    /// Moves everything that's playing to a new stream's mixer.
    fn attach(&self, mixer: rodio::mixer::Mixer) {
        let mut playing = lock(&self.playing);
        playing.retain(Playing::is_playing);
        for sound in playing.iter() {
            mixer.add(sound.voice());
        }
        *lock(&self.mixer) = Some(mixer);
    }

    fn detach(&self) {
        *lock(&self.mixer) = None;
    }
}

/// Why the stream is closed even if a device is there. Each is a separate bit,
/// as several can apply at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum Hold {
    /// The app is in the background.
    #[cfg_attr(not(any(target_os = "android", target_os = "ios")), allow(dead_code))]
    Background = 1,
    /// Android paused the app, but it can still be seen behind something.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    Paused = 2,
    /// iOS gave the audio to something else, like a phone call.
    #[cfg_attr(not(target_os = "ios"), allow(dead_code))]
    Interrupted = 4,
}

const RETRY_MIN: Duration = Duration::from_secs(1);
const RETRY_MAX: Duration = Duration::from_secs(10);
/// A stream that fails sooner than this after opening isn't worth reopening
/// right away, or a device that keeps failing would glitch on every frame.
const STREAM_STABLE: Duration = Duration::from_secs(5);

/// Decides when the stream should be open.
struct StreamPolicy {
    holds: u8,
    /// There was a working device, last we knew.
    available: bool,
    opened_at: Option<Instant>,
    next_try: Option<Instant>,
    delay: Duration,
}

impl StreamPolicy {
    fn new() -> Self {
        StreamPolicy {
            holds: 0,
            available: false,
            opened_at: None,
            next_try: None,
            delay: RETRY_MIN,
        }
    }

    fn wants_open(&self, now: Instant) -> bool {
        self.holds == 0 && self.next_try.is_none_or(|t| now >= t)
    }

    /// Whether to look beyond the default device. Only on the first try, as
    /// listing devices can be slow enough to notice in a game.
    fn search_all_devices(&self) -> bool {
        self.delay == RETRY_MIN
    }

    fn opened(&mut self, now: Instant) {
        self.available = true;
        self.opened_at = Some(now);
        self.next_try = None;
    }

    fn back_off(&mut self, now: Instant) {
        self.next_try = Some(now + self.delay);
        self.delay = (self.delay * 2).min(RETRY_MAX);
    }

    fn open_failed(&mut self, now: Instant) {
        self.available = false;
        self.back_off(now);
    }

    /// The open stream stopped working.
    fn lost(&mut self, now: Instant) {
        self.available = false;
        let stable = self
            .opened_at
            .take()
            .is_some_and(|at| now.duration_since(at) >= STREAM_STABLE);
        if stable {
            self.next_try = None;
            self.delay = RETRY_MIN;
        } else {
            self.back_off(now);
        }
    }

    fn set_hold(&mut self, hold: Hold, on: bool) {
        if on {
            self.holds |= hold as u8;
        } else {
            self.holds &= !(hold as u8);
            // Much can have changed in the meantime, so try again right away.
            self.next_try = None;
            self.delay = RETRY_MIN;
        }
    }

    fn is_held(&self) -> bool {
        self.holds != 0
    }
}

/// Opens a stream to the default device, or if `search_all` is set, whichever
/// device works. `failed` is set if the stream stops working later.
fn open_sink(
    failed: &Arc<AtomicBool>,
    search_all: bool,
) -> std::result::Result<rodio::MixerDeviceSink, rodio::DeviceSinkError> {
    use rodio::DeviceSinkBuilder;
    use rodio::cpal::traits::{DeviceTrait, HostTrait};

    let failed = failed.clone();
    let on_error = move |error: rodio::cpal::StreamError| {
        // A glitch, which doesn't stop the stream.
        if !matches!(error, rodio::cpal::StreamError::BufferUnderrun) {
            log::warn!("audio stream error: {error}");
            failed.store(true, Ordering::Release);
        }
    };
    let default = DeviceSinkBuilder::from_default_device().and_then(|builder| {
        builder
            .with_error_callback(on_error.clone())
            .open_sink_or_fallback()
    });
    default.or_else(|error| {
        let others = rodio::cpal::default_host().output_devices();
        match others {
            Ok(mut devices) if search_all => devices
                .find_map(|device| {
                    let real = device
                        .description()
                        .is_ok_and(|d| d.driver().is_some_and(|driver| driver != "null"));
                    real.then(|| DeviceSinkBuilder::from_device(device).ok())
                        .flatten()
                        .and_then(|builder| {
                            builder
                                .with_error_callback(on_error.clone())
                                .open_sink_or_fallback()
                                .ok()
                        })
                })
                .ok_or(error),
            _ => Err(error),
        }
    })
}

/// How a game's sound gets along with other apps' on iOS, chosen with
/// [`Audio::set_session`]. Other platforms always mix with other apps, so there
/// this only records the choice.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AudioSession {
    /// Plays over the player's music, and stays quiet when the phone's silent
    /// switch is on. The default, which suits most casual games.
    #[default]
    Ambient,
    /// Stops the player's music, and stays quiet when the silent switch is on.
    /// What iOS does if an app doesn't choose.
    SoloAmbient,
    /// Stops the player's music, and plays even with the silent switch on, for
    /// games where sound is the point. The game still stops in the background.
    Playback,
}

#[cfg(target_os = "ios")]
#[path = "audio_ios.rs"]
mod session;

#[cfg(not(target_os = "ios"))]
mod session {
    pub(super) fn apply(_session: super::AudioSession) {}
    pub(super) fn set_active(_active: bool) {}
}

/// The audio device. Reach it through `ctx.audio`.
///
/// If no output device is available the game still runs; sources are silent.
/// When a device turns up, or comes back, sound starts by itself. Sounds that
/// were playing carry on, and looping sounds started meanwhile begin. Other
/// sounds started without a device are dropped.
pub struct Audio {
    engine: Arc<Engine>,
    sink: Option<rodio::MixerDeviceSink>,
    /// Set from the audio thread when the stream stops working.
    failed: Arc<AtomicBool>,
    policy: StreamPolicy,
    session: AudioSession,
    master: Arc<AtomicU32>,
}

impl Audio {
    pub(crate) fn new() -> Self {
        let mut audio = Audio {
            engine: Arc::default(),
            sink: None,
            failed: Arc::default(),
            policy: StreamPolicy::new(),
            session: AudioSession::default(),
            master: Arc::new(AtomicU32::new(1.0f32.to_bits())),
        };
        session::apply(audio.session);
        audio.open_stream(Instant::now());
        audio
    }

    fn open_stream(&mut self, now: Instant) {
        let first_try = self.policy.search_all_devices();
        session::set_active(true);
        match open_sink(&self.failed, first_try) {
            Ok(mut sink) => {
                sink.log_on_drop(false);
                self.failed.store(false, Ordering::Release);
                self.engine.attach(sink.mixer().clone());
                self.sink = Some(sink);
                self.policy.opened(now);
            }
            Err(e) => {
                if first_try {
                    log::warn!("no audio output available ({e}); sounds will be silent");
                }
                self.policy.open_failed(now);
            }
        }
    }

    fn close_stream(&mut self) {
        self.engine.detach();
        self.sink = None;
    }

    /// Handles a failed stream or a device that may have turned up. Called once
    /// per frame, so it's cheap unless something needs doing.
    pub(crate) fn poll(&mut self) {
        #[cfg(target_os = "ios")]
        self.poll_session();
        let now = Instant::now();
        if self.failed.swap(false, Ordering::Acquire) && self.sink.is_some() {
            log::info!("the audio stream stopped working, so opening a new one");
            self.close_stream();
            self.policy.lost(now);
        }
        if self.sink.is_none() && self.policy.wants_open(now) {
            self.open_stream(now);
        }
    }

    #[cfg(target_os = "ios")]
    fn poll_session(&mut self) {
        if session::take_media_services_reset() {
            // Everything about the session is gone, including what was chosen.
            log::info!("iOS reset its media services, so setting up audio again");
            self.close_stream();
            session::apply(self.session);
        }
        let interrupted = session::is_interrupted();
        if interrupted != (self.policy.holds & Hold::Interrupted as u8 != 0) {
            self.set_hold(Hold::Interrupted, interrupted);
        }
    }

    /// Closes the stream for this reason, or reopens it once no reason is left.
    pub(crate) fn set_hold(&mut self, hold: Hold, on: bool) {
        self.policy.set_hold(hold, on);
        self.engine
            .held
            .store(self.policy.is_held(), Ordering::Relaxed);
        if on {
            self.close_stream();
            session::set_active(false);
        } else {
            self.poll();
        }
    }

    /// iOS may never say an interruption is over, for example if the app was in
    /// the background when it ended, so coming back has to count as the end.
    #[cfg(target_os = "ios")]
    pub(crate) fn end_interruption(&mut self) {
        session::clear_interrupted();
        self.set_hold(Hold::Interrupted, false);
    }

    /// Whether sound can currently reach an output device. Stays `true` while the
    /// app is in the background, and turns `false` if the device goes away, until
    /// it or another one is found.
    pub fn is_available(&self) -> bool {
        self.policy.available
    }

    /// Chooses how the game's sound gets along with other apps on iOS. The default
    /// is [`AudioSession::Ambient`], which lets the player's music keep playing.
    /// Call it in `load` to set it before any sound plays. It only records the
    /// choice on other platforms.
    pub fn set_session(&mut self, session: AudioSession) {
        self.session = session;
        session::apply(session);
    }

    /// The choice made with [`set_session`](Audio::set_session).
    pub fn session(&self) -> AudioSession {
        self.session
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
        Source::new(data, self.engine.clone(), self.master.clone())
    }

    /// Master volume applied to every source (`love.audio.setVolume`).
    pub fn set_volume(&self, volume: f32) {
        store_f32(&self.master, volume.max(0.0));
    }

    /// The master volume.
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
            Arc::default(),
            Arc::new(AtomicU32::new(1f32.to_bits())),
        )
    }

    /// A stream's mixer that the test reads from, without any device.
    fn stream() -> (rodio::mixer::Mixer, rodio::mixer::MixerSource) {
        rodio::mixer::mixer(NonZero::new(1).unwrap(), NonZero::new(10).unwrap())
    }

    /// Reads like a stream does, where an empty mixer means silence.
    fn take(out: &mut rodio::mixer::MixerSource, n: usize) -> Vec<f32> {
        (0..n)
            .map(|_| out.next().map_or(0.0, |v| v as f32))
            .collect()
    }

    #[test]
    fn playing_sounds_carry_on_in_a_new_stream() {
        let s = source(vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6], 1);
        let (mixer, mut out) = stream();
        s.engine.attach(mixer);
        s.play();
        assert_eq!(take(&mut out, 2), vec![0.1, 0.2]);

        // The device went away, and a new one was found.
        s.engine.detach();
        drop(out);
        assert!(s.is_playing());
        let (mixer, mut out) = stream();
        s.engine.attach(mixer);
        assert_eq!(take(&mut out, 4), vec![0.3, 0.4, 0.5, 0.6]);
    }

    #[test]
    fn stopped_and_finished_sounds_stay_stopped_in_a_new_stream() {
        let finished = source(vec![0.1], 1);
        let stopped = source(vec![0.2, 0.2], 1);
        stopped.engine.attach(stream().0);
        stopped.play();
        stopped.stop();
        let (mixer, mut out) = stream();
        finished.engine.attach(mixer);
        finished.play();
        assert_eq!(take(&mut out, 2), vec![0.1, 0.0]);
        assert!(!finished.is_playing());

        for sound in [&finished, &stopped] {
            sound.engine.detach();
            let (mixer, mut out) = stream();
            sound.engine.attach(mixer);
            assert_eq!(take(&mut out, 1), vec![0.0]);
        }
    }

    #[test]
    fn paused_sounds_stay_paused_in_a_new_stream() {
        let s = source(vec![0.1, 0.2, 0.3], 1);
        let (mixer, mut out) = stream();
        s.engine.attach(mixer);
        s.play();
        assert_eq!(take(&mut out, 1), vec![0.1]);
        s.pause();
        assert_eq!(take(&mut out, 1), vec![0.0]);

        s.engine.detach();
        let (mixer, mut out) = stream();
        s.engine.attach(mixer);
        assert_eq!(take(&mut out, 1), vec![0.0]);
        s.play();
        assert_eq!(take(&mut out, 2), vec![0.2, 0.3]);
    }

    #[test]
    fn the_old_voice_stops_when_a_sound_moves() {
        let s = source(vec![0.5; 4], 1);
        let (old_mixer, mut old_out) = stream();
        s.engine.attach(old_mixer);
        s.play();
        assert_eq!(take(&mut old_out, 1), vec![0.5]);
        let (mixer, _out) = stream();
        s.engine.attach(mixer);
        assert_eq!(
            take(&mut old_out, 1),
            vec![0.0],
            "only the new stream plays"
        );
    }

    #[test]
    fn without_a_device_only_looping_sounds_wait_for_one() {
        let one_shot = source(vec![0.1], 1);
        one_shot.play();
        assert!(!one_shot.is_playing(), "as it always was without a device");

        let looping = source(vec![0.1, 0.2], 1);
        looping.set_looping(true);
        looping.play();
        assert!(looping.is_playing());
        let (mixer, mut out) = stream();
        looping.engine.attach(mixer);
        assert_eq!(take(&mut out, 3), vec![0.1, 0.2, 0.1]);
    }

    #[test]
    fn while_held_any_sound_waits_for_the_stream() {
        let s = source(vec![0.1, 0.2], 1);
        s.engine.held.store(true, Ordering::Relaxed);
        s.play();
        assert!(s.is_playing());
        s.engine.held.store(false, Ordering::Relaxed);
        let (mixer, mut out) = stream();
        s.engine.attach(mixer);
        assert_eq!(take(&mut out, 2), vec![0.1, 0.2]);
    }

    #[test]
    fn playing_twice_makes_one_voice() {
        let s = source(vec![0.25, 0.25], 1);
        let (mixer, mut out) = stream();
        s.engine.attach(mixer);
        s.play();
        s.stop();
        s.play();
        assert_eq!(lock(&s.engine.playing).len(), 1);
        assert_eq!(take(&mut out, 1), vec![0.25], "not doubled");
    }

    #[test]
    fn a_held_stream_stays_closed_until_every_hold_ends() {
        let now = Instant::now();
        let mut p = StreamPolicy::new();
        assert!(p.wants_open(now));
        p.set_hold(Hold::Background, true);
        p.set_hold(Hold::Paused, true);
        assert!(!p.wants_open(now));
        p.set_hold(Hold::Background, false);
        assert!(!p.wants_open(now));
        p.set_hold(Hold::Paused, false);
        assert!(p.wants_open(now));
    }

    #[test]
    fn opening_retries_back_off_up_to_a_limit() {
        let now = Instant::now();
        let mut p = StreamPolicy::new();
        assert!(p.search_all_devices());
        p.open_failed(now);
        assert!(!p.available);
        assert!(!p.wants_open(now));
        assert!(!p.search_all_devices());
        assert!(p.wants_open(now + RETRY_MIN));
        for _ in 0..10 {
            p.open_failed(now);
        }
        assert!(p.wants_open(now + RETRY_MAX));
        assert!(!p.wants_open(now + RETRY_MAX - Duration::from_millis(1)));
    }

    #[test]
    fn a_stream_that_was_fine_is_reopened_at_once() {
        let now = Instant::now();
        let mut p = StreamPolicy::new();
        p.opened(now);
        assert!(p.available);
        let later = now + STREAM_STABLE;
        p.lost(later);
        assert!(!p.available);
        assert!(p.wants_open(later));
    }

    #[test]
    fn a_stream_that_keeps_failing_is_not_reopened_every_frame() {
        let now = Instant::now();
        let mut p = StreamPolicy::new();
        p.opened(now);
        p.lost(now);
        assert!(!p.wants_open(now));
        assert!(p.wants_open(now + RETRY_MIN));
    }

    #[test]
    fn coming_back_tries_again_right_away() {
        let now = Instant::now();
        let mut p = StreamPolicy::new();
        p.open_failed(now);
        p.set_hold(Hold::Background, true);
        p.set_hold(Hold::Background, false);
        assert!(p.wants_open(now));
    }

    /// Uses the real device, so it does nothing on a machine without one.
    #[test]
    fn a_real_stream_survives_closing_and_failing() {
        let mut audio = Audio::new();
        if !audio.is_available() {
            return;
        }
        let silence = SoundData::from_samples(44_100, 1, vec![0.0; 44_100 * 60]);
        let source = audio.new_source_from_data(silence);
        source.play();
        let wait_for_progress = |audio: &mut Audio, source: &Source| {
            let before = source.tell();
            for _ in 0..200 {
                audio.poll();
                std::thread::sleep(Duration::from_millis(10));
                if source.tell() > before {
                    return true;
                }
            }
            false
        };
        assert!(wait_for_progress(&mut audio, &source));

        audio.set_hold(Hold::Background, true);
        assert!(audio.sink.is_none());
        assert!(audio.is_available(), "it's only closed on purpose");
        let position = source.tell();
        std::thread::sleep(Duration::from_millis(100));
        audio.poll();
        assert!(audio.sink.is_none(), "stays closed while held");
        assert_eq!(source.tell(), position, "no time passes in the background");
        audio.set_hold(Hold::Background, false);
        assert!(audio.sink.is_some());
        assert!(wait_for_progress(&mut audio, &source));

        // What the audio thread does when a device goes away.
        audio.failed.store(true, Ordering::Release);
        audio.poll();
        assert!(source.is_playing());
        assert!(wait_for_progress(&mut audio, &source));
        assert!(audio.is_available());
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
