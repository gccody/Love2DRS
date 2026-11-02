# love2drs 0.1 API reference

Everything below is public API. `use love2drs::prelude::*;` brings in:

- core: `Game`, `Context`, `Conf`, `Error`, `Result`
- module types (for helper function signatures): `Graphics`, `Keyboard`, `Mouse`, `Gamepads`,
  `Touch`, and `Audio` (audio feature)
- graphics: `DrawMode` (+ `Fill`, `Line`), `Align`, `BlendMode`, `Canvas`, `Color`, `DrawParams`,
  `Drawable`, `FilterMode`, `Font`, `Image`, `ImageData`, `Quad`, `Shader`, `SpriteBatch`,
  `Vertex`, `WrapMode`
- input: `Key`, `MouseButton`, `Gamepad`, `GamepadId`, `GamepadButton`, `GamepadAxis`, `TouchId`,
  `TouchPoint`
- math: `Vec2`, `vec2`, `Transform`, `Rng`, `lerp`, `approach`, `noise1`, `noise2`, `random`,
  `random_int`, `random_range`, `random_normal`
- audio (audio feature): `SoundData`, `Source`

Not in the prelude (import explicitly, rarely needed): `love2drs::timer::Timer`,
`love2drs::window::Window`, `love2drs::graphics::{FrameStats, IntoUniform, UNIFORM_SLOTS}`,
`love2drs::math::{sin_cos, set_random_seed}`.

`Vec2`/`vec2` are re-exported from `glam` (all glam `Vec2` methods work: `length`,
`normalize_or_zero`, `dot`, `distance`, `lerp`, `perp`, `clamp_length_max`, operators...).

---

## Crate root

```rust
pub fn run<G: Game, F: FnOnce(&mut Context) -> Result<G>>(conf: Conf, load: F) -> Result<()>
pub type Result<T = (), E = Error> = std::result::Result<T, E>;
pub enum Error { Io { path: String, source: std::io::Error }, Image(String), Font(String),
                 Audio(String), Graphics(String), Window(String), Custom(String) }
impl Error { pub fn custom(msg: impl Display) -> Self }   // also: From<image::ImageError>
pub use wgpu; pub use winit;
```

`Error` implements `std::error::Error + Display`. To use `?` on foreign errors inside the load
closure, map them: `.map_err(Error::custom)?`.

## `Conf` (startup options; builder, all fields also `pub`)

```rust
Conf::new(title)            // 800x600, not resizable, windowed, vsync on, msaa 1, max_dt 0.25
  .size(w: u32, h: u32)     // logical pixels
  .resizable(bool)
  .fullscreen(bool)         // borderless fullscreen
  .vsync(bool)
  .msaa(samples: u32)       // 1 or 4 (other values round to these)
  .min_size(w: u32, h: u32)
  .max_dt(seconds: f32)     // clamp for dt passed to update
```

## `Game` trait (all methods optional; defaults do nothing)

```rust
fn update(&mut self, ctx: &mut Context, dt: f32)
fn draw(&mut self, ctx: &mut Context)
fn key_pressed(&mut self, ctx: &mut Context, key: Key, repeat: bool)
fn key_released(&mut self, ctx: &mut Context, key: Key)
fn text_input(&mut self, ctx: &mut Context, text: &str)        // typed text, layout/IME aware
fn mouse_pressed(&mut self, ctx: &mut Context, x: f32, y: f32, button: MouseButton)
fn mouse_released(&mut self, ctx: &mut Context, x: f32, y: f32, button: MouseButton)
fn mouse_cancelled(&mut self, ctx: &mut Context, x: f32, y: f32, button: MouseButton) // mobile: the OS took the finger acting as the mouse; not a click, no mouse_released follows
fn mouse_moved(&mut self, ctx: &mut Context, x: f32, y: f32, dx: f32, dy: f32)
fn wheel_moved(&mut self, ctx: &mut Context, x: f32, y: f32)   // y > 0 = away from user
fn touch_pressed(&mut self, ctx: &mut Context, id: TouchId, x: f32, y: f32)
fn touch_moved(&mut self, ctx: &mut Context, id: TouchId, x: f32, y: f32, dx: f32, dy: f32)
fn touch_released(&mut self, ctx: &mut Context, id: TouchId, x: f32, y: f32) // the finger was really lifted
fn touch_cancelled(&mut self, ctx: &mut Context, id: TouchId, x: f32, y: f32) // the OS took it, or focus was lost: don't act on it
fn gamepad_added(&mut self, ctx: &mut Context, id: GamepadId)  // also for pads present at startup
fn gamepad_removed(&mut self, ctx: &mut Context, id: GamepadId)
fn gamepad_pressed(&mut self, ctx: &mut Context, id: GamepadId, button: GamepadButton)
fn gamepad_released(&mut self, ctx: &mut Context, id: GamepadId, button: GamepadButton)
fn gamepad_axis(&mut self, ctx: &mut Context, id: GamepadId, axis: GamepadAxis, value: f32)
fn resize(&mut self, ctx: &mut Context, width: f32, height: f32) // logical pixels
fn focus(&mut self, ctx: &mut Context, focused: bool)
fn file_dropped(&mut self, ctx: &mut Context, path: std::path::PathBuf)
fn suspended(&mut self, ctx: &mut Context)    // Android/iOS: went to the background, so save now
fn resumed(&mut self, ctx: &mut Context)      // Android/iOS: back from the background
fn low_memory(&mut self, ctx: &mut Context)   // the OS wants memory back (love.lowmemory)
fn quit(&mut self, ctx: &mut Context) -> bool                   // return true to cancel quitting
```

The trait requires `'static`, so your game struct must own its data (no borrowed references).

## `Context`

```rust
pub struct Context {
    pub graphics: Graphics, pub keyboard: Keyboard, pub mouse: Mouse,
    pub gamepad: Gamepads, pub touch: Touch,
    pub timer: Timer, pub window: Window,
    pub audio: Audio,          // only with the `audio` feature
}
impl Context { pub fn quit(&mut self) }
```

---

## Graphics (`ctx.graphics`)

### Screen / frame
```rust
fn width(&self) -> f32;  fn height(&self) -> f32;  fn dimensions(&self) -> (f32, f32)  // logical px
fn dpi_scale(&self) -> f32                    // physical px per logical px
fn set_vsync(&mut self, bool)
fn stats(&self) -> FrameStats                 // { draw_calls, passes, vertices, triangles } of last frame
fn capture_screenshot(&mut self, path: impl Into<PathBuf>)   // PNG, needs `png` feature
fn set_background_color(&mut self, impl Into<Color>);  fn background_color(&self) -> Color
fn clear(&mut self, impl Into<Color>)         // clears current target (screen or canvas)
```

### Resource creation
```rust
fn new_image(&self, path) -> Result<Image>
fn new_image_from_bytes(&self, &[u8]) -> Result<Image>           // e.g. include_bytes!
fn new_image_from_data(&self, &ImageData) -> Image
fn new_image_from_rgba(&self, w: u32, h: u32, rgba: &[u8]) -> Result<Image>
fn new_canvas(&self, w: u32, h: u32) -> Canvas                   // in pixels of the canvas
fn new_sprite_batch(&self, &Image, capacity: usize) -> SpriteBatch
fn new_font(&self, size: f32) -> Font                            // bundled Hack font
fn new_font_from_file(&self, path, size: f32) -> Result<Font>
fn new_font_from_bytes(&self, &[u8], size: f32) -> Result<Font>
fn new_shader(&mut self, effect_wgsl: &str) -> Result<Shader>
fn set_default_filter(&mut self, FilterMode); fn default_filter(&self) -> FilterMode
```
`set_default_filter` affects images/canvases created **afterwards** (default `Linear`).

### State
```rust
fn set_color(&mut self, impl Into<Color>);   fn color(&self) -> Color
fn set_line_width(&mut self, f32);           fn line_width(&self) -> f32     // default 1
fn set_point_size(&mut self, f32);           fn point_size(&self) -> f32     // default 1
fn set_font(&mut self, &Font);               fn font(&self) -> &Font
fn set_blend_mode(&mut self, BlendMode);     fn blend_mode(&self) -> BlendMode
fn set_shader(&mut self, Option<&Shader>);   fn shader(&self) -> Option<&Shader>
fn set_scissor(&mut self, x, y, w, h);       fn reset_scissor(&mut self)    // screen space, ignores transform
fn scissor(&self) -> Option<(f32, f32, f32, f32)>
fn set_canvas(&mut self, Option<&Canvas>);   fn canvas(&self) -> Option<&Canvas>
fn with_canvas(&mut self, &Canvas, f: impl FnOnce(&mut Graphics))  // restores previous target
```

### Transform stack
```rust
fn push(&mut self)          // saves transform only
fn push_all(&mut self)      // saves transform, color, font, blend, shader, scissor, canvas, line width, point size
fn pop(&mut self)
fn origin(&mut self)        // reset to identity
fn translate(&mut self, x, y); fn rotate(&mut self, radians); fn scale(&mut self, sx, sy); fn shear(&mut self, kx, ky)
fn apply_transform(&mut self, &Transform); fn replace_transform(&mut self, Transform); fn transform(&self) -> Transform
fn transform_point(&self, x, y) -> (f32, f32)           // local -> screen
fn inverse_transform_point(&self, x, y) -> (f32, f32)   // screen (e.g. mouse) -> local
```
Each call composes so that later calls apply first to drawn geometry (same as LÖVE):
`translate(cx, cy); rotate(a); scale(s, s)` draws scaled, then rotated, then moved.

### Shapes (`mode` is `Fill` or `Line`)
```rust
fn rectangle(&mut self, mode, x, y, w, h)               // (x, y) = top-left
fn rounded_rectangle(&mut self, mode, x, y, w, h, radius)
fn circle(&mut self, mode, cx, cy, radius)
fn ellipse(&mut self, mode, cx, cy, rx, ry)
fn arc(&mut self, mode, cx, cy, radius, a1, a2)         // Fill = pie slice, Line = open arc
fn polygon(&mut self, mode, points: &[f32])             // flat [x1,y1,x2,y2,...]; Fill must be convex
fn triangle(&mut self, mode, x1, y1, x2, y2, x3, y3)
fn line(&mut self, x1, y1, x2, y2)
fn polyline(&mut self, points: &[f32])
fn point(&mut self, x, y);  fn points(&mut self, points: &[f32])   // squares of point_size
fn mesh(&mut self, vertices: &[Vertex], indices: Option<&[u32]>, image: Option<&Image>)
```
`Vertex::new(x, y, u, v, color_rgba8: [u8; 4])`; fields `pos`, `uv`, `color`. Mesh colors are
per-vertex only (current color is NOT applied). Without indices every 3 vertices form a triangle.

### Images, canvases, batches
```rust
fn draw(&mut self, &impl Drawable, x, y)                         // Image, Canvas, SpriteBatch
fn draw_ex(&mut self, &impl Drawable, impl Into<DrawParams>)
fn draw_quad(&mut self, &Image, Quad, x, y)
fn draw_quad_ex(&mut self, &Image, Quad, impl Into<DrawParams>)
```

### Text
```rust
fn print(&mut self, text: &str, x, y)                            // top-left of first line; '\n' breaks lines
fn print_ex(&mut self, text: &str, impl Into<DrawParams>)        // anchor relative to whole text block
fn printf(&mut self, text: &str, x, y, limit: f32, Align)        // word wrap inside width `limit`
```

---

## `DrawParams` (builder; `Copy`)

```rust
DrawParams::new()  /  DrawParams::at(x, y)  /  (x, y).into()  /  vec2(x, y).into()
  .pos(x, y)
  .rotation(radians)
  .scale(s)  .scale_xy(sx, sy)  .flip_x(bool)
  .origin(ox, oy)          // pixels, the pivot that sits at (x, y)
  .anchor(fx, fy)          // fraction of size; overrides origin
  .centered()              // anchor(0.5, 0.5)
  .color(impl Into<Color>) // tint override for this draw
  .to_transform(w, h) -> Transform
// pub fields: x, y, rotation, sx, sy, ox, oy, anchor: Option<(f32, f32)>, color: Option<Color>
```
Vertical flip: `.scale_xy(sx, -sy)`.

## `Color` (`Copy`, fields `r g b a: f32`)

```rust
Color::rgb(r, g, b)  Color::rgba(r, g, b, a)  Color::rgba8(u8, u8, u8, u8)
Color::hex(0xRRGGBB)  Color::hsv(h_degrees, s, v)
c.with_alpha(a)  c.lerp(other, t)  c.to_rgba8() -> [u8; 4]
WHITE BLACK TRANSPARENT RED GREEN BLUE YELLOW CYAN MAGENTA ORANGE GRAY
From: (f32,f32,f32), (f32,f32,f32,f32), [f32;3], [f32;4];  Into<[f32;4]>
```

## `Image` (cheap clone)

```rust
img.width() -> u32  img.height() -> u32  img.dimensions() -> (u32, u32)
img.set_filter(FilterMode::{Linear, Nearest})  img.filter()
img.set_wrap(WrapMode::{Clamp, Repeat, MirroredRepeat})  img.wrap()
img.replace_pixels(rgba: &[u8])      // full image, w*h*4 bytes
img.replace_with(&ImageData)         // same size
img.write_region(x, y, w, h, rgba)   // panics if out of bounds / wrong length
```

## `ImageData` (CPU pixels, RGBA8)

```rust
ImageData::new(w, h)                          // transparent
ImageData::from_fn(w, h, |x, y| -> Color)
ImageData::from_rgba(w, h, Vec<u8>) -> Result
ImageData::load(path) -> Result  /  ImageData::decode(&[u8]) -> Result
d.save_png(path) -> Result                    // `png` feature
d.width() d.height() d.pixels() d.pixels_mut() d.into_pixels()
d.get_pixel(x, y) -> Color                    // panics out of bounds
d.set_pixel(x, y, Color)                      // ignores out of bounds
d.fill_rect(x: i32, y: i32, w: u32, h: u32, Color)
```

## `Quad` (`Copy`, fields `x y w h: f32`, pixel units)

```rust
Quad::new(x, y, w, h)
Quad::grid(cell_w, cell_h, cols, rows) -> Vec<Quad>   // row-major
```

## `Canvas` (cheap clone; drawable)

```rust
c.width() c.height() c.dimensions() c.image() -> &Image c.set_filter(..) c.set_wrap(..)
```
Created at a fixed pixel size. Canvases are not resized with the window. Recreate one in `resize`
if needed.

## `SpriteBatch` (drawable; retained on GPU)

```rust
b.add(impl Into<DrawParams>) -> usize               // whole image
b.add_quad(Quad, impl Into<DrawParams>) -> usize
b.set(index, params);  b.set_quad(index, Quad, params)
b.clear();  b.len();  b.is_empty();  b.image()
b.set_color(impl Into<Color>)                       // color for sprites added afterwards
```
Draw with `g.draw(&batch, x, y)` / `g.draw_ex(&batch, params)`. The whole batch is transformed
and tinted by the current color.

## `Font` (cheap clone)

```rust
Font::default_with_size(size)  Font::load(path, size) -> Result  Font::from_bytes(Vec<u8>, size) -> Result
f.with_size(size) -> Font      // same face, new size, shares parsed data
f.size()  f.ascent()  f.descent()  f.height()  f.line_advance()
f.set_line_height(multiplier)  f.line_height()
f.width(text) -> f32           // widest line
f.wrap(text, limit) -> Vec<String>
```
`Align::{Left, Center, Right}` for `printf`.

## `BlendMode`

`Alpha` (default), `PremultipliedAlpha`, `Add`, `Subtract`, `Multiply`, `Replace`, `Screen`,
`Lighten`, `Darken`.

## `Shader`

```rust
let s = g.new_shader(WGSL)?;     // WGSL must define `effect`, see below
s.send(slot: usize, value)       // slot < 16; value: f32, [f32;2], (f32,f32), [f32;3], [f32;4], Vec2, Color
g.set_shader(Some(&s)); ...draws...; g.set_shader(None);
```
WGSL contract (provided for you: `uniforms.data: array<vec4<f32>, 16>`, `globals.transform`,
`globals.tint`, `tex`, `samp`):
```wgsl
fn effect(color: vec4<f32>, t: texture_2d<f32>, s: sampler, uv: vec2<f32>, screen: vec2<f32>) -> vec4<f32> {
    return textureSample(t, s, uv) * color;   // the default effect
}
```
`color` = current color × vertex color; `uv` = texture coordinate (for shapes it points at a
white texel, so ignore `uv` when shading shapes); `screen` = fragment position in **physical**
pixels. Values sent mid-frame affect only later draws.

---

## Keyboard (`ctx.keyboard`)

```rust
is_down(Key) -> bool   is_any_down(&[Key]) -> bool
was_pressed(Key) -> bool   was_released(Key) -> bool   // since last frame; valid in update/callbacks
shift() ctrl() alt() -> bool
axis(negative: Key, positive: Key) -> f32              // -1, 0, 1
set_key_repeat(bool)  has_key_repeat()                 // repeat key_pressed events (default off)
set_text_input(bool)                                   // text_input callbacks (default on)
set_screen_keyboard(bool)                              // on-screen keyboard on Android and iOS, else no-op
has_screen_keyboard() -> bool                          // true on Android and iOS
```
`Key` variants: `A..Z`, `Num0..Num9`, `F1..F12`, `Up Down Left Right`, `Space Enter Escape Tab
Backspace Delete Insert Home End PageUp PageDown`, `LShift RShift LCtrl RCtrl LAlt RAlt LSuper
RSuper`, `Minus Equals LeftBracket RightBracket Backslash Semicolon Quote Comma Period Slash
Backtick`, `CapsLock NumLock ScrollLock PrintScreen Pause Menu`, `Kp0..Kp9 KpAdd KpSubtract
KpMultiply KpDivide KpEnter KpDecimal`, `Unknown`. `Key::ALL`, `key.name()` (e.g. `"lshift"`),
`Display`.

## Mouse (`ctx.mouse`)

```rust
x() y() position() -> Vec2   delta() -> Vec2   wheel() -> Vec2     // logical px; delta/wheel per frame
is_down(MouseButton)  was_pressed(MouseButton)  was_released(MouseButton)  was_cancelled(MouseButton)
is_inside()  set_visible(bool) is_visible()  set_grabbed(bool) is_grabbed()  set_position(x, y)
```
`MouseButton::{Left, Right, Middle, Back, Forward, Other(u16)}`.

## Gamepads (`ctx.gamepad`, feature `gamepad`)

Without the feature (or on a platform with no backend) everything reports no controllers.

```rust
count() -> usize   is_connected() -> bool                 // any controller plugged in
iter() -> impl Iterator<Item = &Gamepad>   ids() -> impl Iterator<Item = GamepadId>
get(GamepadId) -> Option<&Gamepad>   first() -> Option<&Gamepad>   // first = connected longest
// "any controller" shortcuts, good for single-player:
is_down(GamepadButton)  is_any_down(&[GamepadButton])  was_pressed(..)  was_released(..)
axis(GamepadAxis) -> f32  left_stick() right_stick() dpad() -> Vec2  // strongest across pads
set_vibration(id, left: f32, right: f32, seconds: f32) -> bool   // 0..1 motors; f32::INFINITY = until stopped
stop_vibration(id)
```
`Gamepad` (one controller): `id() name() is_down(b) is_any_down(&[b]) was_pressed(b)
was_released(b) axis(a) left_stick() right_stick() dpad() is_vibration_supported()`.

`GamepadButton` (Xbox names; `A` = bottom face button on every pad): `A B X Y Back Guide Start
LeftStick RightStick LeftShoulder RightShoulder LeftTrigger RightTrigger DPadUp DPadDown DPadLeft
DPadRight`. `GamepadAxis`: `LeftX LeftY RightX RightY` (-1..1, **y down**) and `LeftTrigger
RightTrigger` (0..1). Both have `ALL` and `name()` (LÖVE names, e.g. `"dpup"`, `"triggerleft"`).
`GamepadId::index()` gives a `usize`. Stick values have a dead zone applied already. Controller
input keeps arriving while the window is unfocused or minimized. Rumble: Windows and Linux, plus
PlayStation pads (USB or Bluetooth) on macOS; other pads on macOS can't (`false`).

## Touch (`ctx.touch`)

```rust
touches() -> &[TouchPoint]   count() -> usize            // fingers on screen, oldest first
get(TouchId) -> Option<&TouchPoint>   position(TouchId) -> Option<Vec2>
was_pressed(TouchId) -> bool   pressed() -> impl Iterator<Item = &TouchPoint>   // this frame
released() -> &[TouchPoint]                              // lifted this frame, last position
cancelled() -> &[TouchPoint]                             // taken by the OS this frame, last position
```
`TouchPoint { id: TouchId, position: Vec2, start: Vec2, delta: Vec2, pressure: f32 }` (logical
px; `delta` since last frame; `pressure` 0..1, 1 when unsupported). `TouchId::index()` gives a
`u64`. A tap is a released point whose `position` is close to `start`. Touchscreens work on
Windows and Linux; the first finger also produces emulated mouse events.

## Timer (`ctx.timer`)

```rust
delta() -> f32   fps() -> f32   average_delta() -> f32   time() -> f64   frame_count() -> u64
```

## Window (`ctx.window`)

```rust
title() set_title(impl Into<String>)  size() -> (f32, f32)  set_size(w: u32, h: u32)
scale_factor() -> f64  set_fullscreen(bool) is_fullscreen()  set_resizable(bool)
has_focus()  minimize()  is_minimized()  maximize()  winit() -> &winit::window::Window
safe_area() -> (f32, f32, f32, f32)   // x, y, w, h clear of notches, system bars and the on-screen keyboard (love.window.getSafeArea)
```
While minimized the game keeps running, capped at 30 fps.

## Audio (`ctx.audio`, feature `audio`)

```rust
ctx.audio.new_source(path) -> Result<Source>              // wav/ogg/mp3/flac, fully decoded
ctx.audio.new_source_from_bytes(&[u8]) -> Result<Source>
ctx.audio.new_source_from_data(SoundData) -> Source
ctx.audio.set_volume(f32)  volume()                       // master volume
ctx.audio.is_available() -> bool                          // false while no output device works
ctx.audio.set_session(AudioSession)  session()            // iOS only; records the choice elsewhere
```
`AudioSession` (iOS): `Ambient` (default; mixes with the player's music, silent switch mutes),
`SoloAmbient` (stops their music, silent switch mutes), `Playback` (stops their music, ignores the
silent switch). Call `set_session` in `load`.

Playing sounds pause while the app is suspended, paused by Android, or interrupted (a call on iOS), and
carry on afterwards. The stream is closed meanwhile. If the device goes away it's reopened by itself.
Without a device, `play()` on a looping sound waits for one, and on anything else does nothing.

`Source` (clone = same voice; `duplicate()` = independent voice):
```rust
play() pause() stop()  play_overlapping()  duplicate() -> Source
is_playing() is_paused()  set_volume(f32) volume()  set_pitch(f32) pitch()   // pitch 2.0 = +1 octave
set_looping(bool) is_looping()  seek(seconds) tell() -> f32  duration() -> f32  data() -> &SoundData
```
`SoundData`:
```rust
SoundData::load(path) / decode(Vec<u8>) -> Result
SoundData::from_samples(sample_rate, channels, Vec<f32>)   // interleaved, -1..1
SoundData::generate(sample_rate, seconds, |t_seconds| -> sample)   // mono synth
d.channels() d.sample_rate() d.duration()
```

## Filesystem (`love2drs::filesystem`)

```text
read(path) -> Result<Vec<u8>>   // from disk, then from the app package on Android/iOS
save_dir() -> PathBuf           // where to save; the working directory on mobile
```

## Android (`love2drs::android_main!`)

`love2drs::android_main!(main);` next to `fn main` makes it the entry point on Android. It expands to
nothing on other platforms. `main` may return `()` or a `Result`.

## Math (`love2drs::math`)

```rust
Transform { a, b, c, d, tx, ty }   // maps (x,y) -> (a*x + c*y + tx, b*x + d*y + ty)
Transform::IDENTITY  translation(x, y)  rotation(r)  scaling(sx, sy)  from_params(x, y, r, sx, sy, ox, oy)
t.mul(&o)  t.apply(x, y)  t.apply_vec(v)  t.translate/rotate/scale/shear(..)  t.inverse() -> Option  t.approx_scale()
Rng::new(seed)  rng.random() /*[0,1)*/  rng.range(a, b)  rng.int(a, b) /*inclusive*/  rng.normal(mean, sd)  rng.next_u64()
random()  random_range(a, b)  random_int(a, b)  random_normal(mean, sd)  set_random_seed(u64)   // global, per thread
lerp(a, b, t)  approach(current, target, rate, dt)   // frame-rate independent smoothing
noise1(x)  noise2(x, y)   // gradient noise, roughly -1..1
sin_cos(x) -> (sin, cos)  // fast
```
