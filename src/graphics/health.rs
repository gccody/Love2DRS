//! Keeps GPU errors from taking the game down.
//!
//! By default wgpu panics on any error nothing else caught. Here they're counted
//! instead, and only end the game, with a reported error, if they don't go away.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Frames in a row with errors, about a second's worth, before the game stops.
const MAX_BAD_FRAMES: u32 = 60;
/// How often to try to get a lost window surface back.
const SURFACE_RETRY: Duration = Duration::from_millis(100);
/// How long to keep trying before the game stops.
const SURFACE_GIVE_UP: Duration = Duration::from_secs(5);

#[derive(Default)]
struct State {
    device_lost: Option<String>,
    first_error: Option<String>,
    out_of_memory: bool,
    bad_frames: u32,
}

/// What came of the frame that just ended.
#[derive(Debug, Default, PartialEq)]
pub(super) struct FrameHealth {
    /// The game can't go on, for this reason.
    pub fatal: Option<String>,
    /// The GPU ran out of memory this frame.
    pub out_of_memory: bool,
}

/// Collects GPU errors, which can arrive from any thread.
#[derive(Default)]
pub(super) struct GpuHealth(Mutex<State>);

impl GpuHealth {
    /// Takes over a device's error reporting.
    pub(super) fn watch(device: &wgpu::Device) -> Arc<GpuHealth> {
        let health = Arc::new(GpuHealth::default());
        let errors = health.clone();
        device.on_uncaptured_error(Arc::new(move |error: wgpu::Error| {
            let out_of_memory = matches!(error, wgpu::Error::OutOfMemory { .. });
            errors.record_error(error.to_string(), out_of_memory);
        }));
        let lost = health.clone();
        device.set_device_lost_callback(move |reason, message| {
            // `Destroyed` only comes from the device being shut down on purpose.
            if reason != wgpu::DeviceLostReason::Destroyed {
                lost.record_device_lost(message);
            }
        });
        health
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        // Everything here stays valid if a holder panicked.
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn record_error(&self, message: String, out_of_memory: bool) {
        let mut state = self.lock();
        state.out_of_memory |= out_of_memory;
        if state.first_error.is_none() {
            // Later ones in a row are usually the same one again.
            log::error!("GPU error: {message}");
            state.first_error = Some(message);
        }
    }

    fn record_device_lost(&self, message: String) {
        log::error!("the GPU device was lost: {message}");
        self.lock().device_lost = Some(message);
    }

    /// Call once per frame. A device that's gone stops the game at once, because
    /// everything on the GPU went with it and the game holds those images and
    /// shaders itself. Other errors have to last for [`MAX_BAD_FRAMES`].
    pub(super) fn end_frame(&self) -> FrameHealth {
        let mut state = self.lock();
        let out_of_memory = std::mem::take(&mut state.out_of_memory);
        if let Some(message) = &state.device_lost {
            return FrameHealth {
                fatal: Some(format!("the GPU device was lost: {message}")),
                out_of_memory,
            };
        }
        if state.first_error.is_none() {
            state.bad_frames = 0;
            return FrameHealth {
                fatal: None,
                out_of_memory,
            };
        }
        state.bad_frames += 1;
        let fatal = (state.bad_frames >= MAX_BAD_FRAMES).then(|| {
            let message = state.first_error.as_deref().unwrap_or_default();
            format!(
                "GPU errors on {} frames in a row, the first being: {message}",
                state.bad_frames
            )
        });
        if fatal.is_none() {
            state.first_error = None;
        }
        FrameHealth {
            fatal,
            out_of_memory,
        }
    }
}

/// When to retry a window surface that was lost, and when to stop trying.
#[derive(Default)]
pub(super) struct SurfaceRecovery {
    lost_since: Option<Instant>,
    next_try: Option<Instant>,
}

impl SurfaceRecovery {
    /// The surface is lost or couldn't be configured.
    pub(super) fn lost(&mut self, now: Instant) {
        self.lost_since.get_or_insert(now);
    }

    /// A frame could be drawn to the surface, or there's no surface to worry about.
    pub(super) fn ok(&mut self) {
        *self = SurfaceRecovery::default();
    }

    /// Whether to try recreating the surface now.
    pub(super) fn should_retry(&mut self, now: Instant) -> bool {
        if self.lost_since.is_none() || self.next_try.is_some_and(|t| now < t) {
            return false;
        }
        self.next_try = Some(now + SURFACE_RETRY);
        true
    }

    pub(super) fn gave_up(&self, now: Instant) -> bool {
        self.lost_since
            .is_some_and(|since| now.duration_since(since) >= SURFACE_GIVE_UP)
    }
}

/// Runs `f` and returns any GPU error it causes, instead of leaving it for
/// [`GpuHealth`].
pub(super) fn catch_errors<T>(device: &wgpu::Device, f: impl FnOnce() -> T) -> (T, Option<String>) {
    let memory = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let result = f();
    // Scopes come off in the opposite order.
    let errors = [
        pollster::block_on(validation.pop()),
        pollster::block_on(internal.pop()),
        pollster::block_on(memory.pop()),
    ];
    let error = errors.into_iter().flatten().next();
    (result, error.map(|e| e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_passing_error_is_forgiven() {
        let health = GpuHealth::default();
        health.record_error("oops".into(), false);
        assert_eq!(health.end_frame(), FrameHealth::default());
        assert_eq!(health.lock().bad_frames, 1);
        assert_eq!(health.end_frame(), FrameHealth::default());
        assert_eq!(health.lock().bad_frames, 0, "a clean frame breaks the run");
    }

    #[test]
    fn errors_that_keep_coming_are_fatal() {
        let health = GpuHealth::default();
        for frame in 1..MAX_BAD_FRAMES {
            health.record_error(format!("error on frame {frame}"), false);
            assert_eq!(health.end_frame().fatal, None);
        }
        health.record_error("the last one".into(), false);
        let fatal = health.end_frame().fatal.unwrap();
        assert!(fatal.contains("the last one"), "{fatal}");
    }

    #[test]
    fn out_of_memory_is_reported_once_per_frame() {
        let health = GpuHealth::default();
        health.record_error("no memory".into(), true);
        assert!(health.end_frame().out_of_memory);
        assert!(!health.end_frame().out_of_memory);
    }

    #[test]
    fn a_lost_device_is_fatal_at_once() {
        let health = GpuHealth::default();
        health.record_device_lost("driver reset".into());
        let fatal = health.end_frame().fatal.unwrap();
        assert!(fatal.contains("driver reset"), "{fatal}");
    }

    #[test]
    fn a_lost_surface_is_retried_then_given_up_on() {
        let start = Instant::now();
        let mut surface = SurfaceRecovery::default();
        assert!(!surface.should_retry(start), "nothing is wrong yet");
        assert!(!surface.gave_up(start));

        surface.lost(start);
        assert!(surface.should_retry(start));
        assert!(!surface.should_retry(start), "too soon");
        assert!(surface.should_retry(start + SURFACE_RETRY));

        // Being lost again doesn't restart the clock.
        surface.lost(start + SURFACE_RETRY);
        assert!(!surface.gave_up(start + SURFACE_GIVE_UP - SURFACE_RETRY));
        assert!(surface.gave_up(start + SURFACE_GIVE_UP));

        surface.ok();
        assert!(!surface.gave_up(start + SURFACE_GIVE_UP));
    }

    /// Needs a GPU, so it does nothing on a machine without one.
    #[test]
    fn uncaptured_errors_are_collected_instead_of_panicking() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&Default::default()));
        let Ok(adapter) = adapter else {
            return;
        };
        let Ok((device, _queue)) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
        else {
            return;
        };
        let bad_buffer = wgpu::BufferDescriptor {
            label: None,
            size: 4,
            // Not allowed together.
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::MAP_WRITE,
            mapped_at_creation: false,
        };

        let (_, caught) = catch_errors(&device, || device.create_buffer(&bad_buffer));
        assert!(caught.is_some(), "the scope should catch it");

        let health = GpuHealth::watch(&device);
        let _buffer = device.create_buffer(&bad_buffer);
        assert!(health.lock().first_error.is_some());
        assert_eq!(health.end_frame().fatal, None);
    }
}
