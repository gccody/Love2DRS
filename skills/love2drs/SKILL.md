---
name: love2drs
description: Build, extend and debug 2D games in Rust with the love2drs crate, a LÖVE (love2d)-inspired framework on wgpu with update/draw callbacks, immediate-mode drawing, sprites, text, canvases, sprite batches, WGSL shaders, keyboard/mouse input and audio. Use whenever a Cargo.toml depends on love2drs, code imports `love2drs::prelude::*`, or the user wants to make a 2D Rust game "like LÖVE/love2d" with this crate, including porting LÖVE Lua code to it.
---

# love2drs

love2drs is a Rust port of the *shape* of LÖVE's API: you implement a `Game` trait with
`update`/`draw` callbacks, and draw with an immediate-mode `ctx.graphics` that batches
everything onto the GPU automatically. This skill documents **love2drs 0.1**.

Read the reference files only when you need them:

- `references/api.md`: every public type and method, grouped by module. Check it before
  calling a method you have not seen used. Do not guess names from LÖVE or other engines.
- `references/recipes.md`: tested patterns such as pixel-art scaling, sprite animation, tile maps,
  cameras, mouse picking, fixed timestep, scenes, shaders, procedural art and sound, text
  input and CI screenshots.

If the project pins a different version, the source of truth is the crate itself. Look at
`~/.cargo/git/checkouts/love2drs-*/` (git dependency) or `~/.cargo/registry/src/*/love2drs-*/`
(crates.io), or run `cargo doc -p love2drs --open`.

## Project setup

```toml
[dependencies]
love2drs = { git = "https://github.com/gccody/Love2DRS" }

# Strongly recommended: without this, debug builds of wgpu/image/audio are very slow.
[profile.dev.package."*"]
opt-level = 2
```

Requires Rust 1.88+ (edition 2024 is fine). Cargo features (all default): `audio` (rodio:
wav/ogg/mp3/flac, and gives you `ctx.audio`), `png`, `jpeg`. Use
`default-features = false, features = ["png"]` for a build without audio.

## Minimal game

```rust
use love2drs::prelude::*;

struct MyGame {
    pos: Vec2,
    player: Image,
}

impl MyGame {
    // Runs once the window and GPU exist (love.load). Create GPU resources here, never before `run`.
    fn new(ctx: &mut Context) -> Result<Self> {
        Ok(MyGame {
            pos: vec2(400.0, 300.0),
            player: ctx.graphics.new_image("assets/player.png")?,
        })
    }
}

impl Game for MyGame {
    fn update(&mut self, ctx: &mut Context, dt: f32) {
        let kb = &ctx.keyboard;
        let dir = vec2(kb.axis(Key::Left, Key::Right), kb.axis(Key::Up, Key::Down));
        self.pos += dir.normalize_or_zero() * 250.0 * dt;
        if kb.was_pressed(Key::Escape) {
            ctx.quit();
        }
    }

    fn draw(&mut self, ctx: &mut Context) {
        let g = &mut ctx.graphics;
        g.set_background_color(Color::hex(0x1d1f27));
        g.draw_ex(&self.player, DrawParams::at(self.pos.x, self.pos.y).centered());
        g.print(&format!("{:.0} fps", ctx.timer.fps()), 10.0, 10.0);
    }
}

fn main() -> love2drs::Result {
    love2drs::run(Conf::new("My Game").size(1280, 720).resizable(true), MyGame::new)
}
```

`main` returns `love2drs::Result` (an alias for `Result<(), love2drs::Error>`). Use `?` freely in
the load closure. To return your own errors use `Error::custom(msg)`.

## Mental model

**The frame loop**, in order:
1. OS events are delivered to the event callbacks (`key_pressed`, `mouse_pressed`, `text_input`,
   `wheel_moved`, `touch_pressed`/`touch_moved`/`touch_released`/`touch_cancelled`, `resize`, `focus`,
   `file_dropped`, `quit`), then controller events (`gamepad_added`/`removed`,
   `gamepad_pressed`/`released`, `gamepad_axis`).
2. `update(ctx, dt)` runs. `dt` is in seconds and clamped to `Conf::max_dt` (0.25 by default).
3. Per-frame input edges are cleared: `was_pressed`, `was_released`, `mouse.delta()`, `mouse.wheel()`,
   touch deltas, `ctx.touch.released()` and `ctx.touch.cancelled()`.
4. The screen is cleared to the background color, the transform is reset, and `draw(ctx)` runs.
5. The frame is submitted and presented (vsync on by default). While the window is minimized
   the loop keeps running, capped at 30 fps (`ctx.window.is_minimized()` to pause).

**`Context`** has public fields: `ctx.graphics`, `ctx.keyboard`, `ctx.mouse`, `ctx.gamepad`,
`ctx.touch`, `ctx.timer`, `ctx.window` and `ctx.audio` (audio feature only). Use `ctx.quit()` to exit. `Game::quit` can veto
the exit by returning `true`.

**Drawing is stateful like LÖVE.** `set_color`, `set_font`, `set_line_width`, `set_blend_mode`,
`set_shader`, `set_scissor`, `set_canvas` and the transform (`push`/`translate`/`rotate`/`scale`/`pop`)
all affect every draw call after them until changed. `push()` saves only the transform.
`push_all()` saves all state. Every `push` needs a matching `pop`.

**Coordinates.** These are logical pixels with the origin at the top-left and y pointing down.
Angles are radians, and positive values rotate clockwise on screen. Colors are `f32` in `0..=1`
(`Color::rgb`, `Color::rgba`, `Color::hex(0xRRGGBB)`, `Color::rgba8`, `Color::hsv`), and tuples
and arrays convert via `impl Into<Color>`.

**Optional LÖVE arguments become `DrawParams`.** LÖVE's `love.graphics.draw(img, x, y, r, sx, sy, ox, oy)` becomes
`g.draw_ex(&img, DrawParams::at(x, y).rotation(r).scale_xy(sx, sy).origin(ox, oy))`. Use
`.centered()` or `.anchor(fx, fy)` for fractional origins, and `.color(c)` for a per-draw tint.
`(x, y)` tuples and `Vec2` also convert into `DrawParams`.

**Resources are cheap handles.** `Image`, `Canvas`, `Font`, `Shader`, `Source` and `SoundData` are
reference-counted. Clone them freely and store them in your game struct. Create them once in the
load closure (or lazily), not every frame.

**Batching is automatic.** Consecutive draws that share texture, blend mode, shader and scissor
become one GPU draw call. Shapes and text share a texture, so mixing them is free. Changing image,
blend mode, shader, scissor or canvas starts a new batch, so group draws by image when you draw
thousands. Check `g.stats().draw_calls`.

## Gotchas (read before writing code)

- **The prelude covers almost everything**, including `Graphics`, `Keyboard`, `Mouse`, `Gamepads`, `Touch`, `Audio`,
  `Rng`, `approach` and `noise2`, so helpers like `fn draw_hud(g: &mut Graphics)` need no
  extra imports. The few exceptions (`Timer`, `Window`, `FrameStats`, `set_random_seed`, ...)
  are listed at the top of `references/api.md`.
- **Edge-triggered input belongs in `update`.** `was_pressed` and `was_released` are cleared
  before `draw`, so they are always false there. Use them in `update`, or use the `key_pressed`
  callback. `is_down` works anywhere.
- **Keys are physical.** `Key::W` is the key in the W position on any layout. Digits are
  `Key::Num0..Num9`, keypad keys are `Key::Kp0..`, and modifiers are `LShift`/`RShift`/`LCtrl`/... (`kb.shift()`
  checks either). For typed characters, implement `text_input` instead of mapping keys.
  `key_pressed` auto-repeat is off until `ctx.keyboard.set_key_repeat(true)`.
- **Gamepads use an Xbox layout.** `GamepadButton::A` is the bottom face button on every
  controller. `ctx.gamepad.is_down(b)`, `left_stick()`, `dpad()` and `axis(a)` read *any*
  connected controller, which suits single-player games. For local multiplayer, keep each
  player's `GamepadId` (from `gamepad_added`) and use `ctx.gamepad.get(id)`. Stick `y` points
  down like the screen, so `pos += ctx.gamepad.left_stick() * speed * dt` just works. Combine
  with the keyboard: `(keys + ctx.gamepad.left_stick()).clamp_length_max(1.0)`. A small dead
  zone is already applied. Rumble with `ctx.gamepad.set_vibration(id, left, right, seconds)`.
- **Touch** is `ctx.touch.touches()` (each with `position`, `start`, `delta`, `pressure`) plus the
  `touch_*` callbacks. The OS also turns the first finger into emulated mouse events, so don't
  handle the same action in both `touch_pressed` and `mouse_pressed`. macOS has no touch events.
- **Borrowing `ctx`.** `let g = &mut ctx.graphics;` borrows only that field, so reading
  `ctx.timer`, `ctx.mouse` or `ctx.keyboard` while `g` is alive is fine. Calling a method on
  `ctx` itself (`ctx.quit()`) is not. Read values first, or end the borrow.
- **Canvases persist across frames and are not auto-cleared.** Call `g.clear(color)` as the
  first thing inside `with_canvas` unless you want accumulation. Do not leave a canvas set at the end of `draw`.
  Prefer `g.with_canvas(&canvas, |g| { ... })`. If the closure needs `&mut self`, clone the
  canvas handle first: `let c = self.canvas.clone(); g.with_canvas(&c, |g| self.draw_world(g));`.
- **Pixel art** needs `g.set_default_filter(FilterMode::Nearest)` *before* creating the images and
  canvases (or `image.set_filter(...)` afterwards). Render the world into a small canvas and draw it
  scaled by an integer factor. See the recipes.
- **Filled `polygon` must be convex.** Point lists are flat `&[x1, y1, x2, y2, ...]`.
- **`mesh` ignores `set_color`.** It uses the per-vertex RGBA8 colors (`Color::to_rgba8()`).
  Vertex positions are transformed by the current transform.
- **`SpriteBatch` is for static or rarely changing geometry** (tile maps). It is uploaded only when
  changed. Its sprites are tinted by the current color when the batch is drawn. For thousands of
  *moving* sprites, plain `draw_ex` calls with one shared image are already batched and fast.
- **Text**: `print` uses the current font. The default is the bundled Hack font at 14 px.
  `g.new_font(size)` gives the default face at another size. Load TTF/OTF files with
  `new_font_from_file` / `new_font_from_bytes`. Measure with `font.width(text)` /
  `font.line_advance()`. `printf` word-wraps and aligns within a width.
- **Shaders are WGSL** and you only write `fn effect(color, t, s, uv, screen) -> vec4<f32>`.
  Uniforms are 16 `vec4` slots read as `uniforms.data[i]` and set with `shader.send(i, value)`.
  `screen` is in *physical* pixels, so multiply logical sizes by `g.dpi_scale()`.
  `new_shader` returns `Err` with the compiler message on bad WGSL.
- **Audio**: a `Source` is one voice, and calling `play()` while it is already playing does
  nothing. Use `play_overlapping()` for sound effects that can overlap. Files are fully decoded
  into memory (no streaming), so very long music costs RAM. With no audio device the game still
  runs and sound is silent. Sound comes back by itself if a device goes away and returns.
- **No built-in** physics, ECS, scene graph, networking or WASM. Use plain Rust (`std::fs`, `include_bytes!`, your own structs) or other crates.
  `love2drs::run` can only be called once per process.
- **Assets**: relative paths resolve against the *current working directory*, which is the
  directory you run `cargo run` from. That is usually the package root, not `src/`. To ship a
  self-contained binary, embed assets with
  `include_bytes!` + `new_image_from_bytes` / `new_font_from_bytes` / `new_source_from_bytes`.
- **Escape hatches**: `love2drs::wgpu` and `love2drs::winit` are re-exported, and
  `ctx.window.winit()` gives the raw window.

## Porting from LÖVE (Lua)

| LÖVE | love2drs |
|---|---|
| `love.load` | the `load` closure passed to `love2drs::run` (returns your `Game`) |
| `love.update(dt)` / `love.draw()` | `Game::update(ctx, dt)` / `Game::draw(ctx)` |
| `love.keypressed(key)` | `Game::key_pressed(ctx, key, repeat)` |
| `love.mousepressed(x, y, b)` | `Game::mouse_pressed(ctx, x, y, button)` |
| `love.graphics.setColor(r, g, b, a)` | `g.set_color(Color::rgba(r, g, b, a))` or `g.set_color((r, g, b))` |
| `love.graphics.rectangle("fill", ...)` | `g.rectangle(Fill, ...)` (`Fill`/`Line` are in the prelude) |
| `love.graphics.newImage("a.png")` | `ctx.graphics.new_image("a.png")?` |
| `love.graphics.draw(img, x, y)` | `g.draw(&img, x, y)` |
| `love.graphics.draw(img, quad, x, y)` | `g.draw_quad(&img, quad, x, y)` |
| `love.graphics.newQuad(x, y, w, h, sw, sh)` | `Quad::new(x, y, w, h)` (pixels; no sheet size needed) |
| `love.graphics.print(s, x, y)` / `printf(s, x, y, w, "center")` | `g.print(s, x, y)` / `g.printf(s, x, y, w, Align::Center)` |
| `love.graphics.push()` / `push("all")` | `g.push()` / `g.push_all()` |
| `love.graphics.setCanvas(c)` | `g.set_canvas(Some(&c))` / `g.with_canvas(&c, \|g\| ...)` |
| `love.graphics.newShader(glsl)` | `g.new_shader(wgsl_effect)?` (WGSL, not GLSL) |
| `love.keyboard.isDown("a", "left")` | `ctx.keyboard.is_any_down(&[Key::A, Key::Left])` |
| `love.mouse.getPosition()` | `ctx.mouse.position()` (a `Vec2`) |
| `love.gamepadpressed(joystick, button)` | `Game::gamepad_pressed(ctx, id, button)` |
| `joystick:isGamepadDown("a")` | `ctx.gamepad.get(id)` then `pad.is_down(GamepadButton::A)`, or `ctx.gamepad.is_down(GamepadButton::A)` for any pad |
| `joystick:getGamepadAxis("leftx")` | `pad.axis(GamepadAxis::LeftX)` / `pad.left_stick()` |
| `joystick:setVibration(l, r, d)` | `ctx.gamepad.set_vibration(id, l, r, d)` |
| `love.touchpressed(id, x, y, ...)` | `Game::touch_pressed(ctx, id, x, y)` |
| `love.touch.getTouches()` | `ctx.touch.touches()` |
| `love.audio.newSource(path, "static")` | `ctx.audio.new_source(path)?` |
| `love.timer.getFPS()` | `ctx.timer.fps()` |
| `love.math.random(a, b)` | `random_int(a, b)` (inclusive). `random_range(a, b)` for floats |
| `love.event.quit()` | `ctx.quit()` |
| `conf.lua` | `Conf::new("title").size(w, h).resizable(true).vsync(true).msaa(4)` |

## Android and iOS

Put `love2drs::android_main!(main);` next to `fn main`, then use the `cargo-love2drs` tool:
`cargo love2drs android run` or `cargo love2drs ios run` (iOS needs macOS). Add `--example <name>`,
`--bin <name>` or `--release` as needed. App settings and the assets to package go under
`[package.metadata.love2drs]` in `Cargo.toml`; see recipe 14. For the stores,
`cargo love2drs android package` makes an `.aab` for Google Play and `cargo love2drs ios package` makes
an `.ipa` and `.dSYM` for the App Store; see recipe 15.

When a game should run on phones:

- `Conf::size` is ignored because the window fills the screen. Never hard-code the layout: use
  `ctx.graphics.width()`/`height()`, handle `resize`, and keep controls inside `ctx.window.safe_area()`.
- Load files with love2drs loaders or `love2drs::filesystem::read`. `std::fs::read` can't see packaged assets.
- Save in `Game::suspended`. The OS may end a backgrounded app without another callback.
- Android's back gesture is `Key::Escape`, and the first finger also acts as the left mouse button.
- The OS can take a finger away (a back gesture, a call, the notification shade, losing focus). That sends
  `touch_cancelled` and, for the first finger, `mouse_cancelled`, not `touch_released` or `mouse_released`.
  **If the game acts on release, forget the pending press in these callbacks** and don't act on it.
- Sound stops while the app is in the background or paused and carries on by itself afterwards. On iOS it mixes
  with the player's music by default, and `ctx.audio.set_session(AudioSession::Playback)` changes that.
- On Android, Vulkan is used if the device has a driver and OpenGL ES 3 otherwise.
- `ctx.gamepad` and rumble work as on desktop. Check `is_vibration_supported()` before relying on rumble.
- `ctx.keyboard.set_screen_keyboard(true)` opens the on-screen keyboard. Read typing from `text_input`, and
  `Key::Backspace` and `Key::Enter` from `key_pressed`. `safe_area()` shrinks while it's up, and the user can
  close it without the game finding out. Android only delivers text that maps to key presses, so emoji, other
  scripts and autocorrect may not arrive.
- Set `icon = "path/to/square.png"` under `[package.metadata.love2drs]` for the app icon.
- `orientation = "landscape"` also holds on Android 16 tablets because the tool marks apps as games
  (`app_category`, default `"game"`). Setting it to anything else, or `"none"`, lets those tablets ignore it.
- A GPU error no longer panics. One-off errors are logged, out-of-memory also calls `low_memory`, and errors that
  last 60 frames in a row or a lost GPU device end the game with an `Error::Graphics` from `love2drs::run`.

## Verifying your work

- Type-check quickly with `cargo check`. Run with `cargo run` (add `--release` for big scenes).
- **Headless-ish screenshot check**: run any love2drs game with
  `LOVE2DRS_SCREENSHOT=out.png` (and optionally `LOVE2DRS_SCREENSHOT_FRAME=30`). The game saves
  that frame as a PNG and exits. Open the PNG to confirm the game renders what you intended. This
  needs a GPU and a display session.
- Inside game code, `ctx.graphics.capture_screenshot("shot.png")` saves the current frame.
- `ctx.graphics.stats()` reports draw calls, passes, vertices and triangles for the previous frame.
  Use it to spot broken batching.
