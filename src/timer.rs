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
    max_fps: Option<f32>,
}

/// How much earlier than the cap allows a frame starts, so it's ready for the
/// display refresh it's meant for rather than just missing it.
const VSYNC_SLACK: Duration = Duration::from_millis(3);

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
            max_fps: None,
        }
    }

    /// When the frame after one that started at `last` may start, or `None`
    /// when it may start right away. `refresh` is the display's rate, for a loop
    /// that should wait for its next refresh even without a cap.
    pub(crate) fn next_frame(&self, last: Instant, now: Instant, refresh: Option<f32>) -> Option<Instant> {
        let fps = match (self.max_fps, refresh) {
            (Some(cap), Some(refresh)) => cap.min(refresh),
            (cap, refresh) => cap.or(refresh)?,
        };
        let due = last + Duration::from_secs_f32(1.0 / fps).saturating_sub(VSYNC_SLACK);
        (now < due).then_some(due)
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

    /// Draws at most `fps` frames a second, or as many as the display shows
    /// with `None` (the default). The loop sleeps between frames, which saves
    /// a lot of battery while a game has little going on.
    ///
    /// Input never waits for the next frame: a key, click, touch, controller
    /// press or window change runs one at once, so a game can lift the cap as
    /// soon as the player does something. Can change every frame. Values below
    /// 1 count as 1.
    pub fn set_max_fps(&mut self, fps: Option<f32>) {
        self.max_fps = fps.filter(|f| !f.is_nan()).map(|f| f.max(1.0));
    }

    /// The cap set with [`Timer::set_max_fps`].
    pub fn max_fps(&self) -> Option<f32> {
        self.max_fps
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_cap_means_draw_now() {
        let t = Timer::new(0.25);
        let now = Instant::now();
        assert_eq!(t.next_frame(now, now, None), None);
    }

    #[test]
    fn the_display_paces_an_uncapped_loop() {
        let mut t = Timer::new(0.25);
        let last = Instant::now();
        let wait = (t.next_frame(last, last, Some(60.0)).unwrap() - last).as_secs_f32();
        assert!((wait - (1.0 / 60.0 - 0.003)).abs() < 1e-4, "waits {wait} s");
        // A cap below the display's rate wins, one above it doesn't.
        t.set_max_fps(Some(20.0));
        assert!(t.next_frame(last, last, Some(60.0)).unwrap() - last > Duration::from_millis(40));
        t.set_max_fps(Some(240.0));
        assert!(t.next_frame(last, last, Some(60.0)).unwrap() - last > Duration::from_millis(13));
    }

    #[test]
    fn a_cap_waits_until_the_frame_is_due() {
        let mut t = Timer::new(0.25);
        t.set_max_fps(Some(20.0));
        let last = Instant::now();
        let due = t.next_frame(last, last, None).expect("the next frame isn't due yet");
        let wait = (due - last).as_secs_f32();
        assert!((wait - 0.047).abs() < 1e-4, "waits {wait} s");
        // Due, then late: draw now.
        assert_eq!(t.next_frame(last, due, None), None);
        assert_eq!(t.next_frame(last, last + Duration::from_secs(1), None), None);
    }

    #[test]
    fn tiny_caps_are_clamped() {
        let mut t = Timer::new(0.25);
        t.set_max_fps(Some(0.0));
        assert_eq!(t.max_fps(), Some(1.0));
        t.set_max_fps(Some(f32::NAN));
        assert_eq!(t.max_fps(), None);
    }
}
