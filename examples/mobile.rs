//! A tour of what a phone adds: touch, rotation, the safe area, packaged assets,
//! sound, saving, the on-screen keyboard and controllers. Runs on desktop too,
//! where the mouse acts as a finger.
//!
//! - Drag to move the logo. Each finger gets its own ring.
//! - Tap to play a sound and count up. The count survives restarts.
//! - Tap "Keyboard" to type, and Return or the back gesture to stop.
//! - Connect a controller: the left stick moves the logo, and A makes it rumble.
//! - On Android, the back gesture arrives as `Key::Escape`.
//!
//! ```sh
//! cargo run --example mobile                        # desktop
//! cargo love2drs android run --example mobile       # phone or emulator
//! cargo love2drs ios run --example mobile           # simulator, on a Mac
//! ```

use love2drs::prelude::*;

/// Written with `std::fs`: on mobile the working directory is the save directory.
const SAVE_FILE: &str = "mobile-example-taps.txt";
const BUTTON_W: f32 = 120.0;
const BUTTON_H: f32 = 44.0;
const STICK_SPEED: f32 = 400.0;

struct Mobile {
    logo: Image,
    logo_pos: Vec2,
    beep: Source,
    taps: u32,
    suspensions: u32,
    typed: String,
    last_pad_event: String,
}

/// The keyboard button, in the bottom corner of the safe area.
fn keyboard_button(ctx: &Context) -> (f32, f32) {
    let (x, y, w, h) = ctx.window.safe_area();
    (x + w - BUTTON_W - 12.0, y + h - BUTTON_H - 12.0)
}

impl Mobile {
    fn load(ctx: &mut Context) -> Result<Self> {
        let (x, y, w, h) = ctx.window.safe_area();
        // Listed under `assets` in Cargo.toml, so it's packaged into the app.
        let logo = ctx.graphics.new_image("assets/mobile/logo.png")?;
        let beep = SoundData::generate(44_100, 0.08, |t| {
            (t * 880.0 * std::f32::consts::TAU).sin() * (1.0 - t / 0.08) * 0.4
        });
        // Saved data isn't packaged, so it goes through plain `std::fs`.
        let taps = std::fs::read_to_string(SAVE_FILE)
            .ok()
            .and_then(|text| text.trim().parse().ok())
            .unwrap_or(0);
        Ok(Mobile {
            logo,
            logo_pos: vec2(x + w / 2.0, y + h / 2.0),
            beep: ctx.audio.new_source_from_data(beep),
            taps,
            suspensions: 0,
            typed: String::new(),
            last_pad_event: String::new(),
        })
    }

    fn save(&self) {
        if let Err(e) = std::fs::write(SAVE_FILE, self.taps.to_string()) {
            log::warn!("can't save: {e}");
        }
    }
}

impl Game for Mobile {
    fn update(&mut self, ctx: &mut Context, dt: f32) {
        if let [finger] = ctx.touch.touches() {
            self.logo_pos += finger.delta;
        }
        self.logo_pos += ctx.gamepad.left_stick() * STICK_SPEED * dt;
        // Rotating the phone can leave the logo off screen. Bring it back.
        let (x, y, w, h) = ctx.window.safe_area();
        self.logo_pos.x = self.logo_pos.x.clamp(x, x + w);
        self.logo_pos.y = self.logo_pos.y.clamp(y, y + h);
    }

    fn draw(&mut self, ctx: &mut Context) {
        let (x, y, w, h) = ctx.window.safe_area();
        let (button_x, button_y) = keyboard_button(ctx);
        let (screen_w, screen_h) = (ctx.graphics.width(), ctx.graphics.height());
        let scale = ctx.window.scale_factor();
        let fps = ctx.timer.fps();
        let pad = ctx.gamepad.first();
        let mut lines = vec![
            format!("Tapped {} times", self.taps),
            format!("screen {screen_w:.0} x {screen_h:.0} @ {scale:.2}x, {fps:.0} fps"),
            format!("safe area {x:.0},{y:.0} {w:.0} x {h:.0}"),
            format!("suspended {} times", self.suspensions),
            format!("audio output: {}", ctx.audio.is_available()),
            format!("typed: {}", self.typed),
        ];
        match pad {
            Some(pad) => {
                let rumble = if pad.is_vibration_supported() {
                    "rumble"
                } else {
                    "no rumble"
                };
                let stick = pad.left_stick();
                let right = pad.right_stick();
                let lt = pad.axis(GamepadAxis::LeftTrigger);
                let rt = pad.axis(GamepadAxis::RightTrigger);
                lines.push(format!("pad: {}, {rumble}", pad.name()));
                lines.push(format!(
                    "L {:.2},{:.2} R {:.2},{:.2} T {lt:.2},{rt:.2}",
                    stick.x, stick.y, right.x, right.y
                ));
                lines.push(self.last_pad_event.clone());
            }
            None => lines.push("no controller".to_string()),
        }
        let fingers: Vec<Vec2> = ctx.touch.touches().iter().map(|t| t.position).collect();

        let g = &mut ctx.graphics;
        g.clear(Color::hex(0x1b2333));

        // Everything inside the safe area can be seen and reached.
        g.set_color(Color::hex(0x2c3a55));
        g.rectangle(Fill, x, y, w, h);

        g.set_color(Color::WHITE);
        let size = self.logo.width() as f32;
        let logo_scale = (w.min(h) * 0.4) / size;
        g.draw_ex(
            &self.logo,
            DrawParams::at(self.logo_pos.x, self.logo_pos.y)
                .scale(logo_scale)
                .centered(),
        );

        for (i, line) in lines.iter().enumerate() {
            g.print(line, x + 12.0, y + 12.0 + i as f32 * 20.0);
        }

        g.set_color(Color::hex(0x4a5f8a));
        g.rounded_rectangle(Fill, button_x, button_y, BUTTON_W, BUTTON_H, 8.0);
        g.set_color(Color::WHITE);
        g.printf(
            "Keyboard",
            button_x,
            button_y + 14.0,
            BUTTON_W,
            Align::Center,
        );

        g.set_color(Color::hex(0xff6688));
        g.set_line_width(3.0);
        for finger in fingers {
            g.circle(Line, finger.x, finger.y, 44.0);
        }
    }

    fn touch_pressed(&mut self, ctx: &mut Context, _id: TouchId, x: f32, y: f32) {
        let (button_x, button_y) = keyboard_button(ctx);
        let on_button =
            x >= button_x && x <= button_x + BUTTON_W && y >= button_y && y <= button_y + BUTTON_H;
        if on_button {
            ctx.keyboard.set_screen_keyboard(true);
            return;
        }
        self.taps += 1;
        self.beep.play_overlapping();
    }

    fn text_input(&mut self, _ctx: &mut Context, text: &str) {
        self.typed.push_str(text);
    }

    // The OS can end a suspended app at any time, so this is the moment to save.
    fn suspended(&mut self, _ctx: &mut Context) {
        self.suspensions += 1;
        self.save();
    }

    fn quit(&mut self, _ctx: &mut Context) -> bool {
        self.save();
        false
    }

    fn key_pressed(&mut self, ctx: &mut Context, key: Key, _repeat: bool) {
        match key {
            Key::Escape => ctx.quit(),
            Key::Backspace => {
                self.typed.pop();
            }
            Key::Enter => ctx.keyboard.set_screen_keyboard(false),
            _ => {}
        }
    }

    fn gamepad_pressed(&mut self, ctx: &mut Context, id: GamepadId, button: GamepadButton) {
        self.last_pad_event = format!("pressed {}", button.name());
        if button == GamepadButton::A {
            ctx.gamepad.set_vibration(id, 1.0, 0.5, 0.3);
        }
    }

    fn gamepad_released(&mut self, _ctx: &mut Context, _id: GamepadId, button: GamepadButton) {
        self.last_pad_event = format!("released {}", button.name());
    }
}

fn main() -> love2drs::Result {
    love2drs::run(Conf::new("love2drs mobile").resizable(true), Mobile::load)
}

love2drs::android_main!(main);
