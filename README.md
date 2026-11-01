<img src="https://raw.githubusercontent.com/gccody/Love2DRS/master/assets/icon.svg" width="64" height="64" alt="">

# love2drs

A [LÖVE](https://love2d.org)-inspired 2D game framework for Rust. It has the same small API (`update` / `draw` callbacks, immediate-mode drawing, `push`/`pop` transforms), and it runs on a batched [wgpu](https://wgpu.rs) renderer that handles hundreds of thousands of sprites.

```rust
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
        g.print("Hello from love2drs!", 20.0, 20.0);
    }
}

fn main() -> love2drs::Result {
    love2drs::run(Conf::new("Hello"), |_ctx| Ok(Hello { pos: vec2(400.0, 300.0) }))
}
```

| | |
|---|---|
## Installation

```toml
[dependencies]
love2drs = { git = "https://github.com/gccody/Love2DRS" }

# Recommended: compile dependencies (wgpu, image decoding, audio) with optimizations
# even in debug builds. Without this, `cargo run` builds of your game can be very slow.
[profile.dev.package."*"]
opt-level = 2
```

Cargo only applies `[profile.*]` settings from your own `Cargo.toml`, not from a
dependency's, so this has to go in your game's manifest. Rust 1.88 or newer is required.

love2drs runs on Windows, macOS and Linux (X11 and Wayland). On Linux, building needs a few
system headers: `sudo apt install pkg-config libudev-dev libasound2-dev` on Debian/Ubuntu, or
`sudo dnf install pkgconf systemd-devel alsa-lib-devel` on Fedora. `libudev-dev` is for the
`gamepad` feature and `libasound2-dev` for `audio`; drop either feature to skip its headers.

| ![showcase](https://raw.githubusercontent.com/gccody/Love2DRS/master/screenshots/showcase.png) | ![starfall](https://raw.githubusercontent.com/gccody/Love2DRS/master/screenshots/starfall.png) |
| ![platformer](https://raw.githubusercontent.com/gccody/Love2DRS/master/screenshots/platformer.png) | ![shaders](https://raw.githubusercontent.com/gccody/Love2DRS/master/screenshots/shaders.png) |

## Examples

```sh
cargo run --example hello                     # the snippet above
cargo run --example showcase                  # tour of the drawing API
cargo run --release --example starfall        # arcade game: catch stars, dodge meteors
cargo run --release --example platformer      # pixel-art platformer (canvas + sprite batch)
cargo run --example shaders                   # custom WGSL pixel shaders / post effects
cargo run --example input                     # gamepad / touch / event tester
cargo run --release --example spritemark -- 100000   # renderer stress test
```

Every example generates its art and sound in code, so none of them need asset files.

## What's in the box

| LÖVE | love2drs |
|---|---|
| `love.load` | the closure passed to `love2drs::run` |
| `love.update(dt)` / `love.draw()` | `Game::update` / `Game::draw` |
| `love.keypressed`, `mousepressed`, `textinput`, `wheelmoved`, `resize`, `focus`, `quit`, ... | `Game::key_pressed`, `mouse_pressed`, `text_input`, `wheel_moved`, `resize`, `focus`, `quit`, ... |
| `love.gamepadpressed`, `gamepadaxis`, `joystickadded`, `touchpressed`, `touchmoved`, ... | `Game::gamepad_pressed`, `gamepad_axis`, `gamepad_added`, `touch_pressed`, `touch_moved`, ... |
| `love.graphics` | `ctx.graphics`: shapes, images, quads, text, canvases, sprite batches, meshes, shaders, blend modes, scissor, transform stack, screenshots |
| `love.keyboard` / `love.mouse` | `ctx.keyboard` / `ctx.mouse` (`is_down`, `was_pressed`, `axis`, position, wheel, grab, ...) |
| `love.joystick` | `ctx.gamepad`: hot-plugged controllers in an Xbox-style layout (buttons, sticks, triggers, rumble) via [gilrs](https://crates.io/crates/gilrs) |
| `love.touch` | `ctx.touch`: every finger on a touchscreen (position, start, delta, pressure) |
| `love.audio` | `ctx.audio` (wav/ogg/mp3/flac, volume, pitch, looping, seek, overlapping SFX, synthesized sounds) |
| `love.timer` | `ctx.timer` (`delta`, `fps`, `time`) |
| `love.window` | `ctx.window` (title, size, fullscreen, ...) |
| `love.math` | `love2drs::math` (`random*`, seedable `Rng`, `noise2`, `Transform`, `Vec2`) |
| `conf.lua` | `Conf::new("title").size(1280, 720).msaa(4).vsync(false)` |

Some differences from LÖVE:

- **A minimized game keeps running at 30 fps.** Nothing is drawn to the screen, but `update`,
  input and audio carry on. Check `ctx.window.is_minimized()` to pause instead.

- **Optional arguments become builders.** `love.graphics.draw(img, x, y, r, sx, sy, ox, oy)` is
  `g.draw_ex(&img, DrawParams::at(x, y).rotation(r).scale(2.0).centered())`. The plain
  `g.draw(&img, x, y)` covers the common case.
- **Shaders are WGSL.** You write only an `effect` function, as in LÖVE; love2drs provides the vertex
  stage, the bindings and 16 `vec4` uniform slots (`shader.send(slot, value)`). See `examples/shaders.rs`.
- **Coordinates are logical pixels.** Text is rasterized at the physical resolution and at the scale it
  is drawn, so it stays sharp on HiDPI screens and when scaled.
- **Keys are physical keys.** `Key::W` is the same key on QWERTY and AZERTY. Use `text_input` for typed text.
- **Colors are 0..1** (LÖVE 11 convention), and blending happens in gamma space as in LÖVE.

## Performance

Draw calls are never sent to the GPU directly:

- Sprites, glyphs and shapes are recorded as 48-byte quad/triangle instances that the vertex shader
  expands, instead of four vertices plus six indices (104 bytes). Consecutive draws that share a
  texture, blend mode, shader and scissor are merged into a single GPU draw call. At the end of the
  frame everything is uploaded with one `write_buffer` per buffer.
- Drawing with the same texture as the previous draw costs one comparison and a `Vec` push; state
  changes are handled on a separate slow path.
- Shapes sample a white texel inside the glyph atlas, so rectangles, circles and text **batch together**.
- All text, in any font and size, shares one glyph atlas that is rasterized on demand.
- `SpriteBatch` keeps static geometry (tile maps) on the GPU between frames. It is only re-uploaded when
  it changes, and is drawn with one draw call that can still be transformed.
- Rotations use a fast polynomial `sin_cos`. The hot paths are `#[inline]` so they inline into your crate.

`spritemark` draws every sprite with an individual, rotated, tinted `draw_ex` call. Measured on the
development machine (release build, vsync off):

| sprites | fps | GPU draw calls |
|---:|---:|---:|
| 30,000 | ~1,100 | 2 |
| 100,000 | ~375 | 2 |
| 200,000 | ~190 | 2 |

At 100,000 sprites a frame is roughly 1.5 ms of recording the draws, 0.8 ms of uploading the
instances (48 bytes per sprite) and 0.25 ms of the example's own update loop.
For geometry that doesn't move, a `SpriteBatch` avoids that cost entirely.

## Cargo features

| feature | default | |
|---|---|---|
| `audio` | yes | sound via [rodio](https://crates.io/crates/rodio) (wav, ogg vorbis, mp3, flac) |
| `gamepad` | yes | controllers via [gilrs](https://crates.io/crates/gilrs); without it `ctx.gamepad` reports none |
| `png` | yes | PNG decoding and screenshots |
| `jpeg` | yes | JPEG decoding |

`cargo build --no-default-features` gives you a graphics-only build. Touch input needs no feature.

### Platform notes for input

- **Gamepads** work on Windows (Windows.Gaming.Input), macOS (IOKit) and Linux (evdev). Controller
  input keeps arriving while the window is unfocused or minimized, so a button let go in the
  background is never left stuck down. Check `ctx.window.has_focus()` if your game should ignore it.
  Linux users need read access to `/dev/input/event*`, which desktop distributions grant to the
  logged-in user.
  Rumble works on Windows and Linux. PlayStation controllers (DualShock 4, DualSense, DualSense
  Edge) rumble on Windows and macOS too, over USB or Bluetooth: love2drs reads and drives them
  directly over HID there, since the OS gamepad APIs can't rumble them. Other controllers can't
  rumble on macOS, so `set_vibration` returns `false` for them.
- **Touch** events come from touchscreens on Windows and Linux. macOS has no touchscreens: trackpad
  gestures arrive as mouse and wheel events, so `ctx.touch` stays empty there. The OS also sends
  emulated mouse events for the first finger, so a tap reaches both `touch_pressed` and `mouse_pressed`.

## Screenshot testing

Set `LOVE2DRS_SCREENSHOT=out.png` (and optionally `LOVE2DRS_SCREENSHOT_FRAME=60`) to make any
love2drs game save that frame as a PNG and exit. This is useful for CI smoke tests. All the screenshots
above were captured this way. In game code, call `ctx.graphics.capture_screenshot("shot.png")`.

## Agent skill

The repo includes an [Agent Skill](skills/love2drs/SKILL.md) that teaches coding agents the love2drs API,
its gotchas and a set of tested recipes. To install it in Claude Code:

```text
/plugin install love2drs --marketplace gccody/Love2DRS
```

On Claude Code versions before 2.1.275, run `/plugin marketplace add gccody/Love2DRS`, then
`/plugin install love2drs@love2drs`. You can also copy `skills/love2drs/` into your project's
`.claude/skills/` or into `~/.claude/skills/`.

## Project layout

```
src/
  app.rs            event loop, Context, Game trait, run()
  conf.rs           Conf (window/startup options)
  graphics/
    mod.rs          Graphics: the immediate-mode drawing API + batching
    renderer.rs     frame recording structures and the wgpu backend
    font.rs         fonts, text layout and the shared glyph atlas
    image.rs        Image, ImageData, Quad, filter/wrap modes
    spritebatch.rs  retained GPU sprite batches
    shader.rs       custom WGSL shaders and uniforms
    params.rs       DrawParams
    color.rs        Color
  input/            keyboard and mouse
  audio.rs          lock-free sources mixed by rodio
  math.rs           Transform, RNG, noise, fast sin_cos
  timer.rs, window.rs
examples/           runnable demos and games
skills/love2drs/    agent skill (SKILL.md + API reference + recipes)
```

## Roadmap ideas

Android and iOS packaging, streaming playback for long music tracks, stencil support, multiple render
targets, a retained `Text` object, a filesystem module that knows about the game's save directory, and
WebAssembly support.

## License

love2drs is dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option. The bundled default font,
[Hack](https://sourcefoundry.org/hack/), is under the MIT License and the Bitstream Vera License; see
`src/graphics/default_font/LICENSE.txt`.
