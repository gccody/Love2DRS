use std::sync::Arc;
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseScrollDelta, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{NamedKey, PhysicalKey};
use winit::window::{Fullscreen, WindowAttributes, WindowId};

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
        mouse_finger: MouseFinger::default(),
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

/// Reads controller events and replays them as `Game` callbacks. Returns
/// whether there were any.
fn poll_gamepads<G: Game>(ctx: &mut Context, game: &mut G) -> bool {
    ctx.gamepad.poll();
    if ctx.gamepad.events.is_empty() {
        return false;
    }
    let mut events = std::mem::take(&mut ctx.gamepad.events);
    for ev in events.drain(..) {
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
    true
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
    /// The finger that acts as the mouse on mobile.
    mouse_finger: MouseFinger,
    #[cfg(target_os = "android")]
    android_input: crate::input::android::Reader,
    #[cfg(target_os = "android")]
    android_pause: PauseDetector,
}

/// Notices when Android pauses the app without taking its window, as it does
/// behind a permission dialog. winit keeps that to itself and `suspended` doesn't
/// fire, but it also holds back redraws until the app resumes. Since every pass of
/// the loop asks for one, a pass that ends with the last one undelivered means the
/// app is paused.
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
    fn about_to_wait(&mut self) -> bool {
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

/// Frame interval while the window is minimized (30 fps).
const MINIMIZED_FRAME_TIME: Duration = Duration::from_nanos(1_000_000_000 / 30);
/// Controllers don't wake the event loop, so while a frame cap has it asleep
/// they're looked at this often.
const PAD_POLL_TIME: Duration = Duration::from_millis(16);

/// The display's refresh rate, when the loop should wait for the next refresh
/// even without a frame cap. Only on iOS: winit's `ControlFlow::Poll` there
/// fires the run loop again at once, so the main thread spun between frames and
/// kept a whole core busy (measured on an iPhone: 98% CPU at 60 fps, and 100
/// times fewer instructions once it waited). Elsewhere presenting a frame already
/// waits for vsync, and coarse timers would make an extra wait miss refreshes.
#[cfg(target_os = "ios")]
fn display_pacing(ctx: &Context) -> Option<f32> {
    if !ctx.graphics.vsync() {
        return None;
    }
    let hz = ctx.window.inner.current_monitor().and_then(|m| m.refresh_rate_millihertz());
    Some(hz.map_or(60.0, |mhz| mhz as f32 / 1000.0))
}

#[cfg(not(target_os = "ios"))]
fn display_pacing(_ctx: &Context) -> Option<f32> {
    None
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
        #[cfg(target_os = "android")]
        if self.android_pause.frame_drawn() {
            log::info!("Android resumed the app");
            #[cfg(feature = "audio")]
            ctx.audio.set_hold(crate::audio::Hold::Paused, false);
        }
        poll_gamepads(ctx, game);
        let dt = ctx.timer.step();
        game.update(ctx, dt);
        ctx.keyboard.end_frame();
        ctx.mouse.end_frame();
        ctx.gamepad.end_frame();
        ctx.touch.end_frame();

        ctx.graphics.begin_frame();
        game.draw(ctx);
        if let Some(path) = &self.autoshot.path
            && ctx.timer.frame_count() == self.autoshot.frame
        {
            ctx.graphics.capture_screenshot(path.clone());
        }
        ctx.graphics.end_frame();
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

    #[cfg(target_os = "android")]
    fn new_events(&mut self, event_loop: &ActiveEventLoop, _cause: winit::event::StartCause) {
        self.read_android_input(event_loop);
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
        if matches!(event, WindowEvent::RedrawRequested) {
            self.frame(event_loop);
            return;
        }
        // Input, or the window changing: show what it did right away.
        self.wake = true;
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

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
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
        #[cfg(target_os = "android")]
        {
            if self.android_pause.about_to_wait() {
                log::info!("Android paused the app without taking its window");
                #[cfg(feature = "audio")]
                ctx.audio.set_hold(crate::audio::Hold::Paused, true);
            }
            // winit remembers the redraw asked for before the pause and sends it
            // when the app resumes, so nothing else has to wake this up.
            if self.android_pause.is_paused() {
                event_loop.set_control_flow(ControlFlow::Wait);
                return;
            }
        }
        // Nothing is presented while minimized, so vsync no longer paces the
        // loop. Keep the game running, but at a capped rate.
        if ctx.window.is_minimized() {
            let next = self.last_frame + MINIMIZED_FRAME_TIME;
            if Instant::now() < next {
                event_loop.set_control_flow(ControlFlow::WaitUntil(next));
                return;
            }
        }
        // Under a frame cap, sleep until the next frame is due, unless input came in.
        let now = Instant::now();
        if !std::mem::take(&mut self.wake)
            && let Some(due) = ctx.timer.next_frame(self.last_frame, now, display_pacing(ctx))
        {
            let has_pads = ctx.gamepad.ids().next().is_some();
            if !(has_pads && poll_gamepads(ctx, game)) {
                let wake_at = if has_pads { due.min(now + PAD_POLL_TIME) } else { due };
                event_loop.set_control_flow(ControlFlow::WaitUntil(wake_at));
                return;
            }
        }
        event_loop.set_control_flow(ControlFlow::Poll);
        ctx.window.inner.request_redraw();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_arriving_means_not_paused() {
        let mut d = PauseDetector::default();
        for _ in 0..5 {
            assert!(!d.about_to_wait());
            assert!(!d.frame_drawn());
            assert!(!d.is_paused());
        }
    }

    #[test]
    fn a_redraw_that_never_comes_means_paused_until_the_next_frame() {
        let mut d = PauseDetector::default();
        assert!(!d.about_to_wait());
        assert!(d.about_to_wait(), "reported once");
        assert!(d.is_paused());
        assert!(!d.about_to_wait(), "reported once");
        assert!(d.is_paused());
        assert!(d.frame_drawn());
        assert!(!d.is_paused());
    }

    #[test]
    fn losing_the_window_is_not_a_pause() {
        let mut d = PauseDetector::default();
        d.about_to_wait();
        d.forget_redraw();
        assert!(!d.about_to_wait());
        assert!(!d.is_paused());
    }
}
