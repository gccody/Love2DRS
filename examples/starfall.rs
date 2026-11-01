//! STARFALL: catch falling stars, dodge meteors, chain combos.
//!
//! Controls: Left/Right or A/D to move, Space to dash, Enter to start,
//! Escape to quit. All graphics are vector shapes and every sound is
//! synthesized at startup, so there are no asset files.
//!
//! Run with `cargo run --release --example starfall`.

use std::f32::consts::{PI, TAU};

use love2drs::prelude::*;

const W: f32 = 800.0;
const H: f32 = 600.0;
const PLAYER_Y: f32 = H - 70.0;
const PLAYER_R: f32 = 16.0;
const PLAYER_SPEED: f32 = 420.0;
const DASH_SPEED: f32 = 1300.0;
const DASH_TIME: f32 = 0.14;
const DASH_COOLDOWN: f32 = 0.8;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Star,
    Meteor,
    Shield,
}

struct Faller {
    pos: Vec2,
    vel: Vec2,
    kind: Kind,
    radius: f32,
    angle: f32,
    spin: f32,
    /// Per-vertex radius multipliers for lumpy meteors.
    shape: [f32; 9],
}

struct Particle {
    pos: Vec2,
    vel: Vec2,
    life: f32,
    max_life: f32,
    size: f32,
    color: Color,
}

struct BgStar {
    pos: Vec2,
    depth: f32,
}

#[derive(PartialEq)]
enum State {
    Title,
    Playing,
    GameOver,
}

struct Sounds {
    collect: Source,
    hit: Source,
    dash: Source,
    shield: Source,
}

struct Starfall {
    state: State,
    time: f32,
    elapsed: f32,
    player_x: f32,
    facing: f32,
    dash_timer: f32,
    dash_dir: f32,
    dash_cooldown: f32,
    shield: f32,
    invulnerable: f32,
    lives: u32,
    score: u32,
    best: u32,
    combo: u32,
    combo_timer: f32,
    spawn_timer: f32,
    shake: f32,
    fallers: Vec<Faller>,
    particles: Vec<Particle>,
    bg: Vec<BgStar>,
    popups: Vec<(Vec2, String, f32)>,
    sounds: Sounds,
    big: Font,
    medium: Font,
}

/// A short synthesized tone with a pitch sweep and decay envelope.
fn tone(start_hz: f32, end_hz: f32, seconds: f32, noise: f32) -> SoundData {
    let mut phase = 0.0f32;
    let mut rng = Rng::new(7);
    SoundData::generate(44_100, seconds, move |t| {
        let k = t / seconds;
        let hz = start_hz + (end_hz - start_hz) * k;
        phase += TAU * hz / 44_100.0;
        let env = (1.0 - k).powi(2) * (t * 400.0).min(1.0);
        let n = (rng.random() * 2.0 - 1.0) * noise;
        (phase.sin() * (1.0 - noise) + n) * env * 0.35
    })
}

fn star_points(c: Vec2, r: f32, angle: f32) -> Vec<f32> {
    // Center first, then the outline closed back on itself, so a fan fill
    // works even though the star is concave.
    let mut pts = vec![c.x, c.y];
    for i in 0..=10 {
        let rr = if i % 2 == 0 { r } else { r * 0.45 };
        let a = angle + i as f32 * PI / 5.0 - PI / 2.0;
        pts.extend_from_slice(&[c.x + a.cos() * rr, c.y + a.sin() * rr]);
    }
    pts
}

impl Starfall {
    fn new(ctx: &mut Context) -> Result<Self> {
        let audio = &ctx.audio;
        let sounds = Sounds {
            collect: audio.new_source_from_data(tone(660.0, 1320.0, 0.12, 0.0)),
            hit: audio.new_source_from_data(tone(180.0, 40.0, 0.45, 0.6)),
            dash: audio.new_source_from_data(tone(300.0, 900.0, 0.1, 0.3)),
            shield: audio.new_source_from_data(tone(440.0, 1760.0, 0.35, 0.0)),
        };
        sounds.dash.set_volume(0.5);
        let bg = (0..140)
            .map(|_| BgStar {
                pos: vec2(random_range(0.0, W), random_range(0.0, H)),
                depth: random_range(0.2, 1.0),
            })
            .collect();
        Ok(Starfall {
            state: State::Title,
            time: 0.0,
            elapsed: 0.0,
            player_x: W / 2.0,
            facing: 0.0,
            dash_timer: 0.0,
            dash_dir: 1.0,
            dash_cooldown: 0.0,
            shield: 0.0,
            invulnerable: 0.0,
            lives: 3,
            score: 0,
            best: 0,
            combo: 0,
            combo_timer: 0.0,
            spawn_timer: 0.0,
            shake: 0.0,
            fallers: Vec::new(),
            particles: Vec::new(),
            bg,
            popups: Vec::new(),
            sounds,
            big: ctx.graphics.new_font(64.0),
            medium: ctx.graphics.new_font(22.0),
        })
    }

    fn reset(&mut self) {
        self.state = State::Playing;
        self.elapsed = 0.0;
        self.player_x = W / 2.0;
        self.dash_timer = 0.0;
        self.dash_cooldown = 0.0;
        self.shield = 0.0;
        self.invulnerable = 1.0;
        self.lives = 3;
        self.score = 0;
        self.combo = 0;
        self.spawn_timer = 0.5;
        self.fallers.clear();
        self.popups.clear();
    }

    fn burst(&mut self, at: Vec2, color: Color, count: usize, speed: f32) {
        for _ in 0..count {
            let a = random_range(0.0, TAU);
            let s = random_range(0.2, 1.0) * speed;
            let life = random_range(0.3, 0.8);
            self.particles.push(Particle {
                pos: at,
                vel: vec2(a.cos(), a.sin()) * s,
                life,
                max_life: life,
                size: random_range(2.0, 5.0),
                color,
            });
        }
    }

    fn spawn(&mut self) {
        let difficulty = 1.0 + self.elapsed / 30.0;
        let roll = random();
        let kind = if roll < 0.04 {
            Kind::Shield
        } else if roll < 0.04 + 0.35 * difficulty.min(1.8) {
            Kind::Meteor
        } else {
            Kind::Star
        };
        let radius = match kind {
            Kind::Star => 14.0,
            Kind::Shield => 13.0,
            Kind::Meteor => random_range(16.0, 30.0),
        };
        let speed = random_range(140.0, 220.0) * difficulty.min(2.5);
        let mut shape = [1.0; 9];
        shape.iter_mut().for_each(|s| *s = random_range(0.75, 1.1));
        self.fallers.push(Faller {
            pos: vec2(random_range(30.0, W - 30.0), -40.0),
            vel: vec2(random_range(-40.0, 40.0), speed),
            kind,
            radius,
            angle: random_range(0.0, TAU),
            spin: random_range(-2.0, 2.0),
            shape,
        });
    }

    fn update_playing(&mut self, ctx: &mut Context, dt: f32) {
        self.elapsed += dt;
        let kb = &ctx.keyboard;
        let input = kb.axis(Key::Left, Key::Right) + kb.axis(Key::A, Key::D);
        let input = input.clamp(-1.0, 1.0);
        if input != 0.0 {
            self.facing = input;
        }

        self.dash_cooldown -= dt;
        if kb.was_pressed(Key::Space) && self.dash_cooldown <= 0.0 {
            self.dash_timer = DASH_TIME;
            self.dash_dir = if input != 0.0 {
                input
            } else if self.facing != 0.0 {
                self.facing
            } else {
                1.0
            };
            self.dash_cooldown = DASH_COOLDOWN;
            self.sounds.dash.play_overlapping();
        }
        let vx = if self.dash_timer > 0.0 {
            self.dash_timer -= dt;
            let p = vec2(self.player_x, PLAYER_Y);
            self.particles.push(Particle {
                pos: p,
                vel: vec2(-self.dash_dir * 60.0, random_range(-30.0, 30.0)),
                life: 0.25,
                max_life: 0.25,
                size: 6.0,
                color: Color::hex(0x7df9ff),
            });
            self.dash_dir * DASH_SPEED
        } else {
            input * PLAYER_SPEED
        };
        self.player_x = (self.player_x + vx * dt).clamp(PLAYER_R, W - PLAYER_R);

        self.spawn_timer -= dt;
        if self.spawn_timer <= 0.0 {
            self.spawn();
            self.spawn_timer = (0.75 - self.elapsed * 0.006).max(0.22) * random_range(0.6, 1.2);
        }

        self.shield = (self.shield - dt).max(0.0);
        self.invulnerable = (self.invulnerable - dt).max(0.0);
        self.combo_timer -= dt;
        if self.combo_timer <= 0.0 {
            self.combo = 0;
        }

        let player = vec2(self.player_x, PLAYER_Y);
        let mut events = Vec::new();
        self.fallers.retain_mut(|f| {
            f.pos += f.vel * dt;
            f.angle += f.spin * dt;
            if f.pos.distance(player) < f.radius + PLAYER_R * 0.8 {
                events.push((f.kind, f.pos));
                return false;
            }
            f.pos.y < H + 50.0
        });

        for (kind, pos) in events {
            match kind {
                Kind::Star => {
                    self.combo += 1;
                    self.combo_timer = 1.5;
                    let gain = 10 * self.combo.min(10);
                    self.score += gain;
                    self.popups.push((pos, format!("+{gain}"), 0.8));
                    self.burst(pos, Color::hex(0xffe066), 18, 220.0);
                    self.sounds
                        .collect
                        .set_pitch(1.0 + self.combo.min(12) as f32 * 0.06);
                    self.sounds.collect.play_overlapping();
                }
                Kind::Shield => {
                    self.shield = 6.0;
                    self.burst(pos, Color::hex(0x7df9ff), 30, 260.0);
                    self.popups.push((pos, "SHIELD".into(), 1.0));
                    self.sounds.shield.play_overlapping();
                }
                Kind::Meteor => {
                    self.burst(pos, Color::hex(0xff6b3d), 40, 320.0);
                    if self.shield > 0.0 {
                        self.score += 25;
                        self.popups.push((pos, "+25".into(), 0.8));
                        self.sounds.collect.play_overlapping();
                    } else if self.invulnerable <= 0.0 {
                        self.lives = self.lives.saturating_sub(1);
                        self.combo = 0;
                        self.shake = 0.5;
                        self.invulnerable = 1.5;
                        self.sounds.hit.play_overlapping();
                        if self.lives == 0 {
                            self.best = self.best.max(self.score);
                            self.state = State::GameOver;
                            self.burst(player, Color::hex(0x7df9ff), 80, 400.0);
                        }
                    }
                }
            }
        }
    }

    fn draw_player(&self, g: &mut Graphics) {
        if self.invulnerable > 0.0 && (self.time * 20.0) as i32 % 2 == 0 {
            return;
        }
        let tilt = if self.dash_timer > 0.0 {
            self.dash_dir * 0.5
        } else {
            self.facing * 0.12
        };
        g.push();
        g.translate(self.player_x, PLAYER_Y);
        g.rotate(tilt);
        // Engine glow
        g.set_blend_mode(BlendMode::Add);
        let flicker = 0.7 + 0.3 * (self.time * 40.0).sin();
        g.set_color(Color::hex(0xff9f1c).with_alpha(0.5 * flicker));
        g.ellipse(Fill, 0.0, 16.0, 6.0, 12.0 * flicker);
        g.set_blend_mode(BlendMode::Alpha);
        // Hull
        g.set_color(Color::hex(0xe0fbfc));
        g.polygon(Fill, &[0.0, -20.0, 16.0, 14.0, 0.0, 6.0, -16.0, 14.0]);
        g.set_color(Color::hex(0x3d5a80));
        g.polygon(Fill, &[0.0, -8.0, 5.0, 4.0, -5.0, 4.0]);
        g.pop();

        if self.shield > 0.0 {
            let fade = (self.shield / 1.0).min(1.0);
            g.set_blend_mode(BlendMode::Add);
            g.set_color(Color::hex(0x7df9ff).with_alpha(0.15 * fade));
            g.circle(Fill, self.player_x, PLAYER_Y, 30.0);
            g.set_color(Color::hex(0x7df9ff).with_alpha(0.7 * fade));
            g.set_line_width(2.0);
            g.circle(
                Line,
                self.player_x,
                PLAYER_Y,
                30.0 + (self.time * 6.0).sin() * 2.0,
            );
            g.set_line_width(1.0);
            g.set_blend_mode(BlendMode::Alpha);
        }
    }

    fn draw_fallers(&self, g: &mut Graphics) {
        // Glows first in one additive pass, so they batch together.
        g.set_blend_mode(BlendMode::Add);
        for f in &self.fallers {
            let c = match f.kind {
                Kind::Star => Color::hex(0xffd166),
                Kind::Shield => Color::hex(0x7df9ff),
                Kind::Meteor => Color::hex(0xff6b3d),
            };
            g.set_color(c.with_alpha(0.12));
            g.circle(Fill, f.pos.x, f.pos.y, f.radius * 2.2);
            g.circle(Fill, f.pos.x, f.pos.y, f.radius * 1.5);
        }
        g.set_blend_mode(BlendMode::Alpha);

        for f in &self.fallers {
            match f.kind {
                Kind::Star => {
                    g.set_color(Color::hex(0xffe066));
                    g.polygon(Fill, &star_points(f.pos, f.radius, f.angle));
                }
                Kind::Shield => {
                    g.set_color(Color::hex(0x7df9ff));
                    g.set_line_width(3.0);
                    g.circle(Line, f.pos.x, f.pos.y, f.radius);
                    g.set_line_width(1.0);
                    g.circle(Fill, f.pos.x, f.pos.y, f.radius * 0.45);
                }
                Kind::Meteor => {
                    // Fiery trail
                    for i in 1..6 {
                        let t = i as f32 / 6.0;
                        g.set_color(Color::hex(0xff6b3d).with_alpha(0.25 * (1.0 - t)));
                        let p = f.pos - f.vel.normalize() * f.radius * 1.6 * t * 2.0;
                        g.circle(Fill, p.x, p.y, f.radius * (1.0 - t * 0.7));
                    }
                    let pts: Vec<f32> = f
                        .shape
                        .iter()
                        .enumerate()
                        .flat_map(|(i, m)| {
                            let a = f.angle + i as f32 * TAU / f.shape.len() as f32;
                            [
                                f.pos.x + a.cos() * f.radius * m,
                                f.pos.y + a.sin() * f.radius * m,
                            ]
                        })
                        .collect();
                    g.set_color(Color::hex(0x6d4c41));
                    g.polygon(Fill, &pts);
                    g.set_color(Color::hex(0x3e2723));
                    g.set_line_width(2.0);
                    g.polygon(Line, &pts);
                    g.set_line_width(1.0);
                    g.set_color(Color::hex(0x8d6e63));
                    g.circle(
                        Fill,
                        f.pos.x - f.radius * 0.3,
                        f.pos.y - f.radius * 0.2,
                        f.radius * 0.22,
                    );
                }
            }
        }
    }

    fn draw_hud(&self, g: &mut Graphics) {
        g.set_font(&self.medium);
        g.set_color(Color::WHITE);
        g.print(&format!("SCORE {}", self.score), 20.0, 16.0);
        g.set_color(Color::GRAY);
        g.print(&format!("BEST {}", self.best.max(self.score)), 20.0, 44.0);
        if self.combo > 1 {
            let pulse = 1.0 + (self.combo_timer * 10.0).sin().abs() * 0.08;
            g.set_color(Color::hex(0xffd166));
            g.print_ex(
                &format!("x{} COMBO", self.combo),
                DrawParams::at(W / 2.0, 30.0).centered().scale(pulse),
            );
        }
        for i in 0..3 {
            let x = W - 30.0 - i as f32 * 28.0;
            if (i as u32) < self.lives {
                g.set_color(Color::hex(0xef476f));
                g.circle(Fill, x - 5.0, 28.0, 7.0);
                g.circle(Fill, x + 5.0, 28.0, 7.0);
                g.polygon(Fill, &[x - 12.0, 30.0, x + 12.0, 30.0, x, 44.0]);
            } else {
                g.set_color(Color::rgba(1.0, 1.0, 1.0, 0.2));
                g.circle(Line, x, 32.0, 9.0);
            }
        }
        // Dash cooldown bar
        let ready = 1.0 - (self.dash_cooldown / DASH_COOLDOWN).clamp(0.0, 1.0);
        g.set_color(Color::rgba(1.0, 1.0, 1.0, 0.15));
        g.rounded_rectangle(Fill, W - 116.0, 54.0, 96.0, 8.0, 4.0);
        g.set_color(if ready >= 1.0 {
            Color::hex(0x7df9ff)
        } else {
            Color::GRAY
        });
        g.rounded_rectangle(Fill, W - 116.0, 54.0, 96.0 * ready, 8.0, 4.0);
    }

    fn draw_centered(
        &self,
        g: &mut Graphics,
        font: &Font,
        text: &str,
        y: f32,
        color: Color,
    ) {
        g.set_font(font);
        g.set_color(color);
        g.printf(text, 0.0, y, W, Align::Center);
    }
}

impl Game for Starfall {
    fn update(&mut self, ctx: &mut Context, dt: f32) {
        self.time += dt;
        self.shake = (self.shake - dt).max(0.0);

        for s in &mut self.bg {
            s.pos.y += (20.0 + 80.0 * s.depth) * dt;
            if s.pos.y > H {
                s.pos = vec2(random_range(0.0, W), 0.0);
            }
        }
        self.particles.retain_mut(|p| {
            p.life -= dt;
            p.pos += p.vel * dt;
            p.vel *= 1.0 - 2.5 * dt;
            p.life > 0.0
        });
        self.popups.retain_mut(|(pos, _, life)| {
            *life -= dt;
            pos.y -= 40.0 * dt;
            *life > 0.0
        });

        match self.state {
            State::Playing => self.update_playing(ctx, dt),
            State::Title | State::GameOver => {
                if ctx.keyboard.was_pressed(Key::Enter) || ctx.keyboard.was_pressed(Key::Space) {
                    self.reset();
                }
                // Keep some ambience falling on menus.
                self.fallers.retain_mut(|f| {
                    f.pos += f.vel * dt;
                    f.angle += f.spin * dt;
                    f.pos.y < H + 50.0
                });
            }
        }
    }

    fn draw(&mut self, ctx: &mut Context) {
        let g = &mut ctx.graphics;
        g.set_background_color(Color::hex(0x0b0d17));

        // Screen shake applies to the world, not the HUD.
        g.push();
        if self.shake > 0.0 {
            let k = self.shake * 16.0;
            g.translate(random_range(-k, k), random_range(-k, k));
        }

        for s in &self.bg {
            g.set_color(Color::rgba(0.7, 0.8, 1.0, s.depth * 0.8));
            let size = 1.0 + s.depth * 1.5;
            g.rectangle(Fill, s.pos.x, s.pos.y, size, size);
        }
        // Ground horizon glow
        g.set_color(Color::hex(0x1b2a4a).with_alpha(0.6));
        g.rectangle(Fill, 0.0, PLAYER_Y + 30.0, W, H - PLAYER_Y - 30.0);

        self.draw_fallers(g);
        if self.state == State::Playing {
            self.draw_player(g);
        }

        g.set_blend_mode(BlendMode::Add);
        for p in &self.particles {
            let k = p.life / p.max_life;
            g.set_color(p.color.with_alpha(k));
            g.rectangle(
                Fill,
                p.pos.x - p.size * 0.5,
                p.pos.y - p.size * 0.5,
                p.size * k,
                p.size * k,
            );
        }
        g.set_blend_mode(BlendMode::Alpha);
        g.pop();

        g.set_font(&self.medium);
        for (pos, text, life) in &self.popups {
            g.set_color(Color::hex(0xffe066).with_alpha(life.min(1.0)));
            g.print_ex(text, DrawParams::at(pos.x, pos.y).centered());
        }

        match self.state {
            State::Playing => self.draw_hud(g),
            State::Title => {
                let bob = (self.time * 2.0).sin() * 6.0;
                self.draw_centered(g, &self.big, "STARFALL", 170.0 + bob, Color::hex(0xffd166));
                self.draw_centered(
                    g,
                    &self.medium,
                    "Catch stars. Dodge meteors. Chain combos.",
                    270.0,
                    Color::hex(0xe0fbfc),
                );
                self.draw_centered(
                    g,
                    &self.medium,
                    "Move: Arrows / A D    Dash: Space",
                    320.0,
                    Color::GRAY,
                );
                if (self.time * 2.0) as i32 % 2 == 0 {
                    self.draw_centered(
                        g,
                        &self.medium,
                        "Press Enter to start",
                        420.0,
                        Color::WHITE,
                    );
                }
            }
            State::GameOver => {
                self.draw_centered(g, &self.big, "GAME OVER", 170.0, Color::hex(0xef476f));
                self.draw_centered(
                    g,
                    &self.medium,
                    &format!("Score {}", self.score),
                    270.0,
                    Color::WHITE,
                );
                self.draw_centered(
                    g,
                    &self.medium,
                    &format!("Best {}", self.best),
                    305.0,
                    Color::GRAY,
                );
                if (self.time * 2.0) as i32 % 2 == 0 {
                    self.draw_centered(
                        g,
                        &self.medium,
                        "Press Enter to play again",
                        420.0,
                        Color::WHITE,
                    );
                }
            }
        }
    }

    fn key_pressed(&mut self, ctx: &mut Context, key: Key, _repeat: bool) {
        match key {
            Key::Escape => ctx.quit(),
            Key::F11 => {
                let fs = ctx.window.is_fullscreen();
                ctx.window.set_fullscreen(!fs);
            }
            _ => {}
        }
    }
}

fn main() -> love2drs::Result {
    love2drs::run(
        Conf::new("Starfall").size(W as u32, H as u32).msaa(4),
        |ctx| {
            let mut game = Starfall::new(ctx)?;
            // Seed the title screen with some falling objects.
            for _ in 0..6 {
                game.spawn();
                if let Some(f) = game.fallers.last_mut() {
                    f.pos.y = random_range(0.0, H);
                }
            }
            Ok(game)
        },
    )
}
