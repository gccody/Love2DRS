//! MOSSY CAVERNS: a tiny pixel-art platformer.
//!
//! Shows the classic low-resolution pipeline: the world is drawn into a
//! 320x180 canvas with nearest-neighbour filtering, then scaled up by an
//! integer factor. Tiles live in a `SpriteBatch` built once at load time, the
//! player is an animated sprite sheet, and the sky is a vertex-colored mesh.
//! All art and sound is generated in code.
//!
//! Controls: Left/Right or A/D to run, Space/Up/W to jump (hold to jump
//! higher), R to restart, Escape to quit.
//!
//! Run with `cargo run --release --example platformer`.

use love2drs::graphics::Graphics;
use love2drs::math::{Rng, approach};
use love2drs::prelude::*;

const VIEW_W: u32 = 320;
const VIEW_H: u32 = 180;
const TILE: f32 = 16.0;

const PW: f32 = 10.0;
const PH: f32 = 14.0;
const RUN_SPEED: f32 = 110.0;
const GRAVITY: f32 = 980.0;
const JUMP_SPEED: f32 = 315.0;
const MAX_FALL: f32 = 420.0;
const COYOTE: f32 = 0.1;
const JUMP_BUFFER: f32 = 0.12;

const LEVEL: [&str; 13] = [
    "########################################################################",
    "#                                                                      #",
    "#                                                                      #",
    "#                                                                      #",
    "#                                                        o             #",
    "#                                                           o o   F    #",
    "#                                  o       o    o         ##############",
    "#              o       o o                              # ##############",
    "#         o        o  =====  o     ##     ===  ===     ## ##############",
    "#  P  o        ##                  ##    ^^^^^^^^^^ o ### ##############",
    "#########   ######   #######   #########################################",
    "#########^^^######^^^#######^^^#########################################",
    "########################################################################",
];

// Tileset layout (one row of 16x16 tiles).
const T_STONE: usize = 0;
const T_GRASS: usize = 1;
const T_SPIKES: usize = 2;
const T_PLANK: usize = 3;

// Player sprite sheet frames.
const F_IDLE: usize = 0;
const F_RUN1: usize = 1;
const F_RUN2: usize = 2;
const F_JUMP: usize = 3;

#[derive(PartialEq)]
enum State {
    Playing,
    Won,
}

struct Player {
    pos: Vec2,
    vel: Vec2,
    on_ground: bool,
    coyote: f32,
    jump_buffer: f32,
    facing_left: bool,
    anim: f32,
    dead: f32,
}

struct Particle {
    pos: Vec2,
    vel: Vec2,
    life: f32,
    color: Color,
}

struct Sfx {
    jump: Source,
    gem: Source,
    die: Source,
    win: Source,
}

struct Platformer {
    canvas: Canvas,
    tiles: SpriteBatch,
    player_sheet: Image,
    player_frames: Vec<Quad>,
    grid: Vec<Vec<u8>>,
    spawn: Vec2,
    gems: Vec<(Vec2, bool)>,
    flag: Vec2,
    player: Player,
    camera: Vec2,
    particles: Vec<Particle>,
    state: State,
    time: f32,
    run_time: f32,
    deaths: u32,
    sfx: Sfx,
    hud_font: Font,
    big_font: Font,
}

// ---------------------------------------------------------------------------
// Procedural art & sound
// ---------------------------------------------------------------------------

fn make_tileset() -> ImageData {
    let mut rng = Rng::new(1234);
    let stone = |rng: &mut Rng, x: u32, y: u32| {
        let edge = x == 0 || y == 0 || x == 15 || y == 15;
        let n = rng.random() * 0.08;
        if edge {
            Color::rgb(0.16 + n, 0.13 + n, 0.22 + n)
        } else if (x + y * 3).is_multiple_of(7) {
            Color::rgb(0.33 + n, 0.29 + n, 0.42 + n)
        } else {
            Color::rgb(0.24 + n, 0.2 + n, 0.32 + n)
        }
    };
    ImageData::from_fn(64, 16, |px, y| {
        let tile = (px / 16) as usize;
        let x = px % 16;
        match tile {
            T_STONE => stone(&mut rng, x, y),
            T_GRASS => {
                let blade = [3, 4, 2, 5, 3, 4, 3, 2, 4, 5, 3, 2, 4, 3, 5, 3][x as usize];
                if y < blade {
                    Color::rgb(0.35, 0.75 + rng.random() * 0.1, 0.35)
                } else if y == blade {
                    Color::rgb(0.2, 0.5, 0.25)
                } else {
                    stone(&mut rng, x, y)
                }
            }
            T_SPIKES => {
                // Three spikes, 8 px tall, sitting on the bottom of the tile.
                let local = (x % 5) as i32 - 2;
                let height = 8 - local.abs() * 3;
                if x < 15 && y >= 16 - height.max(0) as u32 && y >= 8 {
                    if local <= 0 {
                        Color::hex(0xd8dee9)
                    } else {
                        Color::hex(0x8f99ab)
                    }
                } else {
                    Color::TRANSPARENT
                }
            }
            T_PLANK => {
                if y < 2 {
                    Color::hex(0xc08552)
                } else if y < 5 {
                    if x % 8 == 0 {
                        Color::hex(0x5e3023)
                    } else {
                        Color::hex(0x895737)
                    }
                } else if (y < 9) && (x == 2 || x == 13) {
                    Color::hex(0x5e3023)
                } else {
                    Color::TRANSPARENT
                }
            }
            _ => Color::TRANSPARENT,
        }
    })
}

fn make_player_sheet() -> ImageData {
    ImageData::from_fn(64, 16, |px, y| {
        let frame = (px / 16) as usize;
        let x = (px % 16) as i32;
        let y = y as i32;
        let body = Color::hex(0x4ecdc4);
        let dark = Color::hex(0x1a535c);
        // Body with a dark outline.
        if (3..13).contains(&x) && (2..13).contains(&y) {
            let edge = x == 3 || x == 12 || y == 2 || y == 12;
            if edge {
                return dark;
            }
            if (y == 6 || y == 7) && (x == 7 || x == 10) {
                return if y == 6 {
                    Color::WHITE
                } else {
                    Color::hex(0x10141c)
                };
            }
            if y >= 10 {
                return Color::hex(0x3bb3aa);
            }
            return body;
        }
        // Legs differ per frame.
        let legs: &[(i32, i32)] = match frame {
            F_IDLE => &[(5, 13), (5, 14), (10, 13), (10, 14)],
            F_RUN1 => &[(4, 13), (3, 14), (10, 13), (11, 14)],
            F_RUN2 => &[(6, 13), (6, 14), (9, 13), (8, 14)],
            _ => &[(5, 13), (10, 13)],
        };
        if legs.contains(&(x, y)) {
            dark
        } else {
            Color::TRANSPARENT
        }
    })
}

fn tone(hz: f32, end_hz: f32, secs: f32, square: bool) -> SoundData {
    let mut phase = 0.0f32;
    SoundData::generate(22_050, secs, move |t| {
        let k = t / secs;
        phase += (hz + (end_hz - hz) * k) / 22_050.0;
        let wave = if square {
            if phase.fract() < 0.5 { 1.0 } else { -1.0 }
        } else {
            (phase * std::f32::consts::TAU).sin()
        };
        wave * (1.0 - k) * 0.18
    })
}

// ---------------------------------------------------------------------------
// Game
// ---------------------------------------------------------------------------

impl Platformer {
    fn new(ctx: &mut Context) -> Result<Self> {
        let g = &mut ctx.graphics;
        g.set_default_filter(FilterMode::Nearest);
        let canvas = g.new_canvas(VIEW_W, VIEW_H);
        let tileset = g.new_image_from_data(&make_tileset());
        let player_sheet = g.new_image_from_data(&make_player_sheet());
        let tile_quads = Quad::grid(16.0, 16.0, 4, 1);

        let grid: Vec<Vec<u8>> = LEVEL.iter().map(|row| row.as_bytes().to_vec()).collect();
        assert!(
            grid.iter().all(|r| r.len() == grid[0].len()),
            "level rows must have equal length"
        );

        // Build the static tile layer once; it is drawn with one draw call.
        let mut tiles = g.new_sprite_batch(&tileset, 1024);
        let mut gems = Vec::new();
        let mut spawn = Vec2::ZERO;
        let mut flag = Vec2::ZERO;
        for (ty, row) in grid.iter().enumerate() {
            for (tx, &c) in row.iter().enumerate() {
                let pos = vec2(tx as f32 * TILE, ty as f32 * TILE);
                match c {
                    b'#' => {
                        let open_above = ty > 0 && grid[ty - 1][tx] != b'#';
                        let quad = if open_above {
                            tile_quads[T_GRASS]
                        } else {
                            tile_quads[T_STONE]
                        };
                        tiles.add_quad(quad, pos);
                    }
                    b'^' => {
                        tiles.add_quad(tile_quads[T_SPIKES], pos);
                    }
                    b'=' => {
                        tiles.add_quad(tile_quads[T_PLANK], pos);
                    }
                    b'o' => gems.push((pos + vec2(8.0, 8.0), false)),
                    b'P' => spawn = pos + vec2((TILE - PW) / 2.0, TILE - PH),
                    b'F' => flag = pos,
                    _ => {}
                }
            }
        }

        let audio = &ctx.audio;
        let sfx = Sfx {
            jump: audio.new_source_from_data(tone(300.0, 600.0, 0.12, true)),
            gem: audio.new_source_from_data(tone(988.0, 1319.0, 0.15, true)),
            die: audio.new_source_from_data(tone(400.0, 60.0, 0.4, true)),
            win: audio.new_source_from_data(tone(523.0, 1047.0, 0.8, false)),
        };

        Ok(Platformer {
            canvas,
            tiles,
            player_sheet,
            player_frames: Quad::grid(16.0, 16.0, 4, 1),
            grid,
            spawn,
            gems,
            flag,
            player: Player {
                pos: spawn,
                vel: Vec2::ZERO,
                on_ground: false,
                coyote: 0.0,
                jump_buffer: 0.0,
                facing_left: false,
                anim: 0.0,
                dead: 0.0,
            },
            camera: Vec2::ZERO,
            particles: Vec::new(),
            state: State::Playing,
            time: 0.0,
            run_time: 0.0,
            deaths: 0,
            sfx,
            hud_font: ctx.graphics.new_font(18.0),
            big_font: ctx.graphics.new_font(40.0),
        })
    }

    fn world_size(&self) -> Vec2 {
        vec2(
            self.grid[0].len() as f32 * TILE,
            self.grid.len() as f32 * TILE,
        )
    }

    fn tile(&self, tx: i32, ty: i32) -> u8 {
        if tx < 0 || ty < 0 {
            return b'#';
        }
        self.grid
            .get(ty as usize)
            .and_then(|r| r.get(tx as usize))
            .copied()
            .unwrap_or(b' ')
    }

    fn restart(&mut self) {
        self.state = State::Playing;
        self.gems.iter_mut().for_each(|g| g.1 = false);
        self.run_time = 0.0;
        self.deaths = 0;
        self.respawn();
    }

    fn respawn(&mut self) {
        let p = &mut self.player;
        p.pos = self.spawn;
        p.vel = Vec2::ZERO;
        p.dead = 0.0;
        p.facing_left = false;
    }

    fn burst(&mut self, at: Vec2, color: Color, n: usize) {
        for _ in 0..n {
            self.particles.push(Particle {
                pos: at,
                vel: vec2(random_range(-80.0, 80.0), random_range(-140.0, -20.0)),
                life: random_range(0.3, 0.7),
                color,
            });
        }
    }

    fn die(&mut self) {
        let center = self.player.pos + vec2(PW, PH) * 0.5;
        self.burst(center, Color::hex(0x4ecdc4), 24);
        self.player.dead = 0.7;
        self.deaths += 1;
        self.sfx.die.play_overlapping();
    }

    /// Moves along one axis and resolves collisions against solid tiles.
    fn move_axis(&mut self, delta: f32, horizontal: bool) {
        let p = &mut self.player;
        let old_bottom = p.pos.y + PH;
        if horizontal {
            p.pos.x += delta;
        } else {
            p.pos.y += delta;
        }
        let (x0, x1) = (
            (p.pos.x / TILE).floor() as i32,
            ((p.pos.x + PW - 0.01) / TILE).floor() as i32,
        );
        let (y0, y1) = (
            (p.pos.y / TILE).floor() as i32,
            ((p.pos.y + PH - 0.01) / TILE).floor() as i32,
        );
        let mut landed = false;
        for ty in y0..=y1 {
            for tx in x0..=x1 {
                let c = self.tile(tx, ty);
                let top = ty as f32 * TILE;
                let solid = c == b'#';
                // One-way planks only block when falling onto them from above.
                let plank = c == b'=' && !horizontal && delta > 0.0 && old_bottom <= top + 0.01;
                if !(solid || plank) {
                    continue;
                }
                let p = &mut self.player;
                if horizontal {
                    p.pos.x = if delta > 0.0 {
                        tx as f32 * TILE - PW
                    } else {
                        (tx + 1) as f32 * TILE
                    };
                    p.vel.x = 0.0;
                } else if delta > 0.0 {
                    p.pos.y = top - PH;
                    p.vel.y = 0.0;
                    landed = true;
                } else {
                    p.pos.y = (ty + 1) as f32 * TILE;
                    p.vel.y = 0.0;
                }
            }
        }
        if !horizontal {
            self.player.on_ground = landed;
        }
    }

    fn step_physics(&mut self, ctx: &Context, dt: f32) {
        let kb = &ctx.keyboard;
        let input = (kb.axis(Key::Left, Key::Right) + kb.axis(Key::A, Key::D)).clamp(-1.0, 1.0);
        let jump_held = kb.is_any_down(&[Key::Space, Key::Up, Key::W]);

        let p = &mut self.player;
        let accel = if p.on_ground { 18.0 } else { 9.0 };
        p.vel.x = approach(p.vel.x, input * RUN_SPEED, accel, dt);
        if input != 0.0 {
            p.facing_left = input < 0.0;
        }
        p.vel.y = (p.vel.y + GRAVITY * dt).min(MAX_FALL);
        // Releasing jump early cuts the jump short.
        if !jump_held && p.vel.y < -JUMP_SPEED * 0.4 {
            p.vel.y = -JUMP_SPEED * 0.4;
        }
        p.coyote = if p.on_ground { COYOTE } else { p.coyote - dt };
        p.jump_buffer -= dt;
        if p.jump_buffer > 0.0 && p.coyote > 0.0 {
            p.vel.y = -JUMP_SPEED;
            p.jump_buffer = 0.0;
            p.coyote = 0.0;
            p.on_ground = false;
            self.sfx.jump.stop();
            self.sfx.jump.play();
        }

        let v = self.player.vel;
        self.move_axis(v.x * dt, true);
        self.move_axis(v.y * dt, false);
    }

    fn update_playing(&mut self, ctx: &Context, dt: f32) {
        if self.player.dead > 0.0 {
            self.player.dead -= dt;
            if self.player.dead <= 0.0 {
                self.respawn();
            }
            return;
        }
        self.run_time += dt;
        let kb = &ctx.keyboard;
        if kb.was_pressed(Key::Space) || kb.was_pressed(Key::Up) || kb.was_pressed(Key::W) {
            self.player.jump_buffer = JUMP_BUFFER;
        }

        // Fixed substeps keep collisions solid even after a long frame.
        let steps = (dt / (1.0 / 240.0)).ceil().max(1.0);
        for _ in 0..steps as u32 {
            self.step_physics(ctx, dt / steps);
        }

        let p = &mut self.player;
        p.anim += dt * if p.vel.x.abs() > 10.0 { 10.0 } else { 0.0 };

        // Hazards and pickups.
        let (pmin, pmax) = (self.player.pos, self.player.pos + vec2(PW, PH));
        let center = (pmin + pmax) * 0.5;
        let mut hurt = pmin.y > self.world_size().y;
        for ty in (pmin.y / TILE) as i32..=(pmax.y / TILE) as i32 {
            for tx in (pmin.x / TILE) as i32..=(pmax.x / TILE) as i32 {
                if self.tile(tx, ty) == b'^' {
                    // Spike hitbox: lower half of the tile, slightly inset.
                    let (sx, sy) = (tx as f32 * TILE + 3.0, ty as f32 * TILE + 9.0);
                    if pmax.x > sx && pmin.x < sx + 10.0 && pmax.y > sy {
                        hurt = true;
                    }
                }
            }
        }
        if hurt {
            self.die();
            return;
        }
        let mut collected = Vec::new();
        for (pos, taken) in &mut self.gems {
            if !*taken && pos.distance(center) < 11.0 {
                *taken = true;
                collected.push(*pos);
            }
        }
        for pos in collected {
            self.burst(pos, Color::hex(0xffd166), 12);
            self.sfx.gem.play_overlapping();
        }
        let flag_hit = center.x > self.flag.x
            && center.x < self.flag.x + TILE
            && center.y > self.flag.y - TILE;
        if flag_hit {
            self.state = State::Won;
            self.sfx.win.play();
            self.burst(self.flag + vec2(8.0, 0.0), Color::hex(0xef476f), 40);
        }
    }

    fn draw_world(&self, g: &mut Graphics) {
        // Sky gradient as a vertex-colored mesh.
        let (w, h) = (VIEW_W as f32, VIEW_H as f32);
        let top = Color::hex(0x1b1035).to_rgba8();
        let bottom = Color::hex(0x5b3a70).to_rgba8();
        g.set_color(Color::WHITE);
        g.mesh(
            &[
                Vertex::new(0.0, 0.0, 0.0, 0.0, top),
                Vertex::new(w, 0.0, 0.0, 0.0, top),
                Vertex::new(w, h, 0.0, 0.0, bottom),
                Vertex::new(0.0, h, 0.0, 0.0, bottom),
            ],
            Some(&[0, 1, 2, 0, 2, 3]),
            None,
        );

        // Parallax hills: two layers scrolling slower than the camera.
        for (layer, (speed, color, base)) in [(0.2, 0x2e1f47, 110.0), (0.45, 0x3d2a5c, 135.0)]
            .into_iter()
            .enumerate()
        {
            g.set_color(Color::hex(color));
            let offset = -(self.camera.x * speed) % 80.0;
            for i in -1..6 {
                let x = offset + i as f32 * 80.0 + layer as f32 * 40.0;
                let peak = base - 30.0 - ((i * 37 + layer as i32 * 11).rem_euclid(5)) as f32 * 6.0;
                g.polygon(Fill, &[x - 50.0, h, x, peak, x + 50.0, h]);
            }
        }

        g.push();
        g.translate(-self.camera.x, -self.camera.y);

        g.set_color(Color::WHITE);
        g.draw(&self.tiles, 0.0, 0.0);

        // Gems bob and sparkle.
        for (i, (pos, taken)) in self.gems.iter().enumerate() {
            if *taken {
                continue;
            }
            let bob = (self.time * 3.0 + i as f32).sin() * 2.0;
            let (x, y) = (pos.x, pos.y + bob);
            g.set_color(Color::hex(0xffd166).with_alpha(0.25));
            g.circle(Fill, x, y, 6.0);
            g.set_color(Color::hex(0xffd166));
            g.polygon(Fill, &[x, y - 5.0, x + 4.0, y, x, y + 5.0, x - 4.0, y]);
            g.set_color(Color::WHITE);
            g.rectangle(Fill, x - 1.0, y - 3.0, 1.0, 2.0);
        }

        // Goal flag.
        let wave = (self.time * 6.0).sin() * 2.0;
        g.set_color(Color::hex(0xd8dee9));
        g.rectangle(Fill, self.flag.x + 3.0, self.flag.y - 8.0, 2.0, 24.0);
        g.set_color(Color::hex(0xef476f));
        g.polygon(
            Fill,
            &[
                self.flag.x + 5.0,
                self.flag.y - 8.0,
                self.flag.x + 17.0,
                self.flag.y - 3.0 + wave,
                self.flag.x + 5.0,
                self.flag.y + 2.0,
            ],
        );

        // Player.
        let p = &self.player;
        if p.dead <= 0.0 {
            let frame = if !p.on_ground {
                F_JUMP
            } else if p.vel.x.abs() > 10.0 {
                if (p.anim as i32) % 2 == 0 {
                    F_RUN1
                } else {
                    F_RUN2
                }
            } else {
                F_IDLE
            };
            // The 16x16 frame is centered on the 10x14 hitbox; positions are
            // rounded so the pixel art never shimmers.
            let x = (p.pos.x + PW / 2.0).round();
            let y = (p.pos.y + PH - 16.0 + 1.0).round();
            g.set_color(Color::WHITE);
            g.draw_quad_ex(
                &self.player_sheet,
                self.player_frames[frame],
                DrawParams::at(x, y).origin(8.0, 0.0).flip_x(p.facing_left),
            );
        }

        for part in &self.particles {
            g.set_color(part.color.with_alpha((part.life * 2.0).min(1.0)));
            g.rectangle(Fill, part.pos.x.round(), part.pos.y.round(), 2.0, 2.0);
        }
        g.pop();
    }
}

impl Game for Platformer {
    fn update(&mut self, ctx: &mut Context, dt: f32) {
        self.time += dt;
        if ctx.keyboard.was_pressed(Key::R)
            || (self.state == State::Won && ctx.keyboard.was_pressed(Key::Enter))
        {
            self.restart();
        }
        if self.state == State::Playing {
            self.update_playing(ctx, dt);
        }

        self.particles.retain_mut(|p| {
            p.vel.y += 400.0 * dt;
            p.pos += p.vel * dt;
            p.life -= dt;
            p.life > 0.0
        });

        // Smooth camera that follows the player, clamped to the level.
        let world = self.world_size();
        let target =
            self.player.pos + vec2(PW, PH) * 0.5 - vec2(VIEW_W as f32 / 2.0, VIEW_H as f32 * 0.55);
        let target = target.clamp(Vec2::ZERO, world - vec2(VIEW_W as f32, VIEW_H as f32));
        self.camera.x = approach(self.camera.x, target.x, 8.0, dt).round();
        self.camera.y = approach(self.camera.y, target.y, 5.0, dt).round();
    }

    fn draw(&mut self, ctx: &mut Context) {
        let g = &mut ctx.graphics;
        g.set_background_color(Color::hex(0x0d0a14));

        let canvas = self.canvas.clone();
        g.with_canvas(&canvas, |g| {
            g.clear(Color::BLACK);
            self.draw_world(g);
        });

        // Integer-scale the canvas into the window, letterboxed.
        let (sw, sh) = g.dimensions();
        let scale = (sw / VIEW_W as f32)
            .min(sh / VIEW_H as f32)
            .floor()
            .max(1.0);
        let ox = ((sw - VIEW_W as f32 * scale) / 2.0).floor();
        let oy = ((sh - VIEW_H as f32 * scale) / 2.0).floor();
        g.set_color(Color::WHITE);
        g.draw_ex(&self.canvas, DrawParams::at(ox, oy).scale(scale));

        // HUD at full resolution on top of the scaled canvas.
        let total = self.gems.len();
        let got = self.gems.iter().filter(|g| g.1).count();
        g.set_font(&self.hud_font);
        g.set_color(Color::rgba(0.0, 0.0, 0.0, 0.5));
        g.rounded_rectangle(Fill, ox + 10.0, oy + 10.0, 300.0, 34.0, 8.0);
        g.set_color(Color::hex(0xffd166));
        g.print(&format!("◆ {got}/{total}"), ox + 22.0, oy + 16.0);
        g.set_color(Color::WHITE);
        g.print(
            &format!("deaths {}   {:.1}s", self.deaths, self.run_time),
            ox + 110.0,
            oy + 16.0,
        );

        if self.state == State::Won {
            g.set_color(Color::rgba(0.0, 0.0, 0.0, 0.6));
            g.rectangle(Fill, 0.0, 0.0, sw, sh);
            g.set_font(&self.big_font);
            g.set_color(Color::hex(0xffd166));
            g.printf("You made it!", 0.0, sh / 2.0 - 70.0, sw, Align::Center);
            g.set_font(&self.hud_font);
            g.set_color(Color::WHITE);
            g.printf(
                &format!(
                    "{got} of {total} gems, {} deaths, {:.1} seconds\n\nPress Enter to play again",
                    self.deaths, self.run_time
                ),
                0.0,
                sh / 2.0,
                sw,
                Align::Center,
            );
        }
    }

    fn key_pressed(&mut self, ctx: &mut Context, key: Key, _repeat: bool) {
        if key == Key::Escape {
            ctx.quit();
        }
    }
}

fn main() -> love2drs::Result {
    love2drs::run(
        Conf::new("Mossy Caverns")
            .size(960, 540)
            .resizable(true)
            .min_size(320, 180),
        Platformer::new,
    )
}
