# love2drs recipes

Each recipe is a complete program that compiles against love2drs 0.1. Copy the parts you need.
Put one in `src/main.rs` (or `examples/name.rs`) to try it.

Contents:
1. Pixel art at a virtual resolution (canvas + integer scaling + mouse mapping)
2. Sprite sheet animation
3. Tile map with `SpriteBatch`
4. Camera that follows the player and maps the mouse to world space
5. Fixed timestep simulation
6. Scenes / game states
7. Shaders: animated background and canvas post-processing
8. Procedural textures and sound effects (no asset files)
9. Particles with additive blending
10. Text input field
11. Embedding assets in the binary
12. Handling window resize
13. CI smoke test via screenshot

---

## 1. Pixel art at a virtual resolution

Render the world into a small canvas, then draw it scaled by an integer factor, letterboxed.

```rust
use love2drs::prelude::*;

const VIEW_W: u32 = 320;
const VIEW_H: u32 = 180;

struct Pixel {
    view: Canvas,
    hero: Image,
    pos: Vec2,
    // Where the canvas lands on screen, recomputed each frame.
    offset: Vec2,
    scale: f32,
}

impl Pixel {
    fn new(ctx: &mut Context) -> Result<Self> {
        let g = &mut ctx.graphics;
        g.set_default_filter(FilterMode::Nearest); // BEFORE creating images and canvases
        let hero = g.new_image_from_data(&ImageData::from_fn(8, 8, |x, y| {
            if (1..7).contains(&x) && (1..7).contains(&y) { Color::hex(0xffd166) } else { Color::TRANSPARENT }
        }));
        Ok(Pixel { view: g.new_canvas(VIEW_W, VIEW_H), hero, pos: vec2(160.0, 90.0), offset: Vec2::ZERO, scale: 1.0 })
    }

    /// Screen (window) coordinates -> virtual canvas coordinates.
    fn to_view(&self, screen: Vec2) -> Vec2 {
        (screen - self.offset) / self.scale
    }
}

impl Game for Pixel {
    fn update(&mut self, ctx: &mut Context, dt: f32) {
        let kb = &ctx.keyboard;
        self.pos += vec2(kb.axis(Key::A, Key::D), kb.axis(Key::W, Key::S)) * 60.0 * dt;
        if ctx.mouse.is_down(MouseButton::Left) {
            self.pos = self.to_view(ctx.mouse.position());
        }
    }

    fn draw(&mut self, ctx: &mut Context) {
        let g = &mut ctx.graphics;
        let view = self.view.clone();
        g.with_canvas(&view, |g| {
            g.clear(Color::hex(0x1d1f27));
            g.set_color(Color::WHITE);
            // Round positions so sprites don't shimmer between pixels.
            g.draw_ex(&self.hero, DrawParams::at(self.pos.x.round(), self.pos.y.round()).centered());
        });

        let (sw, sh) = g.dimensions();
        self.scale = (sw / VIEW_W as f32).min(sh / VIEW_H as f32).floor().max(1.0);
        self.offset = vec2(
            ((sw - VIEW_W as f32 * self.scale) / 2.0).floor(),
            ((sh - VIEW_H as f32 * self.scale) / 2.0).floor(),
        );
        g.set_background_color(Color::BLACK);
        g.set_color(Color::WHITE);
        g.draw_ex(&self.view, DrawParams::at(self.offset.x, self.offset.y).scale(self.scale));
        // HUD drawn here, after the canvas, is at full window resolution.
    }
}

fn main() -> love2drs::Result {
    love2drs::run(Conf::new("pixel").size(1280, 720).resizable(true), Pixel::new)
}
```

## 2. Sprite sheet animation

```rust
use love2drs::prelude::*;

struct Anim {
    frames: Vec<Quad>,
    fps: f32,
    time: f32,
}

impl Anim {
    fn new(frames: Vec<Quad>, fps: f32) -> Self {
        Anim { frames, fps, time: 0.0 }
    }
    fn update(&mut self, dt: f32) {
        self.time += dt;
    }
    fn frame(&self) -> Quad {
        self.frames[(self.time * self.fps) as usize % self.frames.len()]
    }
}

struct Demo {
    sheet: Image,
    walk: Anim,
    facing_left: bool,
}

impl Game for Demo {
    fn update(&mut self, ctx: &mut Context, dt: f32) {
        let dir = ctx.keyboard.axis(Key::Left, Key::Right);
        if dir != 0.0 {
            self.facing_left = dir < 0.0;
            self.walk.update(dt);
        }
    }

    fn draw(&mut self, ctx: &mut Context) {
        let p = DrawParams::at(400.0, 300.0).scale(6.0).flip_x(self.facing_left).centered();
        ctx.graphics.draw_quad_ex(&self.sheet, self.walk.frame(), p);
    }
}

fn main() -> love2drs::Result {
    love2drs::run(Conf::new("anim"), |ctx| {
        ctx.graphics.set_default_filter(FilterMode::Nearest);
        // A real game would use: ctx.graphics.new_image("assets/walk.png")?
        let sheet = ctx.graphics.new_image_from_data(&ImageData::from_fn(64, 16, |x, y| {
            let f = (x / 16) as i32;
            let (lx, ly) = ((x % 16) as i32, y as i32);
            if (ly - 8).abs() < 6 && (lx - 4 - f * 2).abs() < 3 { Color::hsv(f as f32 * 60.0, 0.7, 1.0) } else { Color::TRANSPARENT }
        }));
        Ok(Demo { sheet, walk: Anim::new(Quad::grid(16.0, 16.0, 4, 1), 8.0), facing_left: false })
    })
}
```

## 3. Tile map with `SpriteBatch`

Build once, then draw with one call per frame. Rebuild (or `set_quad`) only when tiles change.

```rust
use love2drs::prelude::*;

const TILE: f32 = 16.0;
const MAP: &[&str] = &[
    "....................",
    "....................",
    "......###...........",
    "..................#.",
    "####################",
];

struct Tiles {
    batch: SpriteBatch,
}

impl Game for Tiles {
    fn draw(&mut self, ctx: &mut Context) {
        let g = &mut ctx.graphics;
        g.set_color(Color::WHITE);
        g.draw_ex(&self.batch, DrawParams::at(0.0, 0.0).scale(3.0));
    }
}

fn main() -> love2drs::Result {
    love2drs::run(Conf::new("tiles").size(960, 540), |ctx| {
        let g = &mut ctx.graphics;
        g.set_default_filter(FilterMode::Nearest);
        // A 2-tile tileset: grass, dirt.
        let tileset = g.new_image_from_data(&ImageData::from_fn(32, 16, |x, y| {
            match (x / 16, y) {
                (0, 0..=3) => Color::hex(0x6ab04c),
                (0, _) | (1, _) => Color::hex(0x8d5524).lerp(Color::BLACK, ((x + y) % 3) as f32 * 0.08),
                _ => Color::TRANSPARENT,
            }
        }));
        let quads = Quad::grid(TILE, TILE, 2, 1);
        let mut batch = g.new_sprite_batch(&tileset, 256);
        for (ty, row) in MAP.iter().enumerate() {
            for (tx, c) in row.bytes().enumerate() {
                if c == b'#' {
                    let above_open = ty == 0 || MAP[ty - 1].as_bytes()[tx] != b'#';
                    let q = if above_open { quads[0] } else { quads[1] };
                    batch.add_quad(q, (tx as f32 * TILE, ty as f32 * TILE));
                }
            }
        }
        Ok(Tiles { batch })
    })
}
```

## 4. Camera: follow the player, map the mouse into the world

Keep the camera as a `Transform` you build in `update`. Use it to draw, and invert it to pick.

```rust
use love2drs::prelude::*;

struct World {
    player: Vec2,
    cam: Vec2,
    zoom: f32,
    marker: Option<Vec2>,
}

impl World {
    fn camera(&self, screen_w: f32, screen_h: f32) -> Transform {
        // Applied right-to-left to world points: move world so `cam` is at origin, zoom, then center on screen.
        let mut t = Transform::translation(screen_w / 2.0, screen_h / 2.0);
        t.scale(self.zoom, self.zoom);
        t.translate(-self.cam.x, -self.cam.y);
        t
    }
}

impl Game for World {
    fn update(&mut self, ctx: &mut Context, dt: f32) {
        let kb = &ctx.keyboard;
        let dir = vec2(kb.axis(Key::A, Key::D), kb.axis(Key::W, Key::S)).normalize_or_zero();
        self.player += dir * 200.0 * dt;
        // Smooth follow, frame-rate independent.
        self.cam.x = approach(self.cam.x, self.player.x, 6.0, dt);
        self.cam.y = approach(self.cam.y, self.player.y, 6.0, dt);
        // Zoom with the wheel.
        self.zoom = (self.zoom * 1.1f32.powf(ctx.mouse.wheel().y)).clamp(0.25, 4.0);

        if ctx.mouse.was_pressed(MouseButton::Left) {
            let (w, h) = ctx.graphics.dimensions();
            if let Some(inv) = self.camera(w, h).inverse() {
                self.marker = Some(inv.apply_vec(ctx.mouse.position()));
            }
        }
    }

    fn draw(&mut self, ctx: &mut Context) {
        let g = &mut ctx.graphics;
        let (w, h) = g.dimensions();
        g.push();
        g.apply_transform(&self.camera(w, h));
        // --- world space ---
        g.set_color(Color::GRAY);
        for i in -20..=20 {
            let p = i as f32 * 64.0;
            g.line(p, -1280.0, p, 1280.0);
            g.line(-1280.0, p, 1280.0, p);
        }
        g.set_color(Color::hex(0x06d6a0));
        g.circle(Fill, self.player.x, self.player.y, 16.0);
        if let Some(m) = self.marker {
            g.set_color(Color::RED);
            g.circle(Line, m.x, m.y, 10.0);
        }
        g.pop();
        // --- screen space (HUD) ---
        g.set_color(Color::WHITE);
        g.print("WASD move, wheel zoom, click to place a marker", 10.0, 10.0);
    }
}

fn main() -> love2drs::Result {
    love2drs::run(Conf::new("camera").resizable(true), |_| {
        Ok(World { player: Vec2::ZERO, cam: Vec2::ZERO, zoom: 1.0, marker: None })
    })
}
```

Inside `draw`, after applying the camera, `g.inverse_transform_point(mx, my)` also converts the
mouse to world space.

## 5. Fixed timestep

`dt` varies with frame rate. For deterministic physics, step a fixed `STEP` and interpolate.

```rust
use love2drs::prelude::*;

const STEP: f32 = 1.0 / 120.0;

#[derive(Clone, Copy)]
struct Body {
    pos: Vec2,
    vel: Vec2,
}

struct Sim {
    acc: f32,
    prev: Body,
    curr: Body,
}

impl Sim {
    fn step(b: &mut Body) {
        b.vel.y += 900.0 * STEP;
        b.pos += b.vel * STEP;
        if b.pos.y > 550.0 {
            b.pos.y = 550.0;
            b.vel.y = -b.vel.y * 0.8;
        }
    }
}

impl Game for Sim {
    fn update(&mut self, _ctx: &mut Context, dt: f32) {
        self.acc += dt;
        while self.acc >= STEP {
            self.prev = self.curr;
            Self::step(&mut self.curr);
            self.acc -= STEP;
        }
    }

    fn draw(&mut self, ctx: &mut Context) {
        let alpha = self.acc / STEP;
        let p = self.prev.pos.lerp(self.curr.pos, alpha);
        ctx.graphics.circle(Fill, p.x, p.y, 20.0);
    }
}

fn main() -> love2drs::Result {
    let b = Body { pos: vec2(400.0, 100.0), vel: Vec2::ZERO };
    love2drs::run(Conf::new("fixed step"), move |_| Ok(Sim { acc: 0.0, prev: b, curr: b }))
}
```

## 6. Scenes / game states

A plain enum is usually enough. Transition by assigning a new state.

```rust
use love2drs::prelude::*;

enum Scene {
    Title,
    Playing { score: u32, time: f32 },
    GameOver { score: u32 },
}

struct App {
    scene: Scene,
    big: Font,
    small: Font,
}

impl Game for App {
    fn update(&mut self, ctx: &mut Context, dt: f32) {
        let kb = &ctx.keyboard;
        let next = match &mut self.scene {
            Scene::Title if kb.was_pressed(Key::Enter) => Some(Scene::Playing { score: 0, time: 0.0 }),
            Scene::Playing { score, time } => {
                *time += dt;
                if kb.was_pressed(Key::Space) {
                    *score += 1;
                }
                (*time > 10.0).then(|| Scene::GameOver { score: *score })
            }
            Scene::GameOver { .. } if kb.was_pressed(Key::Enter) => Some(Scene::Title),
            _ => None,
        };
        if let Some(s) = next {
            self.scene = s;
        }
    }

    fn draw(&mut self, ctx: &mut Context) {
        let g = &mut ctx.graphics;
        let (w, h) = g.dimensions();
        g.set_font(&self.big);
        match &self.scene {
            Scene::Title => g.printf("MASH SPACE\npress Enter", 0.0, h / 3.0, w, Align::Center),
            Scene::Playing { score, time } => {
                g.printf(&format!("{score}"), 0.0, h / 3.0, w, Align::Center);
                g.set_font(&self.small);
                g.print(&format!("{:.1}s left", 10.0 - time), 10.0, 10.0);
            }
            Scene::GameOver { score } => {
                g.printf(&format!("score {score}\nEnter to restart"), 0.0, h / 3.0, w, Align::Center)
            }
        }
    }

    fn key_pressed(&mut self, ctx: &mut Context, key: Key, _repeat: bool) {
        if key == Key::Escape {
            ctx.quit();
        }
    }
}

fn main() -> love2drs::Result {
    love2drs::run(Conf::new("scenes"), |ctx| {
        Ok(App { scene: Scene::Title, big: ctx.graphics.new_font(48.0), small: ctx.graphics.new_font(16.0) })
    })
}
```

For larger games, use a `Box<dyn SceneTrait>` stack whose methods mirror `Game` and return an
optional transition.

## 7. Shaders: animated background and post-processing

```rust
use love2drs::prelude::*;

// Shapes sample a white texel, so a background shader can ignore `uv` and use `screen`.
const BG: &str = r#"
fn effect(color: vec4<f32>, t: texture_2d<f32>, s: sampler, uv: vec2<f32>, screen: vec2<f32>) -> vec4<f32> {
    let time = uniforms.data[0].x;
    let res = uniforms.data[0].yz;               // physical resolution
    let p = screen / res;
    return vec4<f32>(0.5 + 0.5 * sin(time + p.x * 6.0), p.y, 0.6, 1.0) * color;
}
"#;

// Post effect applied while drawing a canvas: grayscale + vignette.
const POST: &str = r#"
fn effect(color: vec4<f32>, t: texture_2d<f32>, s: sampler, uv: vec2<f32>, screen: vec2<f32>) -> vec4<f32> {
    let c = textureSample(t, s, uv);
    let gray = dot(c.rgb, vec3<f32>(0.299, 0.587, 0.114));
    let amount = uniforms.data[0].x;
    let d = distance(uv, vec2<f32>(0.5, 0.5));
    return vec4<f32>(mix(c.rgb, vec3<f32>(gray), amount) * (1.0 - d * 0.8), c.a) * color;
}
"#;

struct Fx {
    bg: Shader,
    post: Shader,
    scene: Canvas,
    time: f32,
}

impl Game for Fx {
    fn update(&mut self, ctx: &mut Context, dt: f32) {
        self.time += dt;
        let (w, h) = ctx.graphics.dimensions();
        let dpi = ctx.graphics.dpi_scale();
        self.bg.send(0, [self.time, w * dpi, h * dpi, 0.0]);
        self.post.send(0, if ctx.keyboard.is_down(Key::Space) { 1.0 } else { 0.0 });
    }

    fn draw(&mut self, ctx: &mut Context) {
        let g = &mut ctx.graphics;
        let (w, h) = g.dimensions();
        let t = self.time;
        g.with_canvas(&self.scene, |g| {
            g.clear(Color::TRANSPARENT);
            g.set_color(Color::hex(0xef476f));
            g.circle(Fill, 200.0 + t.sin() * 100.0, 150.0, 60.0);
        });

        g.set_shader(Some(&self.bg));
        g.rectangle(Fill, 0.0, 0.0, w, h);
        g.set_shader(Some(&self.post));
        g.set_color(Color::WHITE);
        g.draw_ex(&self.scene, DrawParams::at(w / 2.0, h / 2.0).centered());
        g.set_shader(None);
        g.print("hold Space for grayscale", 10.0, 10.0);
    }
}

fn main() -> love2drs::Result {
    love2drs::run(Conf::new("shaders"), |ctx| {
        let g = &mut ctx.graphics;
        Ok(Fx { bg: g.new_shader(BG)?, post: g.new_shader(POST)?, scene: g.new_canvas(400, 300), time: 0.0 })
    })
}
```

## 8. Procedural textures and sound effects

```rust
use love2drs::prelude::*;

/// A short pitch-swept blip, handy for jump/coin/hit sounds.
fn blip(start_hz: f32, end_hz: f32, secs: f32) -> SoundData {
    let mut phase = 0.0f32;
    let mut last_t = 0.0f32;
    SoundData::generate(44_100, secs, move |t| {
        let k = t / secs;
        let hz = start_hz + (end_hz - start_hz) * k;
        phase += hz * (t - last_t);
        last_t = t;
        let square = if (phase * std::f32::consts::TAU).sin() >= 0.0 { 1.0 } else { -1.0 };
        square * (1.0 - k) * 0.2 // fade out, keep it quiet
    })
}

struct Gen {
    clouds: Image,
    coin: Source,
}

impl Game for Gen {
    fn draw(&mut self, ctx: &mut Context) {
        ctx.graphics.draw_ex(&self.clouds, DrawParams::at(0.0, 0.0).scale(4.0));
        ctx.graphics.print("press C for a coin sound", 10.0, 10.0);
    }

    fn key_pressed(&mut self, _ctx: &mut Context, key: Key, _repeat: bool) {
        if key == Key::C {
            self.coin.play_overlapping();
        }
    }
}

fn main() -> love2drs::Result {
    love2drs::run(Conf::new("procedural"), |ctx| {
        let mut rng = Rng::new(42);
        let data = ImageData::from_fn(200, 150, |x, y| {
            let n = noise2(x as f32 * 0.05, y as f32 * 0.05) * 0.5 + 0.5;
            let sparkle = if rng.random() < 0.002 { 0.3 } else { 0.0 };
            Color::rgb(0.3 + n * 0.5 + sparkle, 0.5 + n * 0.4, 0.9)
        });
        Ok(Gen {
            clouds: ctx.graphics.new_image_from_data(&data),
            coin: ctx.audio.new_source_from_data(blip(988.0, 1319.0, 0.15)),
        })
    })
}
```

## 9. Particles with additive blending

Individual `draw`/shape calls with the same texture and state batch into one GPU draw call, so
thousands of particles are fine. Keep the particles in a `Vec` and remove dead ones with `retain`.

```rust
use love2drs::prelude::*;

struct P {
    pos: Vec2,
    vel: Vec2,
    life: f32,
}

struct Sparks {
    ps: Vec<P>,
}

impl Game for Sparks {
    fn update(&mut self, ctx: &mut Context, dt: f32) {
        if ctx.mouse.is_down(MouseButton::Left) {
            for _ in 0..20 {
                let a = random_range(0.0, std::f32::consts::TAU);
                let v = vec2(a.cos(), a.sin()) * random_range(50.0, 300.0);
                self.ps.push(P { pos: ctx.mouse.position(), vel: v, life: 1.0 });
            }
        }
        for p in &mut self.ps {
            p.vel *= 1.0 - 2.0 * dt;
            p.pos += p.vel * dt;
            p.life -= dt;
        }
        self.ps.retain(|p| p.life > 0.0);
    }

    fn draw(&mut self, ctx: &mut Context) {
        let g = &mut ctx.graphics;
        g.set_blend_mode(BlendMode::Add);
        for p in &self.ps {
            g.set_color(Color::hsv(30.0 + p.life * 30.0, 0.8, 1.0).with_alpha(p.life));
            g.circle(Fill, p.pos.x, p.pos.y, 2.0 + p.life * 3.0);
        }
        g.set_blend_mode(BlendMode::Alpha);
        g.set_color(Color::WHITE);
        g.print(&format!("{} particles, {} draw calls", self.ps.len(), g.stats().draw_calls), 10.0, 10.0);
    }
}

fn main() -> love2drs::Result {
    love2drs::run(Conf::new("particles").vsync(true), |_| Ok(Sparks { ps: Vec::new() }))
}
```

## 10. Text input field

Use `text_input` for characters (layout/IME aware) and `key_pressed` for editing keys. On phones, also call
`ctx.keyboard.set_screen_keyboard(true)` when the field is tapped, `set_screen_keyboard(false)` when it's
done, and keep the field inside `ctx.window.safe_area()`, which leaves out the keyboard.

```rust
use love2drs::prelude::*;

struct Input {
    text: String,
    submitted: Vec<String>,
}

impl Game for Input {
    fn text_input(&mut self, _ctx: &mut Context, text: &str) {
        self.text.push_str(text);
    }

    fn key_pressed(&mut self, _ctx: &mut Context, key: Key, _repeat: bool) {
        match key {
            Key::Backspace => {
                self.text.pop();
            }
            Key::Enter | Key::KpEnter if !self.text.is_empty() => {
                self.submitted.push(std::mem::take(&mut self.text));
            }
            _ => {}
        }
    }

    fn draw(&mut self, ctx: &mut Context) {
        let blink = (ctx.timer.time() * 2.0) as i64 % 2 == 0;
        let g = &mut ctx.graphics;
        let font = g.font().clone();
        g.set_color(Color::hex(0x33364a));
        g.rectangle(Fill, 20.0, 20.0, 400.0, font.line_advance() + 12.0);
        g.set_color(Color::WHITE);
        g.print(&self.text, 26.0, 26.0);
        if blink {
            let x = 26.0 + font.width(&self.text);
            g.line(x, 26.0, x, 26.0 + font.height());
        }
        for (i, line) in self.submitted.iter().enumerate() {
            g.print(line, 26.0, 70.0 + i as f32 * font.line_advance());
        }
    }
}

fn main() -> love2drs::Result {
    love2drs::run(Conf::new("text input"), |ctx| {
        ctx.keyboard.set_key_repeat(true); // hold Backspace to delete repeatedly
        Ok(Input { text: String::new(), submitted: Vec::new() })
    })
}
```

## 11. Embedding assets in the binary

`include_bytes!` paths are relative to the *source file*, and the bytes are compiled into the
executable, so the game runs from any working directory.

```rust,ignore
let img = ctx.graphics.new_image_from_bytes(include_bytes!("../assets/player.png"))?;
let font = ctx.graphics.new_font_from_bytes(include_bytes!("../assets/font.ttf"), 20.0)?;
let music = ctx.audio.new_source_from_bytes(include_bytes!("../assets/music.ogg"))?;
music.set_looping(true);
music.set_volume(0.5);
music.play();
```

Loading from disk instead (`new_image("assets/player.png")`) resolves against the current
working directory. To make that robust, build paths from the executable location
(`std::env::current_exe()`) or from `env!("CARGO_MANIFEST_DIR")` during development.

## 12. Handling window resize

`ctx.graphics.dimensions()` is always current, so layout computed from it in `draw` adapts
automatically. Use the `resize` callback only for things that must be rebuilt, such as
full-window canvases.

```rust
use love2drs::prelude::*;

struct Resizable {
    full: Canvas,
}

impl Game for Resizable {
    fn resize(&mut self, ctx: &mut Context, w: f32, h: f32) {
        // Canvas sizes are in pixels; use physical size for crisp results on HiDPI.
        let dpi = ctx.graphics.dpi_scale();
        self.full = ctx.graphics.new_canvas((w * dpi) as u32, (h * dpi) as u32);
    }

    fn draw(&mut self, ctx: &mut Context) {
        let g = &mut ctx.graphics;
        let (w, h) = g.dimensions();
        let dpi = g.dpi_scale();
        g.with_canvas(&self.full, |g| {
            g.clear(Color::hex(0x1d1f27));
            g.push();
            g.scale(dpi, dpi); // draw in logical units into the physical-size canvas
            g.set_color(Color::hex(0xffd166));
            g.circle(Line, w / 2.0, h / 2.0, w.min(h) * 0.4);
            g.pop();
        });
        g.set_color(Color::WHITE);
        g.draw_ex(&self.full, DrawParams::new().scale(1.0 / dpi));
    }
}

fn main() -> love2drs::Result {
    love2drs::run(Conf::new("resize").resizable(true).min_size(320, 240), |ctx| {
        let (w, h) = ctx.graphics.dimensions();
        let dpi = ctx.graphics.dpi_scale();
        Ok(Resizable { full: ctx.graphics.new_canvas((w * dpi) as u32, (h * dpi) as u32) })
    })
}
```

## 13. CI smoke test via screenshot

Any love2drs program can render a given frame to PNG and exit:

```sh
LOVE2DRS_SCREENSHOT=target/shot.png LOVE2DRS_SCREENSHOT_FRAME=30 cargo run --release
```

On Windows PowerShell: `$env:LOVE2DRS_SCREENSHOT="target/shot.png"; cargo run --release`.
This requires a GPU (or a software adapter such as Mesa llvmpipe / WARP) and a display session.
On Linux CI, wrap the command in `xvfb-run`. Look at the PNG, or compare it against a
reference image, to verify rendering.

## 14. Running on Android and iOS

```toml
# Cargo.toml
[package.metadata.love2drs]
app_id = "com.example.mygame"
name = "My Game"
assets = ["assets"]        # packaged at the same path, so new_image("assets/x.png") still works
orientation = "landscape"  # landscape, portrait or any
icon = "assets/icon.png"   # square PNG, 1024x1024 is best
```

```rust
use love2drs::prelude::*;

const SAVE_FILE: &str = "save.txt"; // relative paths are in the save directory on mobile

struct MyGame {
    score: u32,
}

impl Game for MyGame {
    fn draw(&mut self, ctx: &mut Context) {
        // A phone's window has no fixed size, and part of it may be covered.
        let (x, y, w, _h) = ctx.window.safe_area();
        let g = &mut ctx.graphics;
        g.printf(&format!("Score: {}", self.score), x, y + 12.0, w, Align::Center);
    }

    fn touch_pressed(&mut self, _ctx: &mut Context, _id: TouchId, _x: f32, _y: f32) {
        self.score += 1;
    }

    // The OS may end the app after this without telling it.
    fn suspended(&mut self, _ctx: &mut Context) {
        let _ = std::fs::write(SAVE_FILE, self.score.to_string());
    }

    // Android's back gesture arrives as Escape.
    fn key_pressed(&mut self, ctx: &mut Context, key: Key, _repeat: bool) {
        if key == Key::Escape {
            ctx.quit();
        }
    }
}

fn main() -> love2drs::Result {
    love2drs::run(Conf::new("My Game"), |_ctx| {
        let score = std::fs::read_to_string(SAVE_FILE)
            .ok()
            .and_then(|text| text.trim().parse().ok())
            .unwrap_or(0);
        Ok(MyGame { score })
    })
}

love2drs::android_main!(main);
```

```sh
cargo install --git https://github.com/gccody/Love2DRS cargo-love2drs
cargo love2drs android run   # phone or running emulator
cargo love2drs ios run       # Simulator, on a Mac
```

If the game acts when a finger lifts, a touch the OS takes away (back gesture, incoming call, notification
shade, losing focus) mustn't count:

```rust
impl Game for MyGame {
    fn touch_pressed(&mut self, _ctx: &mut Context, id: TouchId, x: f32, y: f32) {
        self.pressed = Some((id, vec2(x, y)));
    }

    fn touch_released(&mut self, _ctx: &mut Context, id: TouchId, x: f32, y: f32) {
        if self.pressed.take().is_some_and(|(pressed, _)| pressed == id) {
            self.tap(vec2(x, y));
        }
    }

    // The player didn't let go. No touch_released follows.
    fn touch_cancelled(&mut self, _ctx: &mut Context, _id: TouchId, _x: f32, _y: f32) {
        self.pressed = None;
    }
}
```

Mouse-only games do the same with `mouse_cancelled`.

## 15. Packaging for the stores

```toml
[package.metadata.love2drs]
app_category = "game"     # the default; keeps a locked orientation working on Android 16 tablets
# On iOS, "game" also writes the keys that make the app eligible for Game Mode. "none" leaves it all out.
privacy_manifest = "ios/PrivacyInfo.xcprivacy"  # needed if the game collects data, see below
```

Google Play takes an `.aab`. `package` always builds optimized, for arm64 and x86_64 unless `--abi` says
otherwise. It needs [bundletool](https://github.com/google/bundletool/releases) and a JDK:

```sh
LOVE2DRS_KEYSTORE_PASS=... cargo love2drs android package \
    --bundletool bundletool-all.jar --keystore upload.jks --key-alias upload
```

The bundle lands in `target/love2drs/android/<name>-release.aab`, with native debug symbols for Google Play
inside it. Without `--keystore` it's signed with the debug key, which Google Play rejects. Set
`LOVE2DRS_KEY_PASS` if the key's password differs from the keystore's, and `BUNDLETOOL_JAR` instead of
`--bundletool` if that's easier.

The App Store takes an `.ipa`. On a Mac with a distribution certificate and an App Store provisioning profile:

```sh
cargo love2drs ios package --identity "Apple Distribution: Me (TEAMID)" --profile game.mobileprovision
```

This writes `<name>.ipa` and `<name>.app.dSYM` to `target/love2drs/ios/iphoneos/release/`. Upload the `.ipa` with
Transporter or `xcrun altool`, and keep the `.dSYM` for reading crash reports. Without `--identity` the result is
unsigned and needs signing before upload.

Some Android SDKs only come as Java in Maven packages, like Google Play Games. List them, and any `<meta-data>` they read,
and `cargo love2drs` has Gradle make the app's Java side. Everything else is packaged as usual, and games without
dependencies don't need Gradle. It's found on `PATH`, in `GRADLE`, or left behind by another project's wrapper. Call the
library through JNI, loading its classes with the activity's class loader, since `FindClass` on a thread you attached only
sees the system's.

```toml
[package.metadata.love2drs]
android_dependencies = ["com.google.android.gms:play-services-games-v2:22.1.0"]
android_meta_data = { "com.google.android.gms.games.APP_ID" = "1234567890" }
```

SDK calls that take an `Activity` need `love2drs::android_app().activity_as_ptr()`.
`ndk_context::android_context()` holds the `Application`, and passing that where an `Activity` is declared aborts the process.

The tool writes the `DT*` keys App Store Connect checks and a `PrivacyInfo.xcprivacy`. The one it makes declares no
tracking, no data collection, and the required-reason system APIs it finds in the executable, with the reason
for ordinary use inside an app. That's only a starting point: if the game collects data or has analytics, write
your own and set `privacy_manifest`, and keep it consistent with what you tell App Store Connect.
