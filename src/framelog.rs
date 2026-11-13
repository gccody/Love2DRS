//! `LOVE2DRS_FRAME_LOG=1`: once a second, print where the main thread's time went.
//!
//! A game that should be gentle on the battery has two numbers to watch: how often
//! the loop wakes up, and how long it then stays busy. This prints both, with the
//! busy time split by phase, so a change can be judged without a profiler:
//!
//! ```text
//! love2drs: 240 frames, 240 wakeups (0 by the OS) in 1.00 s, busy 5.4% | ms per frame: input 0.00,
//! update 0.01, draw 0.05, acquire 3.79, encode 0.06, submit 0.10, present 0.10 | 2 passes, 28 draws
//! per frame | gpu ms per pass: 0.142 + 0.120 = 6.3% busy
//! ```
//!
//! "By the OS" counts waits that a message cut short, such as the mouse moving. The
//! GPU times need a driver with timestamp queries, and are left out without one.
//!
//! Nothing can set an environment variable for an app on a phone, so there an
//! empty file named `love2drs-frame-log` in the save directory turns it on too.
//! The lines go to logcat on Android and to the system log on iOS.
//!
//! Costs nothing when it's off.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// A part of the frame whose time is counted.
#[derive(Clone, Copy)]
pub(crate) enum Phase {
    /// Reading controllers.
    Input,
    /// `Game::update`.
    Update,
    /// `Game::draw`, which only records what to draw.
    Draw,
    /// Waiting for the window's next image. With vsync this is where the loop
    /// waits for the display, and that wait isn't counted as busy.
    Acquire,
    /// Uploading the frame and turning it into GPU commands.
    Encode,
    /// Handing the commands to the driver.
    Submit,
    /// Presenting the image. Some drivers wait for the display here instead.
    Present,
}

const PHASES: usize = 7;
const NAMES: [&str; PHASES] = ["input", "update", "draw", "acquire", "encode", "submit", "present"];

static NANOS: [AtomicU64; PHASES] = [const { AtomicU64::new(0) }; PHASES];

/// How many of a frame's render passes are timed on the GPU.
pub(crate) const GPU_PASSES: usize = 8;
/// GPU time per pass, in the order the passes ran, and how many frames it covers.
static GPU_NANOS: [AtomicU64; GPU_PASSES] = [const { AtomicU64::new(0) }; GPU_PASSES];
static GPU_FRAMES: AtomicU64 = AtomicU64::new(0);

/// The GPU's time for each pass of one frame.
pub(crate) fn gpu_frame(pass_nanos: &[u64]) {
    for (total, n) in GPU_NANOS.iter().zip(pass_nanos) {
        total.fetch_add(*n, Ordering::Relaxed);
    }
    GPU_FRAMES.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var("LOVE2DRS_FRAME_LOG").is_ok_and(|v| !v.is_empty() && v != "0") || asked_for_by_file()
    })
}

#[cfg(any(target_os = "android", target_os = "ios"))]
fn asked_for_by_file() -> bool {
    crate::filesystem::save_dir().join("love2drs-frame-log").exists()
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn asked_for_by_file() -> bool {
    false
}

/// Prints a line where it can be read: stderr, which Android sends to logcat. An
/// iOS app's stderr goes nowhere unless a debugger started it, so there it's the
/// system log (`log stream`, Console, Xcode).
#[cfg(not(target_os = "ios"))]
fn print(line: &str) {
    eprintln!("{line}");
}

#[cfg(target_os = "ios")]
fn print(line: &str) {
    use objc2_foundation::NSString;

    unsafe extern "C" {
        fn NSLog(format: *const NSString, ...);
    }
    let (format, line) = (NSString::from_str("%@"), NSString::from_str(line));
    // SAFETY: the format takes one object, which is what follows it.
    unsafe { NSLog(&*format, &*line as *const NSString) };
}

/// Runs `f`, counting how long it took as `phase`.
#[inline]
pub(crate) fn time<T>(phase: Phase, f: impl FnOnce() -> T) -> T {
    if !enabled() {
        return f();
    }
    let start = Instant::now();
    let out = f();
    add(phase, start.elapsed());
    out
}

/// When a phase that can't be wrapped in a closure started, for [`since`].
#[inline]
pub(crate) fn now() -> Option<Instant> {
    enabled().then(Instant::now)
}

/// Counts the time since `began` as `phase`.
#[inline]
pub(crate) fn since(phase: Phase, began: Option<Instant>) {
    if let Some(began) = began {
        add(phase, began.elapsed());
    }
}

fn add(phase: Phase, elapsed: Duration) {
    NANOS[phase as usize].fetch_add(elapsed.as_nanos() as u64, Ordering::Relaxed);
}

/// Counts the loop's wakeups and frames, and prints the totals every second.
pub(crate) struct FrameLog {
    since: Instant,
    busy: Duration,
    wakeups: u32,
    /// Wakeups that weren't the loop's own doing: a message from the OS.
    woken: u32,
    frames: u32,
    passes: u32,
    draws: u32,
}

impl FrameLog {
    pub fn new() -> Self {
        FrameLog {
            since: Instant::now(),
            busy: Duration::ZERO,
            wakeups: 0,
            woken: 0,
            frames: 0,
            passes: 0,
            draws: 0,
        }
    }

    /// The loop woke up to handle events. `by_os` is for a wait that something cut short.
    #[inline]
    pub fn woke(&mut self, by_os: bool) {
        if enabled() {
            self.wakeups += 1;
            self.woken += u32::from(by_os);
        }
    }

    /// The loop finished handling something it started at `began` (from [`now`]).
    #[inline]
    pub fn done(&mut self, began: Option<Instant>) {
        if let Some(began) = began {
            self.busy += began.elapsed();
        }
    }

    /// A frame was drawn.
    #[inline]
    pub fn frame(&mut self, stats: crate::graphics::FrameStats) {
        if enabled() {
            self.frames += 1;
            self.passes += stats.passes;
            self.draws += stats.draw_calls;
        }
    }

    /// The loop is about to go back to sleep: prints the totals once a second.
    pub fn sleeping(&mut self) {
        if !enabled() {
            return;
        }
        let now = Instant::now();
        let elapsed = now - self.since;
        if elapsed < Duration::from_secs(1) {
            return;
        }
        let nanos: [u64; PHASES] = std::array::from_fn(|i| NANOS[i].swap(0, Ordering::Relaxed));
        // Waiting for the display isn't work.
        let waited = Duration::from_nanos(nanos[Phase::Acquire as usize] + nanos[Phase::Present as usize]);
        let busy = self.busy.saturating_sub(waited);
        let frames = self.frames.max(1);
        let phases: Vec<String> = NAMES
            .iter()
            .zip(nanos)
            .map(|(name, n)| format!("{name} {:.2}", n as f64 / 1e6 / frames as f64))
            .collect();
        let gpu_frames = GPU_FRAMES.swap(0, Ordering::Relaxed);
        let gpu: Vec<f64> = GPU_NANOS
            .iter()
            .map(|n| n.swap(0, Ordering::Relaxed) as f64 / 1e6 / gpu_frames.max(1) as f64)
            .collect();
        let gpu = if gpu_frames == 0 {
            String::new()
        } else {
            let passes: Vec<String> = gpu.iter().filter(|ms| **ms > 0.0).map(|ms| format!("{ms:.3}")).collect();
            // Scaled up from the frames that were timed to all of them.
            let share = gpu.iter().sum::<f64>() * self.frames as f64 / 10.0 / elapsed.as_secs_f64();
            format!(" | gpu ms per pass: {} = {:.1}% busy", passes.join(" + "), share)
        };
        print(&format!(
            "love2drs: {} frames, {} wakeups ({} by the OS) in {:.2} s, busy {:.1}% | ms per frame: {} | {} passes, {} draws per frame{gpu}",
            self.frames,
            self.wakeups,
            self.woken,
            elapsed.as_secs_f32(),
            100.0 * busy.as_secs_f32() / elapsed.as_secs_f32(),
            phases.join(", "),
            self.passes / frames,
            self.draws / frames,
        ));
        *self = FrameLog::new();
        self.since = now;
    }
}
