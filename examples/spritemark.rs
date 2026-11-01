//! Sprite stress test: thousands of bouncing, individually drawn sprites.
//!
//! Every sprite is a plain `g.draw_ex(...)` call; the renderer batches them
//! into a handful of GPU draw calls automatically.
//!
//! Run with `cargo run --release --example spritemark -- [initial_count]`.
//! Hold the left mouse button to add sprites, right button to remove them.

use love2drs::prelude::*;

struct Sprite {
    pos: Vec2,
    vel: Vec2,
    spin: f32,
    angle: f32,
    tint: Color,
}

struct Spritemark {
    image: Image,
    sprites: Vec<Sprite>,
    fps_log: f32,
}

const GRAVITY: f32 = 900.0;

impl Spritemark {
    fn new(ctx: &mut Context) -> Result<Self> {
        // A soft round "gem" generated at startup.
        let data = ImageData::from_fn(24, 24, |x, y| {
            let d = vec2(x as f32 - 11.5, y as f32 - 11.5).length() / 11.5;
            let shade = 1.0 - d * 0.5;
            if d < 1.0 {
                Color::rgba(shade, shade, shade, (1.0 - d).min(0.15) / 0.15)
            } else {
                Color::TRANSPARENT
            }
        });
        let image = ctx.graphics.new_image_from_data(&data);
        let count = std::env::args()
            .nth(1)
            .and_then(|a| a.parse().ok())
            .unwrap_or(10_000);
        let mut game = Spritemark {
            image,
            sprites: Vec::new(),
            fps_log: 0.0,
        };
        game.spawn(count, vec2(400.0, 100.0));
        Ok(game)
    }

    fn spawn(&mut self, n: usize, at: Vec2) {
        for _ in 0..n {
            let angle = random_range(0.0, std::f32::consts::TAU);
            let speed = random_range(50.0, 450.0);
            self.sprites.push(Sprite {
                pos: at,
                vel: vec2(angle.cos(), angle.sin()) * speed,
                spin: random_range(-4.0, 4.0),
                angle: 0.0,
                tint: Color::hsv(random_range(0.0, 360.0), 0.55, 1.0),
            });
        }
    }
}

impl Game for Spritemark {
    fn update(&mut self, ctx: &mut Context, dt: f32) {
        if ctx.mouse.is_down(MouseButton::Left) {
            self.spawn(500, ctx.mouse.position());
        }
        if ctx.mouse.is_down(MouseButton::Right) {
            let n = self.sprites.len().saturating_sub(500);
            self.sprites.truncate(n);
        }

        let (w, h) = ctx.graphics.dimensions();
        for s in &mut self.sprites {
            s.vel.y += GRAVITY * dt;
            s.pos += s.vel * dt;
            s.angle += s.spin * dt;
            if s.pos.x < 0.0 || s.pos.x > w {
                s.vel.x = -s.vel.x;
                s.pos.x = s.pos.x.clamp(0.0, w);
            }
            if s.pos.y > h {
                s.vel.y = -s.vel.y * 0.85;
                s.pos.y = h;
                if random() < 0.5 {
                    s.vel.y -= random_range(0.0, 400.0);
                }
            }
            if s.pos.y < 0.0 {
                s.vel.y = 0.0;
                s.pos.y = 0.0;
            }
        }

        self.fps_log += dt;
        if self.fps_log >= 2.0 {
            self.fps_log = 0.0;
            let stats = ctx.graphics.stats();
            println!(
                "{} sprites: {:.0} fps ({:.2} ms), {} draw calls",
                self.sprites.len(),
                ctx.timer.fps(),
                ctx.timer.average_delta() * 1000.0,
                stats.draw_calls
            );
        }
    }

    fn draw(&mut self, ctx: &mut Context) {
        let g = &mut ctx.graphics;
        g.set_background_color(Color::hex(0x14161c));
        for s in &self.sprites {
            g.draw_ex(
                &self.image,
                DrawParams::at(s.pos.x, s.pos.y)
                    .rotation(s.angle)
                    .centered()
                    .color(s.tint),
            );
        }

        let stats = g.stats();
        let fps = ctx.timer.fps();
        g.set_color(Color::rgba(0.0, 0.0, 0.0, 0.7));
        g.rectangle(Fill, 8.0, 8.0, 330.0, 74.0);
        g.set_color(Color::WHITE);
        g.print(
            &format!(
                "sprites:    {}\nfps:        {fps:.0}\ndraw calls: {}\nLMB add / RMB remove",
                self.sprites.len(),
                stats.draw_calls
            ),
            16.0,
            12.0,
        );
    }

    fn key_pressed(&mut self, ctx: &mut Context, key: Key, _repeat: bool) {
        if key == Key::Escape {
            ctx.quit();
        }
    }
}

fn main() -> love2drs::Result {
    love2drs::run(
        Conf::new("love2drs spritemark")
            .size(1024, 720)
            .vsync(false)
            .resizable(true),
        Spritemark::new,
    )
}
