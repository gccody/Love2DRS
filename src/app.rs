use std::sync::Arc;
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseScrollDelta, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{NamedKey, PhysicalKey};
use winit::window::{Fullscreen, WindowAttributes, WindowId};

use crate::framelog::{self, FrameLog, Phase};
use crate::graphics::Graphics;
use crate::input::gamepad::GamepadEvent;
use crate::input::touch::{MouseAction, MouseFinger};
use crate::input::{
    GamepadAxis, GamepadButton, GamepadId, Gamepads, Key, Keyboard, Mouse, MouseButton, Touch,
    TouchId,
};
use crate::math::vec2;
use crate::platform::MOBILE;
use crate::timer::Timer;
use crate::window::Window;
use crate::{Conf, Error, Result};

/// Everything a game can touch: the `love.*` modules.
pub struct Context {
    /// Drawing, images, fonts, canvases and shaders (`love.graphics`).
    pub graphics: Graphics,
    /// Keyboard state (`love.keyboard`).
    pub keyboard: Keyboard,
    /// Mouse state and cursor control (`love.mouse`).
    pub mouse: Mouse,
    /// Connected controllers and rumble (`love.joystick`).
    pub gamepad: Gamepads,
    /// Fingers on a touchscreen (`love.touch`).
    pub touch: Touch,
    /// Frame timing (`love.timer`).
    pub timer: Timer,
    /// The game window (`love.window`).
    pub window: Window,
    /// Sound loading and playback (`love.audio`). Requires the `audio` feature.
    #[cfg(feature = "audio")]
    pub audio: crate::audio::Audio,
    quit: bool,
}

impl Context {
    /// Asks the game to exit after the current frame (`love.event.quit`).
    /// [`Game::quit`] is still consulted.
    pub fn quit(&mut self) {
        self.quit = true;
    }
}

/// Your game. Every callback has a default empty implementation, so only
/// implement what you need, just like LÖVE's `love.*` callbacks.
#[allow(unused_variables)]
pub trait Game: 'static {
    /// Advance the simulation by `dt` seconds (`love.update`).
    fn update(&mut self, ctx: &mut Context, dt: f32) {}

    /// Draw the frame (`love.draw`). The screen has already been cleared to
    /// the background color and the transform reset.
    fn draw(&mut self, ctx: &mut Context) {}

    /// A key went down. `repeat` is true for auto-repeat events, which are
    /// only delivered after `ctx.keyboard.set_key_repeat(true)`.
    fn key_pressed(&mut self, ctx: &mut Context, key: Key, repeat: bool) {}

    /// A key was released.
    fn key_released(&mut self, ctx: &mut Context, key: Key) {}

    /// Text typed by the user (already composed by the OS keyboard layout / IME).
    fn text_input(&mut self, ctx: &mut Context, text: &str) {}

    /// A mouse button went down at `(x, y)` in logical pixels.
    fn mouse_pressed(&mut self, ctx: &mut Context, x: f32, y: f32, button: MouseButton) {}

    /// A mouse button was released at `(x, y)` in logical pixels.
    fn mouse_released(&mut self, ctx: &mut Context, x: f32, y: f32, button: MouseButton) {}

    /// A held button ended without being let go, so it isn't a click. This only
    /// happens on mobile, where the first finger acts as the left button, when
    /// that finger gets [`touch_cancelled`](Game::touch_cancelled). Mouse-only
    /// games should forget the press here. `mouse_released` won't follow.
    fn mouse_cancelled(&mut self, ctx: &mut Context, x: f32, y: f32, button: MouseButton) {}

    /// The cursor moved to `(x, y)`; `(dx, dy)` is the movement since the last event.
    fn mouse_moved(&mut self, ctx: &mut Context, x: f32, y: f32, dx: f32, dy: f32) {}

    /// Mouse wheel movement; `y > 0` scrolls away from the user.
    fn wheel_moved(&mut self, ctx: &mut Context, x: f32, y: f32) {}

    /// A finger touched the screen at `(x, y)` in logical pixels. Pressure and
    /// other details are in `ctx.touch.get(id)`.
    fn touch_pressed(&mut self, ctx: &mut Context, id: TouchId, x: f32, y: f32) {}

    /// A finger moved to `(x, y)`; `(dx, dy)` is the movement since the last event.
    fn touch_moved(&mut self, ctx: &mut Context, id: TouchId, x: f32, y: f32, dx: f32, dy: f32) {}

    /// A finger left the screen at `(x, y)`. Only sent when the player really
    /// lifted it, so this is a safe place to act on a tap. See
    /// [`touch_cancelled`](Game::touch_cancelled) for the other way a touch ends.
    fn touch_released(&mut self, ctx: &mut Context, id: TouchId, x: f32, y: f32) {}

    /// The OS took a finger away at `(x, y)`, for example for a back gesture, an
    /// incoming call or the notification shade. It also happens to every finger
    /// when the window loses focus or the app is suspended. The player didn't
    /// mean to let go, so forget the touch and don't act on it, as you would for
    /// a drag dropped outside any target.
    fn touch_cancelled(&mut self, ctx: &mut Context, id: TouchId, x: f32, y: f32) {}

    /// A controller was connected. Controllers already plugged in at startup
    /// are announced before the first `update`.
    fn gamepad_added(&mut self, ctx: &mut Context, id: GamepadId) {}

    /// A controller was disconnected. It is already gone from `ctx.gamepad`.
    fn gamepad_removed(&mut self, ctx: &mut Context, id: GamepadId) {}

    /// A controller button went down.
    fn gamepad_pressed(&mut self, ctx: &mut Context, id: GamepadId, button: GamepadButton) {}

    /// A controller button was released.
    fn gamepad_released(&mut self, ctx: &mut Context, id: GamepadId, button: GamepadButton) {}

    /// A stick or trigger moved to `value` (-1..1 for sticks with `y` down,
    /// 0..1 for triggers).
    fn gamepad_axis(&mut self, ctx: &mut Context, id: GamepadId, axis: GamepadAxis, value: f32) {}

    /// The window was resized (logical pixels).
    fn resize(&mut self, ctx: &mut Context, width: f32, height: f32) {}

    /// The window gained (`true`) or lost (`false`) keyboard focus. Held keys and
    /// buttons are released when focus is lost, and fingers get
    /// [`touch_cancelled`](Game::touch_cancelled).
    fn focus(&mut self, ctx: &mut Context, focused: bool) {}

    /// The app went to the background on Android or iOS, or a system overlay
    /// took over. Save progress here: the OS may end a suspended app without
    /// further notice. `update` and `draw` stop until [`resumed`](Game::resumed)
    /// and sounds are silenced. Never called on desktop.
    fn suspended(&mut self, ctx: &mut Context) {}

    /// The app came back after [`suspended`](Game::suspended).
    fn resumed(&mut self, ctx: &mut Context) {}

    /// The OS is short of memory (`love.lowmemory`). Free what you can, or a
    /// suspended app may be ended.
    fn low_memory(&mut self, ctx: &mut Context) {}

    /// Files dropped onto the window.
    fn file_dropped(&mut self, ctx: &mut Context, path: std::path::PathBuf) {}

    /// The user tried to close the window or [`Context::quit`] was called.
    /// Return `true` to abort quitting.
    fn quit(&mut self, ctx: &mut Context) -> bool {
        false
    }
}

/// Opens the window and runs the game until it quits.
///
/// `load` runs once the window and GPU are ready, like `love.load`, and
/// builds your game state.
///
/// On iOS this never returns: an error is printed and ends the process instead.
///
/// ```no_run
/// use love2drs::prelude::*;
///
/// struct Hello;
///
/// impl Game for Hello {
///     fn draw(&mut self, ctx: &mut Context) {
///         ctx.graphics.print("Hello, world!", 400.0, 300.0);
///     }
/// }
///
/// fn main() -> love2drs::Result {
///     love2drs::run(Conf::new("Hello"), |_ctx| Ok(Hello))
/// }
/// ```
pub fn run<G, F>(conf: Conf, load: F) -> Result<()>
where
    G: Game,
    F: FnOnce(&mut Context) -> Result<G>,
{
    #[cfg(not(target_os = "android"))] // android_main! has already done this
    crate::platform::init();
    #[allow(unused_mut)]
    let mut builder = EventLoop::builder();
    #[cfg(target_os = "android")]
    {
        use winit::platform::android::EventLoopBuilderExtAndroid;
        builder.with_android_app(crate::platform::android_app().clone());
    }
    let event_loop = builder.build().map_err(|e| Error::Window(e.to_string()))?;
    // Raw mouse and keyboard reports, which nothing here reads. A gaming mouse sends
    // a thousand a second, and each one would wake the loop up.
    event_loop.listen_device_events(winit::event_loop::DeviceEvents::Never);
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App::<G, F> {
        conf,
        load: Some(load),
        state: None,
        error: None,
        autoshot: AutoShot::from_env(),
        last_frame: Instant::now(),
        wake: false,
        suspended: false,
        pad_used: None,
        occluded: false,
        redraw_asked: false,
        last_draw: Instant::now(),
        draw_next: false,
        refresh: None,
        refresh_checked: None,
        runaway: RunawayGuard::default(),
        mouse_finger: MouseFinger::default(),
        log: FrameLog::new(),
        #[cfg(target_os = "android")]
        android_input: Default::default(),
        #[cfg(target_os = "android")]
        android_pause: PauseDetector::default(),
    };
    event_loop
        .run_app(&mut app)
        .map_err(|e| Error::Window(e.to_string()))?;
    match app.error {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// Ends the event loop. iOS ignores `exit`, as apps aren't meant to quit
/// themselves, so a game that asks to has to end the process.
fn exit_app(event_loop: &ActiveEventLoop) {
    if cfg!(target_os = "ios") {
        std::process::exit(0);
    }
    event_loop.exit();
}

/// Updates the keyboard state and calls the game's key callbacks.
fn key_event<G: Game>(
    ctx: &mut Context,
    game: &mut G,
    key: Key,
    pressed: bool,
    repeat: bool,
    text: Option<&str>,
) {
    if pressed {
        if !repeat || ctx.keyboard.key_repeat {
            ctx.keyboard.press(key);
            game.key_pressed(ctx, key, repeat);
        }
        if ctx.keyboard.text_input
            && let Some(text) = text
            && text.chars().any(|c| !c.is_control())
        {
            game.text_input(ctx, text);
        }
    } else {
        ctx.keyboard.release(key);
        game.key_released(ctx, key);
    }
}

/// What [`poll_gamepads`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PadNews {
    Nothing,
    /// A controller came or went, which isn't the player using one.
    Changed,
    /// A button or stick moved.
    Used,
}

/// Reads controller events and replays them as `Game` callbacks.
fn poll_gamepads<G: Game>(ctx: &mut Context, game: &mut G) -> PadNews {
    ctx.gamepad.poll();
    if ctx.gamepad.events.is_empty() {
        return PadNews::Nothing;
    }
    let mut news = PadNews::Changed;
    let mut events = std::mem::take(&mut ctx.gamepad.events);
    for ev in events.drain(..) {
        if !matches!(ev, GamepadEvent::Added(_) | GamepadEvent::Removed(_)) {
            news = PadNews::Used;
        }
        match ev {
            GamepadEvent::Added(id) => game.gamepad_added(ctx, id),
            GamepadEvent::Removed(id) => game.gamepad_removed(ctx, id),
            GamepadEvent::Pressed(id, b) => game.gamepad_pressed(ctx, id, b),
            GamepadEvent::Released(id, b) => game.gamepad_released(ctx, id, b),
            GamepadEvent::Axis(id, a, v) => game.gamepad_axis(ctx, id, a, v),
        }
    }
    // Hand the (now empty) buffer back so its allocation is reused.
    if ctx.gamepad.events.is_empty() {
        ctx.gamepad.events = events;
    }
    news
}

/// Testing aid:`LOVE2DRS_SCREENSHOT=out.png` saves frame
/// `LOVE2DRS_SCREENSHOT_FRAME` (default 60) and exits. Useful for CI smoke tests.
struct AutoShot {
    path: Option<String>,
    frame: u64,
}

impl AutoShot {
    fn from_env() -> Self {
        AutoShot {
            path: std::env::var("LOVE2DRS_SCREENSHOT")
                .ok()
                .filter(|p| !p.is_empty()),
            frame: std::env::var("LOVE2DRS_SCREENSHOT_FRAME")
                .ok()
                .and_then(|f| f.parse().ok())
                .unwrap_or(60),
        }
    }
}

struct App<G, F> {
    conf: Conf,
    load: Option<F>,
    state: Option<(Context, G)>,
    error: Option<Error>,
    autoshot: AutoShot,
    last_frame: Instant,
    /// Input arrived since the last frame, so the next one runs at once, even
    /// under [`Timer::set_max_fps`](crate::Timer::set_max_fps).
    wake: bool,
    /// The app is in the background, so nothing may be drawn.
    suspended: bool,
    /// When a controller last did something.
    pad_used: Option<Instant>,
    /// The OS says the window can't be seen (macOS and iOS do).
    occluded: bool,
    /// The loop was asked for the next frame, so its next wakeup is that and not
    /// something the OS sent.
    redraw_asked: bool,
    /// When `Game::draw` last ran.
    last_draw: Instant,
    /// Something happened to the window, so the next frame is drawn even if the
    /// window seemed hidden: it may be how it came back.
    draw_next: bool,
    /// The display's refresh rate, and when it was asked for.
    refresh: Option<f32>,
    refresh_checked: Option<Instant>,
    runaway: RunawayGuard,
    /// The finger that acts as the mouse on mobile.
    mouse_finger: MouseFinger,
    /// `LOVE2DRS_FRAME_LOG`: where the loop's time goes.
    log: FrameLog,
    #[cfg(target_os = "android")]
    android_input: crate::input::android::Reader,
    #[cfg(target_os = "android")]
    android_pause: PauseDetector,
}

/// Notices when Android pauses the app without taking its window, as it does
/// behind a permission dialog. winit keeps that to itself and `suspended` doesn't
/// fire, but it also holds back redraws until the app resumes. A redraw that was
/// asked for comes before the loop's next pass, so asking for another with the
/// last one undelivered means the app is paused.
///
/// Only a pass that really asks counts. One that goes back to sleep until a frame
/// is due (under a frame cap, say) hasn't asked for anything, and the pass after
/// it finding no frame drawn is no sign of a pause.
#[cfg(any(target_os = "android", test))]
#[derive(Default)]
struct PauseDetector {
    redraw_requested: bool,
    paused: bool,
}

#[cfg(any(target_os = "android", test))]
impl PauseDetector {
    /// The loop is about to ask for a redraw. Returns whether this is the moment
    /// the app turned out to be paused.
    fn asking_for_redraw(&mut self) -> bool {
        let just_paused = self.redraw_requested && !self.paused;
        self.paused |= just_paused;
        self.redraw_requested = true;
        just_paused
    }

    /// A frame is running. Returns whether the app was paused until now.
    fn frame_drawn(&mut self) -> bool {
        self.redraw_requested = false;
        std::mem::take(&mut self.paused)
    }

    fn is_paused(&self) -> bool {
        self.paused
    }

    /// The window came or went, which says nothing about redraws asked for before.
    fn forget_redraw(&mut self) {
        self.redraw_requested = false;
    }
}

/// Frame interval while the window is minimized or can't be seen (30 fps).
const HIDDEN_FRAME_TIME: Duration = Duration::from_nanos(1_000_000_000 / 30);
/// How often a window that can't be seen is drawn anyway, to find out when it can
/// be seen again even if the OS never says so.
const HIDDEN_DRAW_TIME: Duration = Duration::from_millis(250);
/// How often to ask which display the window is on and how fast it is.
const REFRESH_CHECK_TIME: Duration = Duration::from_secs(2);
/// The frame rate to hold a loop to when it runs away and the display's rate
/// isn't known.
const UNKNOWN_REFRESH: f32 = 250.0;
/// Controllers don't wake the event loop, so while a frame cap has it asleep
/// they're looked at this often.
const PAD_POLL_TIME: Duration = Duration::from_millis(16);
/// But only while one is in use. A controller nobody has touched for this long is
/// looked at less often, as waking up 60 times a second for it would undo much of
/// what the cap saves. Plenty of PCs have a device that calls itself a controller
/// and never sends a thing.
const PAD_IN_USE_TIME: Duration = Duration::from_secs(60);
/// How often a controller that isn't in use is looked at, which is the longest
/// its first press can take to be noticed.
const IDLE_PAD_POLL_TIME: Duration = Duration::from_millis(50);

/// The display's refresh rate, when the loop should wait for the next refresh
/// even without a frame cap. Only on iOS: winit's `ControlFlow::Poll` there
/// fires the run loop again at once, so the main thread spun between frames and
/// kept a whole core busy (measured on an iPhone: 98% CPU at 60 fps, and 100
/// times fewer instructions once it waited). Elsewhere presenting a frame already
/// waits for vsync, and coarse timers would make an extra wait miss refreshes.
#[cfg(target_os = "ios")]
fn display_pacing(ctx: &Context, refresh: Option<f32>) -> Option<f32> {
    ctx.graphics.vsync().then_some(refresh.unwrap_or(60.0))
}

#[cfg(not(target_os = "ios"))]
fn display_pacing(_ctx: &Context, _refresh: Option<f32>) -> Option<f32> {
    None
}

/// What the loop does between asking for the next frame and being handed it.
///
/// Polling, except on macOS and iOS, where winit polls with a timer that repeats
/// every 0.1 µs and the redraw wakes the loop by itself anyway. So there the loop
/// waits, with a timer [`REDRAW_PATIENCE`] away in case the redraw never comes. A
/// running game moves it on every frame before it can fire.
///
/// On macOS the timer is set only once, so after a frame has waited for the
/// display it's a whole frame behind, and CoreFoundation walks it forward an
/// interval at a time: some 200,000 steps a frame, which on an M1 at 60 fps took
/// the main thread from 8.5% of a core to 10.6%.
///
/// On iOS the redraw comes with UIKit's next display update, which a frame paced
/// by the display asks for [`VSYNC_SLACK`](crate::timer) early. Polling until then
/// kept the loop spinning for those 3 ms of every frame: 3,000 passes a second for
/// 60 frames in the simulator, and busy 14 to 20% of the time where the frames
/// themselves took 6%.
#[cfg(any(target_os = "macos", target_os = "ios"))]
fn until_the_redraw(now: Instant) -> ControlFlow {
    ControlFlow::WaitUntil(now + REDRAW_PATIENCE)
}

#[cfg(not(any(target_os = "macos", target_os = "ios")))]
fn until_the_redraw(_now: Instant) -> ControlFlow {
    ControlFlow::Poll
}

/// How long macOS and iOS are given to deliver a redraw before the loop asks again.
#[cfg(any(target_os = "macos", target_os = "ios"))]
const REDRAW_PATIENCE: Duration = Duration::from_millis(100);

/// Holds the loop to the display's rate when presenting doesn't.
///
/// Presenting with vsync is what paces the loop, and sometimes nothing waits: a
/// driver told to ignore vsync, a compositor that's stopped showing the window.
/// The loop would then draw thousands of frames a second that nobody can see.
///
/// A frame that waited for the display took most of a refresh. A few in a row
/// that didn't are normal, while a new swapchain's queue fills up, so it takes
/// [`RUNAWAY_FRAMES`] of them before the loop is paced by a timer instead. That
/// would keep the queue from ever filling again, so every [`RUNAWAY_HOLD`] the
/// timer lets go for long enough to see whether presenting waits by now.
#[derive(Default)]
struct RunawayGuard {
    /// Frames in a row that came back in under half a refresh.
    fast_frames: u32,
    /// The frame that was last counted, by when it started.
    counted: Option<Instant>,
    holding_until: Option<Instant>,
}

/// More than any swapchain queues up.
const RUNAWAY_FRAMES: u32 = 8;
const RUNAWAY_HOLD: Duration = Duration::from_secs(1);

impl RunawayGuard {
    /// When the frame after the one that started at `last` may start, or `None`
    /// for at once.
    fn next_frame(&mut self, last: Instant, now: Instant, refresh: Option<f32>) -> Option<Instant> {
        let period = Duration::from_secs_f32(1.0 / refresh.unwrap_or(UNKNOWN_REFRESH));
        let due = last + period;
        if let Some(until) = self.holding_until {
            if now < until {
                return (now < due).then_some(due);
            }
            *self = RunawayGuard::default();
        }
        if self.counted != Some(last) {
            self.counted = Some(last);
            self.fast_frames = if now - last < period / 2 { self.fast_frames + 1 } else { 0 };
        }
        if self.fast_frames < RUNAWAY_FRAMES {
            return None;
        }
        self.holding_until = Some(now + RUNAWAY_HOLD);
        (now < due).then_some(due)
    }
}

impl<G, F> App<G, F>
where
    G: Game,
    F: FnOnce(&mut Context) -> Result<G>,
{
    fn init(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        let conf = &self.conf;
        let mut attrs = WindowAttributes::default()
            .with_title(conf.title.clone())
            .with_resizable(conf.resizable)
            .with_visible(false);
        // A phone's window always fills the screen. iOS would otherwise honor
        // the size and leave part of the screen black.
        if !MOBILE {
            attrs = attrs.with_inner_size(winit::dpi::LogicalSize::new(conf.width, conf.height));
            if let Some((w, h)) = conf.min_size {
                attrs = attrs.with_min_inner_size(winit::dpi::LogicalSize::new(w, h));
            }
        }
        #[cfg(target_os = "ios")]
        {
            use winit::platform::ios::WindowAttributesExtIOS;
            attrs = attrs
                .with_prefers_status_bar_hidden(true)
                .with_prefers_home_indicator_hidden(true);
        }
        if conf.fullscreen {
            attrs = attrs.with_fullscreen(Some(Fullscreen::Borderless(None)));
        }
        #[cfg(target_os = "ios")]
        let scene_window = crate::platform::SceneWindow::show();
        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .map_err(|e| Error::Window(e.to_string()))?,
        );
        #[cfg(target_os = "ios")]
        drop(scene_window);
        let graphics = Graphics::new(window.clone(), event_loop.owned_display_handle(), conf)?;
        let mut ctx = Context {
            graphics,
            keyboard: Keyboard::new(window.clone()),
            mouse: Mouse::new(window.clone()),
            gamepad: Gamepads::new(),
            touch: Touch::new(),
            timer: Timer::new(conf.max_dt),
            window: Window::new(window.clone(), conf.title.clone()),
            #[cfg(feature = "audio")]
            audio: crate::audio::Audio::new(),
            quit: false,
        };
        let load = self.load.take().expect("load runs once");
        let game = load(&mut ctx)?;
        window.set_visible(true);
        self.state = Some((ctx, game));
        Ok(())
    }

    fn frame(&mut self, event_loop: &ActiveEventLoop) {
        if self.suspended {
            return;
        }
        let Some((ctx, game)) = &mut self.state else {
            return;
        };
        self.last_frame = Instant::now();
        ctx.timer.frame_started(self.last_frame);
        #[cfg(target_os = "android")]
        if self.android_pause.frame_drawn() {
            log::info!("Android resumed the app");
            #[cfg(feature = "audio")]
            ctx.audio.set_hold(crate::audio::Hold::Paused, false);
        }
        if framelog::time(Phase::Input, || poll_gamepads(ctx, game)) == PadNews::Used {
            self.pad_used = Some(self.last_frame);
        }
        let dt = ctx.timer.step();
        framelog::time(Phase::Update, || game.update(ctx, dt));
        ctx.keyboard.end_frame();
        ctx.mouse.end_frame();
        ctx.gamepad.end_frame();
        ctx.touch.end_frame();

        // A window nobody can see isn't drawn, as in LÖVE. It is after anything
        // happens to it, and a few times a second regardless, which is how a
        // window the OS never reported as back gets noticed.
        let hidden = ctx.window.is_minimized() || ctx.graphics.surface_hidden();
        let draw = !hidden
            || std::mem::take(&mut self.draw_next)
            || self.last_frame - self.last_draw >= HIDDEN_DRAW_TIME
            || self.autoshot.path.is_some();
        ctx.graphics.begin_frame();
        if draw {
            self.last_draw = self.last_frame;
            framelog::time(Phase::Draw, || game.draw(ctx));
        }
        if let Some(path) = &self.autoshot.path
            && ctx.timer.frame_count() == self.autoshot.frame
        {
            ctx.graphics.capture_screenshot(path.clone());
        }
        ctx.graphics.end_frame(draw);
        if draw && !ctx.graphics.surface_hidden() {
            // It reached the screen, whatever the OS said last.
            self.occluded = false;
        }
        self.log.frame(ctx.graphics.stats());
        if ctx.graphics.take_out_of_memory() {
            game.low_memory(ctx);
        }
        if let Some(error) = ctx.graphics.take_fatal_error() {
            self.fail(event_loop, error);
            return;
        }

        if self.autoshot.path.is_some() && ctx.timer.frame_count() >= self.autoshot.frame {
            exit_app(event_loop);
        }
        if ctx.quit {
            ctx.quit = false;
            if !game.quit(ctx) {
                exit_app(event_loop);
            }
        }
    }

    /// Updates the touch state and calls the game's touch callbacks, in logical pixels.
    fn touch_event(&mut self, id: TouchId, phase: TouchPhase, x: f32, y: f32, pressure: f32) {
        let Some((ctx, game)) = &mut self.state else {
            return;
        };
        match phase {
            TouchPhase::Started => {
                ctx.touch.begin(id, vec2(x, y), pressure);
                game.touch_pressed(ctx, id, x, y);
            }
            TouchPhase::Moved => {
                if let Some(d) = ctx.touch.moved(id, vec2(x, y), pressure) {
                    game.touch_moved(ctx, id, x, y, d.x, d.y);
                }
            }
            TouchPhase::Ended => {
                if ctx.touch.end(id, vec2(x, y), pressure) {
                    game.touch_released(ctx, id, x, y);
                }
            }
            TouchPhase::Cancelled => {
                if ctx.touch.cancel(id, vec2(x, y), pressure) {
                    game.touch_cancelled(ctx, id, x, y);
                }
            }
        }
        if MOBILE {
            self.emulate_mouse(id, phase, x, y);
        }
    }

    /// Takes everything waiting in Android's input queue before winit can (see
    /// `input/android.rs`). winit still reads what arrives in the moment between
    /// this and its own read, so `window_event` keeps handling touches and keys.
    #[cfg(target_os = "android")]
    fn read_android_input(&mut self, event_loop: &ActiveEventLoop) {
        use crate::input::android::Event;

        let mut reader = std::mem::take(&mut self.android_input);
        reader.drain(crate::platform::android_app(), |event| {
            self.wake = true;
            self.draw_next = true;
            let Some((ctx, game)) = &mut self.state else {
                return;
            };
            match event {
                Event::Touch {
                    id,
                    phase,
                    x,
                    y,
                    pressure,
                } => {
                    let scale = ctx.window.inner.scale_factor() as f32;
                    self.touch_event(TouchId(id), phase, x / scale, y / scale, pressure);
                }
                Event::Key {
                    key,
                    pressed,
                    repeat,
                    text,
                } => {
                    let mut buffer = [0; 4];
                    let text = text.map(|c| &*c.encode_utf8(&mut buffer));
                    key_event(ctx, game, key, pressed, repeat, text);
                }
                Event::PadButton {
                    device,
                    button,
                    down,
                } => ctx.gamepad.android_button(device, button, down),
                Event::PadMotion(motion) => ctx.gamepad.android_motion(&motion),
            }
        });
        self.android_input = reader;
        if let Some((ctx, game)) = &mut self.state
            && ctx.quit
        {
            ctx.quit = false;
            if !game.quit(ctx) {
                exit_app(event_loop);
            }
        }
    }

    /// Drives the mouse with the first finger on mobile, as the OS does for
    /// touchscreens on desktop, so mouse-only games work on a phone.
    fn emulate_mouse(&mut self, id: TouchId, phase: TouchPhase, x: f32, y: f32) {
        let Some((ctx, game)) = &mut self.state else {
            return;
        };
        match self.mouse_finger.route(id, phase) {
            Some(MouseAction::Press) => {
                let d = ctx.mouse.moved(vec2(x, y));
                game.mouse_moved(ctx, x, y, d.x, d.y);
                ctx.mouse.press(MouseButton::Left);
                game.mouse_pressed(ctx, x, y, MouseButton::Left);
            }
            Some(MouseAction::Move) => {
                let d = ctx.mouse.moved(vec2(x, y));
                game.mouse_moved(ctx, x, y, d.x, d.y);
            }
            Some(MouseAction::Release) => {
                ctx.mouse.release(MouseButton::Left);
                game.mouse_released(ctx, x, y, MouseButton::Left);
            }
            Some(MouseAction::Cancel) => {
                ctx.mouse.cancel(MouseButton::Left);
                game.mouse_cancelled(ctx, x, y, MouseButton::Left);
            }
            None => {}
        }
    }

    /// Ends every finger without a release, for when focus is lost or the app is
    /// suspended. Lifting them later does nothing, as they're already gone.
    fn cancel_touches(&mut self) {
        let Some((ctx, game)) = &mut self.state else {
            return;
        };
        let mouse_finger = self.mouse_finger.take();
        for t in ctx.touch.cancel_all() {
            let (x, y) = (t.position.x, t.position.y);
            game.touch_cancelled(ctx, t.id, x, y);
            if mouse_finger == Some(t.id) {
                ctx.mouse.cancel(MouseButton::Left);
                game.mouse_cancelled(ctx, x, y, MouseButton::Left);
            }
        }
    }

    /// Stops after a failure to start or resume. `run_app` never returns on
    /// iOS, so `run` could never report an error stored there.
    fn fail(&mut self, event_loop: &ActiveEventLoop, error: Error) {
        if cfg!(target_os = "ios") {
            eprintln!("love2drs: {error}");
            std::process::exit(1);
        }
        self.error = Some(error);
        event_loop.exit();
    }
}

impl<G, F> ApplicationHandler for App<G, F>
where
    G: Game,
    F: FnOnce(&mut Context) -> Result<G>,
{
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.error.is_some() {
            return;
        }
        self.suspended = false;
        #[cfg(target_os = "android")]
        self.android_pause.forget_redraw();
        let result = match &mut self.state {
            None => self.init(event_loop),
            Some((ctx, game)) => ctx.graphics.resume().map(|()| {
                #[cfg(all(feature = "audio", target_os = "ios"))]
                ctx.audio.end_interruption();
                #[cfg(feature = "audio")]
                ctx.audio.set_hold(crate::audio::Hold::Background, false);
                game.resumed(ctx);
            }),
        };
        if let Err(e) = result {
            self.fail(event_loop, e);
        }
    }

    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: winit::event::StartCause) {
        let for_a_frame = std::mem::take(&mut self.redraw_asked);
        self.log.woke(!for_a_frame && matches!(cause, winit::event::StartCause::WaitCancelled { .. }));
        #[cfg(target_os = "android")]
        {
            let began = framelog::now();
            self.read_android_input(event_loop);
            self.log.done(began);
        }
        let _ = event_loop;
    }

    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        self.suspended = true;
        #[cfg(target_os = "android")]
        self.android_pause.forget_redraw();
        self.cancel_touches();
        let Some((ctx, game)) = &mut self.state else {
            return;
        };
        #[cfg(any(target_os = "android", target_os = "ios"))]
        ctx.gamepad.suspend();
        ctx.keyboard.release_all();
        ctx.mouse.release_all();
        #[cfg(feature = "audio")]
        ctx.audio.set_hold(crate::audio::Hold::Background, true);
        game.suspended(ctx);
        ctx.graphics.suspend();
    }

    fn memory_warning(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some((ctx, game)) = &mut self.state {
            game.low_memory(ctx);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let began = framelog::now();
        self.handle_window_event(event_loop, event);
        self.log.done(began);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let began = framelog::now();
        self.plan_next_frame(event_loop);
        self.log.done(began);
        self.log.sleeping();
    }
}

impl<G, F> App<G, F>
where
    G: Game,
    F: FnOnce(&mut Context) -> Result<G>,
{
    fn handle_window_event(&mut self, event_loop: &ActiveEventLoop, event: WindowEvent) {
        if matches!(event, WindowEvent::RedrawRequested) {
            self.frame(event_loop);
            // UIKit hands out a redraw when it likes, which can be after the loop
            // has settled down to wait for it. One more pass plans the next frame.
            #[cfg(target_os = "ios")]
            event_loop.set_control_flow(ControlFlow::Poll);
            return;
        }
        // Input, or the window changing: show what it did right away.
        self.wake = true;
        self.draw_next = true;
        let Some((ctx, game)) = &mut self.state else {
            return;
        };
        match event {
            WindowEvent::CloseRequested => {
                if !game.quit(ctx) {
                    exit_app(event_loop);
                }
            }
            WindowEvent::Resized(size) => {
                let scale = ctx.window.inner.scale_factor();
                ctx.graphics.resize(size.width, size.height, scale);
                let logical = size.to_logical::<f32>(scale);
                if size.width > 0 && size.height > 0 {
                    game.resize(ctx, logical.width, logical.height);
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                let size = crate::window::drawable_size(&ctx.window.inner);
                ctx.graphics.resize(size.width, size.height, scale_factor);
            }
            WindowEvent::Focused(focused) => {
                if !focused {
                    self.cancel_touches();
                }
                let Some((ctx, game)) = &mut self.state else {
                    return;
                };
                if !focused {
                    ctx.keyboard.release_all();
                    ctx.mouse.release_all();
                }
                game.focus(ctx, focused);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let mut key = match event.physical_key {
                    PhysicalKey::Code(code) => Key::from_winit(code),
                    PhysicalKey::Unidentified(_) => Key::Unknown,
                };
                // Android's back gesture is its Escape key.
                if cfg!(target_os = "android") && event.logical_key == NamedKey::BrowserBack {
                    key = Key::Escape;
                }
                // iOS's on-screen keyboard sends Return as a character.
                if cfg!(target_os = "ios") && event.logical_key.to_text() == Some("\n") {
                    key = Key::Enter;
                }
                key_event(
                    ctx,
                    game,
                    key,
                    event.state == ElementState::Pressed,
                    event.repeat,
                    event.text.as_deref(),
                );
            }
            WindowEvent::CursorMoved { position, .. } => {
                let p = position.to_logical::<f32>(ctx.window.inner.scale_factor());
                let d = ctx.mouse.moved(vec2(p.x, p.y));
                game.mouse_moved(ctx, p.x, p.y, d.x, d.y);
            }
            WindowEvent::Occluded(occluded) => self.occluded = occluded,
            WindowEvent::CursorEntered { .. } => ctx.mouse.inside = true,
            WindowEvent::CursorLeft { .. } => ctx.mouse.inside = false,
            WindowEvent::MouseInput { state, button, .. } => {
                let b = MouseButton::from_winit(button);
                let p = ctx.mouse.position;
                match state {
                    ElementState::Pressed => {
                        ctx.mouse.press(b);
                        game.mouse_pressed(ctx, p.x, p.y, b);
                    }
                    ElementState::Released => {
                        ctx.mouse.release(b);
                        game.mouse_released(ctx, p.x, p.y, b);
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (x, y) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (x, y),
                    MouseScrollDelta::PixelDelta(p) => (p.x as f32 / 40.0, p.y as f32 / 40.0),
                };
                ctx.mouse.scrolled(vec2(x, y));
                game.wheel_moved(ctx, x, y);
            }
            WindowEvent::Touch(touch) => {
                let p = touch
                    .location
                    .to_logical::<f32>(ctx.window.inner.scale_factor());
                let pressure = touch.force.map_or(1.0, |f| f.normalized() as f32);
                self.touch_event(TouchId(touch.id), touch.phase, p.x, p.y, pressure);
            }
            WindowEvent::DroppedFile(path) => game.file_dropped(ctx, path),
            _ => {}
        }
        let Some((ctx, game)) = &mut self.state else {
            return;
        };
        if ctx.quit {
            ctx.quit = false;
            if !game.quit(ctx) {
                exit_app(event_loop);
            }
        }
    }

    /// Decides when the loop runs next: at once, or after a sleep.
    fn plan_next_frame(&mut self, event_loop: &ActiveEventLoop) {
        let Some((ctx, game)) = &mut self.state else {
            return;
        };
        #[cfg(feature = "audio")]
        ctx.audio.poll();
        // The OS forbids drawing in the background. Sleep until `resumed`.
        if self.suspended {
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        }
        // winit remembers the redraw asked for before the pause and sends it
        // when the app resumes, so nothing else has to wake this up.
        #[cfg(target_os = "android")]
        if self.android_pause.is_paused() {
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        }
        // Nothing is presented while the window is minimized or can't be seen, so
        // vsync no longer paces the loop. Keep the game running, but at a capped
        // rate. A screenshot run counts frames, so it's left to run.
        let now = Instant::now();
        let hidden = ctx.window.is_minimized() || self.occluded || ctx.graphics.surface_hidden();
        if hidden && self.autoshot.path.is_none() {
            let next = self.last_frame + HIDDEN_FRAME_TIME;
            if now < next {
                event_loop.set_control_flow(ControlFlow::WaitUntil(next));
                return;
            }
        }
        if self.refresh_checked.is_none_or(|at| now - at >= REFRESH_CHECK_TIME) {
            self.refresh_checked = Some(now);
            let monitor = ctx.window.inner.current_monitor();
            self.refresh = monitor.and_then(|m| m.refresh_rate_millihertz()).map(|mhz| mhz as f32 / 1000.0);
        }
        // Under a frame cap, sleep until the next frame is due, unless input came in.
        if !std::mem::take(&mut self.wake)
            && let Some(due) = ctx.timer.next_frame(self.last_frame, now, display_pacing(ctx, self.refresh))
        {
            let has_pads = ctx.gamepad.ids().next().is_some();
            let news = if has_pads { poll_gamepads(ctx, game) } else { PadNews::Nothing };
            if news == PadNews::Used {
                self.pad_used = Some(now);
            }
            // Anything from a controller gets a frame at once, like any other input.
            if news == PadNews::Nothing {
                let in_use = self.pad_used.is_some_and(|at| now - at < PAD_IN_USE_TIME);
                let wake_at = match (has_pads, in_use) {
                    (true, true) => due.min(now + PAD_POLL_TIME),
                    (true, false) => due.min(now + IDLE_PAD_POLL_TIME),
                    (false, _) => due,
                };
                event_loop.set_control_flow(ControlFlow::WaitUntil(wake_at));
                return;
            }
        }
        if ctx.graphics.vsync()
            && self.autoshot.path.is_none()
            && let Some(due) = self.runaway.next_frame(self.last_frame, now, self.refresh)
        {
            event_loop.set_control_flow(ControlFlow::WaitUntil(due));
            return;
        }
        #[cfg(target_os = "android")]
        if self.android_pause.asking_for_redraw() {
            log::info!("Android paused the app without taking its window");
            #[cfg(feature = "audio")]
            ctx.audio.set_hold(crate::audio::Hold::Paused, true);
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        }
        event_loop.set_control_flow(until_the_redraw(now));
        ctx.window.inner.request_redraw();
        self.redraw_asked = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs `frames` frames that each come back after `took`, as a loop that
    /// starts the next one as soon as the guard allows. Returns how long they took.
    fn run_frames(guard: &mut RunawayGuard, frames: u32, took: Duration, refresh: Option<f32>) -> Duration {
        let start = Instant::now();
        let mut last = start;
        for _ in 0..frames {
            let done = last + took;
            last = guard.next_frame(last, done, refresh).unwrap_or(done);
        }
        last - start
    }

    #[test]
    fn frames_that_wait_for_the_display_are_left_alone() {
        let mut guard = RunawayGuard::default();
        // 240 Hz: each frame took most of the 4.2 ms refresh, so vsync paced it.
        let took = run_frames(&mut guard, 240, Duration::from_micros(4100), Some(240.0));
        assert_eq!(took, Duration::from_micros(4100) * 240);
        assert!(guard.holding_until.is_none());
    }

    #[test]
    fn a_few_quick_frames_while_the_queue_fills_are_left_alone() {
        let mut guard = RunawayGuard::default();
        let quick = run_frames(&mut guard, 4, Duration::from_micros(300), Some(240.0));
        assert_eq!(quick, Duration::from_micros(300) * 4);
        run_frames(&mut guard, 1, Duration::from_micros(4100), Some(240.0));
        assert_eq!(guard.fast_frames, 0, "a frame that waited starts the count over");
    }

    #[test]
    fn a_loop_nothing_paces_is_held_to_the_displays_rate() {
        let mut guard = RunawayGuard::default();
        // Nothing waits: 0.4 ms a frame would be 2500 frames a second.
        let took = run_frames(&mut guard, 600, Duration::from_micros(400), Some(60.0));
        let fps = 600.0 / took.as_secs_f32();
        assert!((60.0..75.0).contains(&fps), "{fps} frames a second");
        // Without a known rate, only a loop that's clearly spinning is held back.
        let mut guard = RunawayGuard::default();
        run_frames(&mut guard, 100, Duration::from_millis(3), None);
        assert!(guard.holding_until.is_none());
        let took = run_frames(&mut guard, 2500, Duration::from_micros(400), None);
        assert!(2500.0 / took.as_secs_f32() < 300.0);
    }

    #[test]
    fn frames_arriving_means_not_paused() {
        let mut d = PauseDetector::default();
        for _ in 0..5 {
            assert!(!d.asking_for_redraw());
            assert!(!d.frame_drawn());
            assert!(!d.is_paused());
        }
    }

    /// Under a frame cap the loop sleeps between frames without asking for a
    /// redraw, so several passes go by for each frame. That froze every capped
    /// game on Android: the second pass found no frame drawn and called it a pause.
    #[test]
    fn passes_that_only_wait_for_the_next_frame_are_not_a_pause() {
        let mut d = PauseDetector::default();
        for _ in 0..5 {
            // A pass that sleeps until the frame is due, then one that asks.
            assert!(!d.is_paused());
            assert!(!d.asking_for_redraw());
            assert!(!d.frame_drawn());
            assert!(!d.is_paused());
        }
    }

    #[test]
    fn a_redraw_that_never_comes_means_paused_until_the_next_frame() {
        let mut d = PauseDetector::default();
        assert!(!d.asking_for_redraw());
        assert!(d.asking_for_redraw(), "reported once");
        assert!(d.is_paused());
        assert!(!d.asking_for_redraw(), "reported once");
        assert!(d.is_paused());
        assert!(d.frame_drawn());
        assert!(!d.is_paused());
    }

    #[test]
    fn losing_the_window_is_not_a_pause() {
        let mut d = PauseDetector::default();
        d.asking_for_redraw();
        d.forget_redraw();
        assert!(!d.asking_for_redraw());
        assert!(!d.is_paused());
    }
}
