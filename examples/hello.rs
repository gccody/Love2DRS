//! The smallest love2drs program: move a ball with the arrow keys.
//!
//! Run with `cargo run --example hello`.

use love2drs::prelude::*;

struct Hello {
    pos: Vec2,
}

impl Game for Hello {
    fn update(&mut self, ctx: &mut Context, dt: f32) {
        let kb = &ctx.keyboard;
        let dir = vec2(kb.axis(Key::Left, Key::Right), kb.axis(Key::Up, Key::Down));
        self.pos += dir.normalize_or_zero() * 300.0 * dt;
    }

    fn draw(&mut self, ctx: &mut Context) {
        let g = &mut ctx.graphics;
        g.set_color(Color::hex(0xff6688));
        g.circle(Fill, self.pos.x, self.pos.y, 40.0);
        g.set_color(Color::WHITE);
        g.print("Hello from love2drs! Move with the arrow keys.", 20.0, 20.0);
    }

    fn key_pressed(&mut self, ctx: &mut Context, key: Key, _repeat: bool) {
        if key == Key::Escape {
            ctx.quit();
        }
    }
}

fn main() -> love2drs::Result {
    love2drs::run(Conf::new("Hello, love2drs"), |_ctx| {
        Ok(Hello {
            pos: vec2(400.0, 300.0),
        })
    })
}
