use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::PhysicalKey;
use winit::window::{Fullscreen, WindowAttributes, WindowId};

use crate::graphics::Graphics;
use crate::input::{Key, Keyboard, Mouse, MouseButton};
use crate::math::vec2;
use crate::timer::Timer;
use crate::window::Window;
use crate::{Conf, Error, Result};

/// Everything a game can touch: the `love.*` modules.
pub struct Context {
    pub graphics: Graphics,
    pub keyboard: Keyboard,
    pub mouse: Mouse,
    pub timer: Timer,
    pub window: Window,
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

    fn key_released(&mut self, ctx: &mut Context, key: Key) {}

    /// Text typed by the user (already composed by the OS keyboard layout / IME).
    fn text_input(&mut self, ctx: &mut Context, text: &str) {}

    fn mouse_pressed(&mut self, ctx: &mut Context, x: f32, y: f32, button: MouseButton) {}

    fn mouse_released(&mut self, ctx: &mut Context, x: f32, y: f32, button: MouseButton) {}

    fn mouse_moved(&mut self, ctx: &mut Context, x: f32, y: f32, dx: f32, dy: f32) {}

    /// Mouse wheel movement; `y > 0` scrolls away from the user.
    fn wheel_moved(&mut self, ctx: &mut Context, x: f32, y: f32) {}

    /// The window was resized (logical pixels).
    fn resize(&mut self, ctx: &mut Context, width: f32, height: f32) {}

    fn focus(&mut self, ctx: &mut Context, focused: bool) {}

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
    let event_loop = EventLoop::new().map_err(|e| Error::Window(e.to_string()))?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App::<G, F> {
        conf,
        load: Some(load),
        state: None,
        error: None,
        autoshot: AutoShot::from_env(),
    };
    event_loop
        .run_app(&mut app)
        .map_err(|e| Error::Window(e.to_string()))?;
    match app.error {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// Testing aid: `LOVE2DRS_SCREENSHOT=out.png` saves frame
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
            .with_inner_size(winit::dpi::LogicalSize::new(conf.width, conf.height))
            .with_resizable(conf.resizable)
            .with_visible(false);
        if let Some((w, h)) = conf.min_size {
            attrs = attrs.with_min_inner_size(winit::dpi::LogicalSize::new(w, h));
        }
        if conf.fullscreen {
            attrs = attrs.with_fullscreen(Some(Fullscreen::Borderless(None)));
        }
        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .map_err(|e| Error::Window(e.to_string()))?,
        );
        let graphics = Graphics::new(window.clone(), event_loop.owned_display_handle(), conf)?;
        let mut ctx = Context {
            graphics,
            keyboard: Keyboard::new(),
            mouse: Mouse::new(window.clone()),
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
        let Some((ctx, game)) = &mut self.state else {
            return;
        };
        let dt = ctx.timer.step();
        game.update(ctx, dt);
        ctx.keyboard.end_frame();
        ctx.mouse.end_frame();

        ctx.graphics.begin_frame();
        game.draw(ctx);
        if let Some(path) = &self.autoshot.path
            && ctx.timer.frame_count() == self.autoshot.frame
        {
            ctx.graphics.capture_screenshot(path.clone());
        }
        ctx.graphics.end_frame();

        if self.autoshot.path.is_some() && ctx.timer.frame_count() >= self.autoshot.frame {
            event_loop.exit();
        }
        if ctx.quit {
            ctx.quit = false;
            if !game.quit(ctx) {
                event_loop.exit();
            }
        }
    }
}

impl<G, F> ApplicationHandler for App<G, F>
where
    G: Game,
    F: FnOnce(&mut Context) -> Result<G>,
{
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_none()
            && self.error.is_none()
            && let Err(e) = self.init(event_loop)
        {
            self.error = Some(e);
            event_loop.exit();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if matches!(event, WindowEvent::RedrawRequested) {
            self.frame(event_loop);
            return;
        }
        let Some((ctx, game)) = &mut self.state else {
            return;
        };
        match event {
            WindowEvent::CloseRequested => {
                if !game.quit(ctx) {
                    event_loop.exit();
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
                let size = ctx.window.inner.inner_size();
                ctx.graphics.resize(size.width, size.height, scale_factor);
            }
            WindowEvent::Focused(focused) => {
                if !focused {
                    ctx.keyboard.release_all();
                    ctx.mouse.release_all();
                }
                game.focus(ctx, focused);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let key = match event.physical_key {
                    PhysicalKey::Code(code) => Key::from_winit(code),
                    PhysicalKey::Unidentified(_) => Key::Unknown,
                };
                match event.state {
                    ElementState::Pressed => {
                        if !event.repeat || ctx.keyboard.key_repeat {
                            ctx.keyboard.press(key);
                            game.key_pressed(ctx, key, event.repeat);
                        }
                        if ctx.keyboard.text_input
                            && let Some(text) = &event.text
                            && text.chars().any(|c| !c.is_control())
                        {
                            game.text_input(ctx, text);
                        }
                    }
                    ElementState::Released => {
                        ctx.keyboard.release(key);
                        game.key_released(ctx, key);
                    }
                }
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
            WindowEvent::DroppedFile(path) => game.file_dropped(ctx, path),
            _ => {}
        }
        if ctx.quit {
            ctx.quit = false;
            if !game.quit(ctx) {
                event_loop.exit();
            }
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some((ctx, _)) = &self.state {
            ctx.window.inner.request_redraw();
        }
    }
}
