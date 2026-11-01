//! A tour of the drawing API: shapes, lines, transforms, blend modes, text
//! layout, canvases, scissoring, images and sprite sheets.
//!
//! Run with `cargo run --example showcase`.

use std::f32::consts::{PI, TAU};

use love2drs::prelude::*;

struct Showcase {
    time: f32,
    checker: Image,
    sheet: Image,
    frames: Vec<Quad>,
    canvas: Canvas,
    title: Font,
}

impl Showcase {
    fn new(ctx: &mut Context) -> Result<Self> {
        let g = &mut ctx.graphics;

        // A procedurally generated texture: no asset files needed.
        let checker = g.new_image_from_data(&ImageData::from_fn(64, 64, |x, y| {
            if (x / 8 + y / 8) % 2 == 0 {
                Color::hex(0x3a86ff)
            } else {
                Color::hex(0xffbe0b)
            }
        }));

        // A tiny 4-frame "sprite sheet" with a pulsing square, drawn pixel-crisp.
        g.set_default_filter(FilterMode::Nearest);
        let sheet = g.new_image_from_data(&ImageData::from_fn(64, 16, |x, y| {
            let frame = x / 16;
            let (lx, ly) = ((x % 16) as i32 - 8, y as i32 - 8);
            let r = 3 + frame as i32;
            if lx.abs() < r && ly.abs() < r {
                Color::hsv(frame as f32 * 70.0, 0.7, 1.0)
            } else {
                Color::TRANSPARENT
            }
        }));
        g.set_default_filter(FilterMode::Linear);
        let frames = Quad::grid(16.0, 16.0, 4, 1);

        let canvas = g.new_canvas(160, 120);
        let title = g.new_font(28.0);
        Ok(Showcase {
            time: 0.0,
            checker,
            sheet,
            frames,
            canvas,
            title,
        })
    }
}

impl Game for Showcase {
    fn update(&mut self, _ctx: &mut Context, dt: f32) {
        self.time += dt;
    }

    fn draw(&mut self, ctx: &mut Context) {
        let t = self.time;
        let fps = ctx.timer.fps();
        let g = &mut ctx.graphics;
        g.set_background_color(Color::hex(0x1d1f27));

        // Render into the canvas first; it is drawn on screen further down.
        g.with_canvas(&self.canvas, |g| {
            g.clear(Color::hex(0x2b2d42));
            g.set_color(Color::hex(0x8ecae6));
            for i in 0..12 {
                let a = t + i as f32 * TAU / 12.0;
                g.circle(Fill, 80.0 + a.cos() * 40.0, 60.0 + a.sin() * 40.0, 6.0);
            }
            g.set_color(Color::WHITE);
            g.print("canvas", 56.0, 52.0);
        });

        // Title
        g.set_font(&self.title.clone());
        g.set_color(Color::WHITE);
        g.print("love2drs showcase", 20.0, 14.0);
        let default = g.new_font(14.0);
        g.set_font(&default);
        g.set_color(Color::GRAY);
        g.print(&format!("{fps:.0} fps"), 720.0, 24.0);

        // --- Row 1: filled shapes -------------------------------------------
        let y = 90.0;
        g.set_color(Color::hex(0xef476f));
        g.rectangle(Fill, 20.0, y, 80.0, 60.0);
        g.set_color(Color::hex(0xffd166));
        g.rounded_rectangle(Fill, 120.0, y, 80.0, 60.0, 16.0);
        g.set_color(Color::hex(0x06d6a0));
        g.circle(Fill, 260.0, y + 30.0, 30.0);
        g.set_color(Color::hex(0x118ab2));
        g.ellipse(Fill, 350.0, y + 30.0, 45.0, 22.0);
        g.set_color(Color::hex(0xf78c6b));
        g.arc(
            Fill,
            450.0,
            y + 30.0,
            30.0,
            0.0,
            1.5 * PI + (t * 2.0).sin() * 0.4,
        );
        g.set_color(Color::hex(0xc77dff));
        g.polygon(Fill, &[520.0, y + 60.0, 550.0, y, 580.0, y + 60.0]);
        g.set_color(Color::hex(0x90be6d));
        let star: Vec<f32> = (0..10)
            .flat_map(|i| {
                let r = if i % 2 == 0 { 32.0 } else { 14.0 };
                let a = i as f32 * PI / 5.0 - PI / 2.0 + t * 0.5;
                [650.0 + a.cos() * r, y + 30.0 + a.sin() * r]
            })
            .collect();
        g.polygon(Line, &star);

        // --- Row 2: outlines and lines ---------------------------------------
        let y = 180.0;
        g.set_line_width(3.0);
        g.set_color(Color::hex(0xef476f));
        g.rectangle(Line, 20.0, y, 80.0, 60.0);
        g.set_color(Color::hex(0xffd166));
        g.rounded_rectangle(Line, 120.0, y, 80.0, 60.0, 16.0);
        g.set_color(Color::hex(0x06d6a0));
        g.circle(Line, 260.0, y + 30.0, 30.0);
        g.set_color(Color::hex(0x118ab2));
        g.ellipse(Line, 350.0, y + 30.0, 45.0, 22.0);
        g.set_color(Color::hex(0xf78c6b));
        g.arc(Line, 450.0, y + 30.0, 30.0, 0.0, 1.5 * PI);
        g.set_line_width(6.0);
        g.set_color(Color::hex(0xc77dff));
        let wave: Vec<f32> = (0..=40)
            .flat_map(|i| {
                let x = 510.0 + i as f32 * 6.5;
                [x, y + 30.0 + (x * 0.05 + t * 3.0).sin() * 20.0]
            })
            .collect();
        g.polyline(&wave);
        g.set_line_width(1.0);
        g.set_point_size(4.0);
        g.set_color(Color::WHITE);
        let pts: Vec<f32> = (0..20)
            .flat_map(|i| [20.0 + i as f32 * 4.0, y + 75.0 + (i % 3) as f32 * 4.0])
            .collect();
        g.points(&pts);

        // --- Row 3: transforms, images, sprite sheet --------------------------
        let y = 300.0;
        g.set_color(Color::WHITE);
        g.draw_ex(
            &self.checker,
            DrawParams::at(60.0, y + 50.0).rotation(t).centered(),
        );
        g.push();
        g.translate(170.0, y + 50.0);
        g.rotate(-t * 0.7);
        g.scale(1.0 + 0.2 * (t * 2.0).sin(), 1.0);
        g.set_color(Color::hex(0xffbe0b));
        g.rectangle(Line, -35.0, -35.0, 70.0, 70.0);
        g.set_color(Color::WHITE);
        g.print_ex("spin", DrawParams::new().centered());
        g.pop();
        let frame = ((t * 6.0) as usize) % self.frames.len();
        g.draw_quad_ex(
            &self.sheet,
            self.frames[frame],
            DrawParams::at(250.0, y + 20.0).scale(4.0),
        );
        g.draw(&self.canvas, 340.0, y - 10.0);
        g.set_color(Color::GRAY);
        g.rectangle(Line, 340.0, y - 10.0, 160.0, 120.0);

        // Blend modes: overlapping circles
        let modes = [
            (BlendMode::Alpha, "alpha"),
            (BlendMode::Add, "add"),
            (BlendMode::Multiply, "multiply"),
        ];
        for (i, (mode, name)) in modes.iter().enumerate() {
            let cx = 560.0 + i as f32 * 85.0;
            g.set_color(Color::hex(0x888888));
            g.rectangle(Fill, cx - 38.0, y - 8.0, 76.0, 90.0);
            g.set_blend_mode(*mode);
            for (j, c) in [Color::RED, Color::GREEN, Color::BLUE].iter().enumerate() {
                let a = j as f32 * TAU / 3.0 - PI / 2.0;
                g.set_color(c.with_alpha(0.7));
                g.circle(Fill, cx + a.cos() * 12.0, y + 30.0 + a.sin() * 12.0, 18.0);
            }
            g.set_blend_mode(BlendMode::Alpha);
            g.set_color(Color::WHITE);
            g.print(name, cx - 30.0, y + 60.0);
        }

        // --- Row 4: text layout and scissor -----------------------------------
        let y = 440.0;
        let text = "printf wraps and aligns text inside a box. All text shares one glyph atlas with shapes.";
        for (i, align) in [Align::Left, Align::Center, Align::Right]
            .into_iter()
            .enumerate()
        {
            let x = 20.0 + i as f32 * 190.0;
            g.set_color(Color::hex(0x33364a));
            g.rectangle(Fill, x, y, 170.0, 120.0);
            g.set_color(Color::WHITE);
            g.printf(text, x + 5.0, y + 5.0, 160.0, align);
        }
        g.set_scissor(600.0, y, 180.0, 120.0);
        for i in 0..14 {
            g.set_color(Color::hsv(i as f32 * 25.0 + t * 60.0, 0.6, 0.9));
            let x = 560.0 + i as f32 * 20.0 + (t * 40.0) % 20.0;
            g.rectangle(Fill, x, y - 20.0, 10.0, 170.0);
        }
        g.reset_scissor();
        g.set_color(Color::WHITE);
        g.rectangle(Line, 600.0, y, 180.0, 120.0);
        g.print("scissor", 610.0, y + 100.0);
    }

    fn key_pressed(&mut self, ctx: &mut Context, key: Key, _repeat: bool) {
        if key == Key::Escape {
            ctx.quit();
        }
    }
}

fn main() -> love2drs::Result {
    love2drs::run(Conf::new("love2drs showcase").msaa(4), Showcase::new)
}
