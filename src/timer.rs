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
    /// When the last frame was meant to start, which is what the next one is
    /// timed from. A sleep always ends a little late, and timing each frame from
    /// when the last one really started would add that up: 22 frames a second
    /// where 20 were asked for.
    slot: Option<Instant>,
}

/// How much earlier than the display's refresh a frame paced by it starts, so
/// it's ready for the refresh it's meant for rather than just missing it.
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
            slot: None,
        }
    }

    /// A frame is starting at `now`. Under a cap it takes the slot it was due in,
    /// so the frames stay evenly spaced however late each sleep ended. A frame
    /// that's early (input came in) or more than a frame late starts the spacing
    /// over from itself.
    pub(crate) fn frame_started(&mut self, now: Instant) {
        let due = self.max_fps.zip(self.slot).map(|(fps, slot)| (slot + Duration::from_secs_f32(1.0 / fps), fps));
        self.slot = Some(match due {
            Some((due, fps)) if now >= due && now - due < Duration::from_secs_f32(1.0 / fps) => due,
            _ => now,
        });
    }

    /// When the frame after one that started at `last` may start, or `None`
    /// when it may start right away. `refresh` is the display's rate, for a loop
    /// that should wait for its next refresh even without a cap.
    pub(crate) fn next_frame(&self, last: Instant, now: Instant, refresh: Option<f32>) -> Option<Instant> {
        let due = match (self.max_fps, refresh) {
            // A cap below the display's rate sets the pace.
            (Some(cap), refresh) if refresh.is_none_or(|refresh| cap < refresh) => {
                self.slot.unwrap_or(last) + Duration::from_secs_f32(1.0 / cap)
            }
            (_, Some(refresh)) => last + Duration::from_secs_f32(1.0 / refresh).saturating_sub(VSYNC_SLACK),
            (_, None) => return None,
        };
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
        t.frame_started(last);
        let due = t.next_frame(last, last, None).expect("the next frame isn't due yet");
        let wait = (due - last).as_secs_f32();
        assert!((wait - 0.05).abs() < 1e-4, "waits {wait} s");
        // Due, then late: draw now.
        assert_eq!(t.next_frame(last, due, None), None);
        assert_eq!(t.next_frame(last, last + Duration::from_secs(1), None), None);
    }

    #[test]
    fn late_sleeps_dont_slow_a_capped_loop_down() {
        let mut t = Timer::new(0.25);
        t.set_max_fps(Some(20.0));
        let start = Instant::now();
        let late = Duration::from_millis(2);
        let mut now = start;
        t.frame_started(now);
        for _ in 0..20 {
            // Every sleep ends 2 ms after the frame was due.
            now = t.next_frame(now, now, None).unwrap() + late;
            t.frame_started(now);
        }
        let took = (now - start).as_secs_f32();
        assert!((took - 1.002).abs() < 1e-3, "20 frames took {took} s");
    }

    #[test]
    fn an_early_or_very_late_frame_restarts_the_spacing() {
        let mut t = Timer::new(0.25);
        t.set_max_fps(Some(20.0));
        let start = Instant::now();
        t.frame_started(start);
        // Input ran a frame 10 ms in: the next one is a full interval after it.
        let early = start + Duration::from_millis(10);
        t.frame_started(early);
        assert_eq!(t.next_frame(early, early, None), Some(early + Duration::from_secs_f32(0.05)));
        // A frame that took a long time doesn't leave a backlog to catch up on.
        let late = early + Duration::from_millis(400);
        t.frame_started(late);
        assert_eq!(t.next_frame(late, late, None), Some(late + Duration::from_secs_f32(0.05)));
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
