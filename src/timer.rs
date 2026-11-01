//! Frame timing (the `love.timer` equivalent).

use std::time::{Duration, Instant};

/// Frame timing information. Reach it through `ctx.timer`.
pub struct Timer {
    start: Instant,
    last: Instant,
    dt: f32,
    max_dt: f32,
    frames: u64,
    fps: f32,
    fps_window_start: Instant,
    fps_window_frames: u32,
    average_dt: f32,
}

impl Timer {
    pub(crate) fn new(max_dt: f32) -> Self {
        let now = Instant::now();
        Timer {
            start: now,
            last: now,
            dt: 0.0,
            max_dt,
            frames: 0,
            fps: 0.0,
            fps_window_start: now,
            fps_window_frames: 0,
            average_dt: 0.0,
        }
    }

    /// Advances to a new frame and returns the clamped delta time.
    pub(crate) fn step(&mut self) -> f32 {
        let now = Instant::now();
        let raw = (now - self.last).as_secs_f32();
        self.last = now;
        self.dt = raw.min(self.max_dt);
        self.frames += 1;
        self.fps_window_frames += 1;
        let window = now - self.fps_window_start;
        if window >= Duration::from_secs(1) {
            let secs = window.as_secs_f32();
            self.fps = self.fps_window_frames as f32 / secs;
            self.average_dt = secs / self.fps_window_frames as f32;
            self.fps_window_frames = 0;
            self.fps_window_start = now;
        }
        self.dt
    }

    /// Seconds since the previous frame (same value passed to `update`).
    pub fn delta(&self) -> f32 {
        self.dt
    }

    /// Frames per second, averaged over the last second.
    pub fn fps(&self) -> f32 {
        self.fps
    }

    /// Average frame time over the last second, in seconds.
    pub fn average_delta(&self) -> f32 {
        self.average_dt
    }

    /// Seconds since the game started.
    pub fn time(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }

    /// Number of frames rendered so far.
    pub fn frame_count(&self) -> u64 {
        self.frames
    }
}
