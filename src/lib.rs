//! # love2drs
//!
//! A [LÖVE](https://love2d.org)-inspired 2D game framework: the same small,
//! friendly API (`load` / `update` / `draw` callbacks, immediate-mode drawing,
//! push/pop transforms) on top of a batched [wgpu](https://wgpu.rs) renderer.
//!
//! ```no_run
//! use love2drs::prelude::*;
//!
//! struct Game1 {
//!     x: f32,
//! }
//!
//! impl Game for Game1 {
//!     fn update(&mut self, ctx: &mut Context, dt: f32) {
//!         self.x += ctx.keyboard.axis(Key::Left, Key::Right) * 200.0 * dt;
//!     }
//!
//!     fn draw(&mut self, ctx: &mut Context) {
//!         let g = &mut ctx.graphics;
//!         g.set_color(Color::hex(0xff6688));
//!         g.circle(Fill, self.x, 300.0, 40.0);
//!         g.set_color(Color::WHITE);
//!         g.print("Use the arrow keys", 10.0, 10.0);
//!     }
//! }
//!
//! fn main() -> love2drs::Result {
//!     love2drs::run(Conf::new("My game"), |_ctx| Ok(Game1 { x: 400.0 }))
//! }
//! ```
//!
//! ## Modules
//!
//! | LÖVE             | love2drs                         |
//! |------------------|----------------------------------|
//! | `love.graphics`  | [`ctx.graphics`](graphics::Graphics) |
//! | `love.keyboard`  | [`ctx.keyboard`](input::Keyboard) |
//! | `love.mouse`     | [`ctx.mouse`](input::Mouse)      |
//! | `love.audio`     | [`ctx.audio`](audio::Audio)      |
//! | `love.timer`     | [`ctx.timer`](timer::Timer)      |
//! | `love.window`    | [`ctx.window`](window::Window)   |
//! | `love.math`      | [`math`]                         |
//!
//! ## Performance
//!
//! Draw calls are recorded into one CPU vertex buffer per frame and merged
//! into as few GPU draw calls as possible; shapes and text share a texture so
//! they batch together. Use [`graphics::SpriteBatch`] for large static
//! geometry such as tile maps: it stays on the GPU between frames.

#![warn(missing_docs)]

mod app;
#[cfg(feature = "audio")]
pub mod audio;
mod conf;
mod error;
pub mod graphics;
pub mod input;
pub mod math;
pub mod timer;
mod util;
pub mod window;

pub use app::{Context, Game, run};
pub use conf::Conf;
pub use error::{Error, Result};

/// Re-export of the GPU crate for advanced use.
pub use wgpu;
/// Re-export of the windowing crate for advanced use.
pub use winit;

/// Everything needed for a typical game: `use love2drs::prelude::*;`
///
/// Besides the drawing types and callbacks, this includes the module types you
/// pass to helper functions (`fn draw_hud(g: &mut Graphics)`,
/// `fn new(audio: &Audio)`) and the common gameplay math (`Rng`, `approach`,
/// `noise2`, ...). Rarer items stay in their modules: [`timer::Timer`],
/// [`window::Window`], [`graphics::FrameStats`], [`graphics::IntoUniform`],
/// [`math::sin_cos`] and [`math::set_random_seed`].
pub mod prelude {
    #[cfg(feature = "audio")]
    pub use crate::audio::{Audio, SoundData, Source};
    pub use crate::graphics::DrawMode::{self, Fill, Line};
    pub use crate::graphics::{
        Align, BlendMode, Canvas, Color, DrawParams, Drawable, FilterMode, Font, Graphics, Image,
        ImageData, Quad, Shader, SpriteBatch, Vertex, WrapMode,
    };
    pub use crate::input::{Key, Keyboard, Mouse, MouseButton};
    pub use crate::math::{
        Rng, Transform, Vec2, approach, lerp, noise1, noise2, random, random_int, random_normal,
        random_range, vec2,
    };
    pub use crate::{Conf, Context, Error, Game, Result};
}
