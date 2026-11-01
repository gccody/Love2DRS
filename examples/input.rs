//! Input tester: shows every connected gamepad, every finger on a touchscreen,
//! and a log of input events. Handy for checking a controller on a new platform.
//!
//! - Move the ball with a left stick, the d-pad, the arrow keys or by dragging.
//! - Press A (bottom face button) to rumble that controller.
//!
//! Run with `cargo run --example input`.

use std::collections::VecDeque;

use love2drs::prelude::*;

const W: f32 = 960.0;
const H: f32 = 640.0;
const LOG_LINES: usize = 10;

struct InputTester {
    ball: Vec2,
    log: VecDeque<String>,
    title: Font,
    small: Font,
}

impl InputTester {
    fn log(&mut self, ctx: &Context, line: String) {
        let t = ctx.timer.time();
        self.log.push_front(format!("{t:7.2}  {line}"));
        self.log.truncate(LOG_LINES);
    }
}

impl Game for InputTester {
    fn update(&mut self, ctx: &mut Context, dt: f32) {
        let kb = &ctx.keyboard;
        let keys = vec2(kb.axis(Key::Left, Key::Right), kb.axis(Key::Up, Key::Down));
        let pad = ctx.gamepad.left_stick() + ctx.gamepad.dpad();
        let dir = (keys + pad).clamp_length_max(1.0);
        self.ball += dir * 400.0 * dt;
        // Dragging with one finger moves the ball too.
        if let [t] = ctx.touch.touches() {
            self.ball += t.delta;
        }
        self.ball = self.ball.clamp(vec2(20.0, 20.0), vec2(W - 20.0, H - 20.0));
    }

    fn draw(&mut self, ctx: &mut Context) {
        let g = &mut ctx.graphics;

        g.set_font(&self.title);
        g.set_color(Color::WHITE);
        g.print("love2drs input tester  (Esc quits)", 20.0, 16.0);
        g.set_font(&self.small);
        g.set_color(Color::hex(0x9aa4b8));
        g.print(
            "Left stick / d-pad / arrows / drag move the ball. A rumbles the controller.",
            20.0,
            40.0,
        );

        // Gamepads.
        let mut y = 74.0;
        if !ctx.gamepad.is_connected() {
            g.set_color(Color::hex(0x9aa4b8));
            g.print("No gamepad connected. Plug one in or pair it.", 20.0, y);
        }
        for pad in ctx.gamepad.iter().take(2) {
            draw_pad(g, &self.small, pad, 20.0, y);
            y += 190.0;
        }
        if ctx.gamepad.count() > 2 {
            g.set_color(Color::hex(0x9aa4b8));
            g.print(&format!("+{} more", ctx.gamepad.count() - 2), 20.0, y);
        }

        // Touches.
        for t in ctx.touch.touches() {
            g.set_color(Color::hex(0x44ccff).with_alpha(0.35));
            g.line(t.start.x, t.start.y, t.position.x, t.position.y);
            g.circle(Fill, t.position.x, t.position.y, 24.0 + 24.0 * t.pressure);
            g.set_color(Color::WHITE);
            g.print(
                &format!("#{}", t.id.index()),
                t.position.x + 30.0,
                t.position.y - 8.0,
            );
        }
        g.set_color(Color::hex(0x9aa4b8));
        g.print(&format!("touches: {}", ctx.touch.count()), W - 120.0, 16.0);

        // Event log.
        let log_y = H - 20.0 - LOG_LINES as f32 * 16.0;
        g.set_color(Color::hex(0x1c2230));
        g.rectangle(Fill, 480.0, log_y - 8.0, W - 500.0, H - log_y - 4.0);
        for (i, line) in self.log.iter().enumerate() {
            let fade = 1.0 - i as f32 / LOG_LINES as f32 * 0.7;
            g.set_color(Color::hex(0xdde3ee).with_alpha(fade));
            g.print(line, 490.0, log_y + i as f32 * 16.0);
        }

        g.set_color(Color::hex(0xff6688));
        g.circle(Fill, self.ball.x, self.ball.y, 20.0);
    }

    fn key_pressed(&mut self, ctx: &mut Context, key: Key, _repeat: bool) {
        if key == Key::Escape {
            ctx.quit();
        }
    }

    fn gamepad_added(&mut self, ctx: &mut Context, id: GamepadId) {
        let name = ctx.gamepad.get(id).map_or("?", |p| p.name()).to_owned();
        self.log(ctx, format!("pad {} added: {name}", id.index()));
    }

    fn gamepad_removed(&mut self, ctx: &mut Context, id: GamepadId) {
        self.log(ctx, format!("pad {} removed", id.index()));
    }

    fn gamepad_pressed(&mut self, ctx: &mut Context, id: GamepadId, button: GamepadButton) {
        self.log(ctx, format!("pad {} pressed {}", id.index(), button.name()));
        if button == GamepadButton::A && !ctx.gamepad.set_vibration(id, 0.6, 0.6, 0.3) {
            self.log(ctx, format!("pad {} can't rumble", id.index()));
        }
    }

    fn gamepad_released(&mut self, ctx: &mut Context, id: GamepadId, button: GamepadButton) {
        self.log(
            ctx,
            format!("pad {} released {}", id.index(), button.name()),
        );
    }

    fn touch_pressed(&mut self, ctx: &mut Context, id: TouchId, x: f32, y: f32) {
        self.log(ctx, format!("touch #{} down at {x:.0},{y:.0}", id.index()));
    }

    fn touch_released(&mut self, ctx: &mut Context, id: TouchId, x: f32, y: f32) {
        self.log(ctx, format!("touch #{} up at {x:.0},{y:.0}", id.index()));
    }
}

/// Draws one controller: name, sticks, triggers and buttons.
fn draw_pad(g: &mut Graphics, font: &Font, pad: &Gamepad, x: f32, y: f32) {
    use GamepadButton as B;

    let on = Color::hex(0x66ee88);
    let off = Color::hex(0x2c3446);
    let lit = |down: bool| if down { on } else { off };

    g.set_font(font);
    g.set_color(Color::WHITE);
    g.print(
        &format!(
            "#{} {}{}",
            pad.id().index(),
            pad.name(),
            if pad.is_vibration_supported() {
                "  (rumble)"
            } else {
                ""
            }
        ),
        x,
        y,
    );

    let top = y + 24.0;
    // Triggers and shoulders.
    for (i, (axis, button)) in [
        (GamepadAxis::LeftTrigger, B::LeftShoulder),
        (GamepadAxis::RightTrigger, B::RightShoulder),
    ]
    .into_iter()
    .enumerate()
    {
        let bx = x + i as f32 * 300.0;
        g.set_color(off);
        g.rectangle(Fill, bx, top, 120.0, 10.0);
        g.set_color(on);
        g.rectangle(Fill, bx, top, 120.0 * pad.axis(axis), 10.0);
        g.set_color(lit(pad.is_down(button)));
        g.rounded_rectangle(Fill, bx, top + 16.0, 120.0, 14.0, 6.0);
    }

    // Sticks.
    for (i, (stick, click)) in [
        (pad.left_stick(), B::LeftStick),
        (pad.right_stick(), B::RightStick),
    ]
    .into_iter()
    .enumerate()
    {
        let cx = x + 60.0 + i as f32 * 200.0;
        let cy = top + 95.0;
        g.set_color(off);
        g.circle(Fill, cx, cy, 40.0);
        g.set_color(lit(pad.is_down(click)));
        g.circle(Fill, cx + stick.x * 30.0, cy + stick.y * 30.0, 12.0);
        g.set_color(Color::hex(0x9aa4b8));
        g.print(
            &format!("{:+.2} {:+.2}", stick.x, stick.y),
            cx - 40.0,
            cy + 46.0,
        );
    }

    // D-pad.
    let (dx, dy) = (x + 160.0, top + 95.0);
    for (b, ox, oy) in [
        (B::DPadUp, 0.0, -18.0),
        (B::DPadDown, 0.0, 18.0),
        (B::DPadLeft, -18.0, 0.0),
        (B::DPadRight, 18.0, 0.0),
    ] {
        g.set_color(lit(pad.is_down(b)));
        g.rectangle(Fill, dx + ox - 8.0, dy + oy - 8.0, 16.0, 16.0);
    }

    // Face buttons, Xbox colors.
    let (fx, fy) = (x + 360.0, top + 95.0);
    for (b, ox, oy, color) in [
        (B::A, 0.0, 22.0, 0x66ee88),
        (B::B, 22.0, 0.0, 0xff5566),
        (B::X, -22.0, 0.0, 0x4499ff),
        (B::Y, 0.0, -22.0, 0xffcc33),
    ] {
        let c = Color::hex(color);
        g.set_color(if pad.is_down(b) { c } else { c.with_alpha(0.2) });
        g.circle(Fill, fx + ox, fy + oy, 10.0);
    }

    // Back / guide / start.
    for (i, b) in [B::Back, B::Guide, B::Start].into_iter().enumerate() {
        g.set_color(lit(pad.is_down(b)));
        g.circle(Fill, x + 200.0 + (i as f32 - 1.0) * 30.0, top + 40.0, 7.0);
    }
}

fn main() -> love2drs::Result {
    let conf = Conf::new("love2drs input tester").size(W as u32, H as u32);
    love2drs::run(conf, |ctx| {
        ctx.graphics.set_background_color(Color::hex(0x121620));
        Ok(InputTester {
            ball: vec2(W / 2.0, H / 2.0),
            log: VecDeque::new(),
            title: ctx.graphics.new_font(18.0),
            small: ctx.graphics.new_font(13.0),
        })
    })
}
