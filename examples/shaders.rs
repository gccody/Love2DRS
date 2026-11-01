//! Custom pixel shaders, LÖVE style: you only write an `effect` function in
//! WGSL. A plasma shader paints the background, and a scene rendered to a
//! canvas is post-processed with a selectable effect.
//!
//! Keys 1-4 pick the post effect, move the mouse to steer the ripple.
//! Run with `cargo run --example shaders`.

use love2drs::prelude::*;

const PLASMA: &str = r#"
fn effect(color: vec4<f32>, t: texture_2d<f32>, s: sampler, uv: vec2<f32>, screen: vec2<f32>) -> vec4<f32> {
    let time = uniforms.data[0].x;
    let p = screen / uniforms.data[0].z * 6.0;
    let v = sin(p.x + time)
          + sin((p.y + time) * 0.7)
          + sin((p.x + p.y + time * 1.3) * 0.5)
          + sin(length(p - vec2<f32>(3.0, 3.0)) * 1.5 - time * 2.0);
    let k = v * 3.14159;
    let rgb = vec3<f32>(0.5 + 0.5 * sin(k), 0.5 + 0.5 * sin(k + 2.094), 0.5 + 0.5 * sin(k + 4.188));
    return vec4<f32>(rgb * 0.35, 1.0) * color;
}
"#;

const RIPPLE: &str = r#"
fn effect(color: vec4<f32>, t: texture_2d<f32>, s: sampler, uv: vec2<f32>, screen: vec2<f32>) -> vec4<f32> {
    let time = uniforms.data[0].x;
    let center = uniforms.data[1].xy;
    let d = distance(uv, center);
    let dir = normalize(uv - center + vec2<f32>(0.00001, 0.0));
    let offset = dir * sin(d * 50.0 - time * 8.0) * 0.012 * exp(-d * 3.0);
    return textureSample(t, s, uv + offset) * color;
}
"#;

const CHROMATIC: &str = r#"
fn effect(color: vec4<f32>, t: texture_2d<f32>, s: sampler, uv: vec2<f32>, screen: vec2<f32>) -> vec4<f32> {
    let time = uniforms.data[0].x;
    let shift = (uv - vec2<f32>(0.5, 0.5)) * (0.012 + 0.006 * sin(time * 3.0));
    let r = textureSample(t, s, uv + shift).r;
    let g = textureSample(t, s, uv);
    let b = textureSample(t, s, uv - shift).b;
    return vec4<f32>(r, g.g, b, g.a) * color;
}
"#;

const CRT: &str = r#"
fn effect(color: vec4<f32>, t: texture_2d<f32>, s: sampler, uv: vec2<f32>, screen: vec2<f32>) -> vec4<f32> {
    // Barrel distortion, scanlines and a vignette.
    let c = uv * 2.0 - vec2<f32>(1.0, 1.0);
    let bent = c * (1.0 + dot(c, c) * 0.06);
    let suv = bent * 0.5 + vec2<f32>(0.5, 0.5);
    let tex = textureSample(t, s, clamp(suv, vec2<f32>(0.0), vec2<f32>(1.0)));
    let inside = step(0.0, suv.x) * step(suv.x, 1.0) * step(0.0, suv.y) * step(suv.y, 1.0);
    let scan = 0.8 + 0.2 * sin(screen.y * 3.14159);
    let vignette = 1.0 - dot(c, c) * 0.25;
    return vec4<f32>(tex.rgb * scan * vignette * inside, tex.a) * color;
}
"#;

struct Shaders {
    time: f32,
    plasma: Shader,
    effects: Vec<(&'static str, Option<Shader>)>,
    current: usize,
    scene: Canvas,
    font: Font,
}

impl Shaders {
    fn new(ctx: &mut Context) -> Result<Self> {
        let g = &mut ctx.graphics;
        let effects = vec![
            ("none", None),
            ("ripple", Some(g.new_shader(RIPPLE)?)),
            ("chromatic aberration", Some(g.new_shader(CHROMATIC)?)),
            ("CRT", Some(g.new_shader(CRT)?)),
        ];
        Ok(Shaders {
            time: 0.0,
            plasma: g.new_shader(PLASMA)?,
            effects,
            current: 1,
            scene: g.new_canvas(480, 360),
            font: g.new_font(24.0),
        })
    }
}

impl Game for Shaders {
    fn update(&mut self, ctx: &mut Context, dt: f32) {
        self.time += dt;
        let (w, h) = ctx.graphics.dimensions();
        let dpi = ctx.graphics.dpi_scale();
        self.plasma.send(0, [self.time, w * dpi, h * dpi, 0.0]);

        // Mouse position relative to the canvas, in 0..1 texture coordinates.
        let (cw, ch) = (self.scene.width() as f32, self.scene.height() as f32);
        let origin = vec2((w - cw) / 2.0, (h - ch) / 2.0);
        let m = (ctx.mouse.position() - origin) / vec2(cw, ch);
        for (_, shader) in &self.effects {
            if let Some(s) = shader {
                s.send(0, self.time);
                s.send(1, m);
            }
        }
    }

    fn draw(&mut self, ctx: &mut Context) {
        let t = self.time;
        let g = &mut ctx.graphics;
        let (w, h) = g.dimensions();

        // Background: one rectangle covering the screen, colored by the shader.
        g.set_shader(Some(&self.plasma));
        g.rectangle(Fill, 0.0, 0.0, w, h);
        g.set_shader(None);

        // Draw a little scene into the canvas.
        let font = self.font.clone();
        g.with_canvas(&self.scene, |g| {
            g.clear(Color::hex(0x1d1f27));
            for i in 0..8 {
                let a = t * 0.8 + i as f32 * std::f32::consts::TAU / 8.0;
                g.set_color(Color::hsv(i as f32 * 45.0 + t * 30.0, 0.65, 1.0));
                g.push();
                g.translate(240.0 + a.cos() * 110.0, 180.0 + a.sin() * 110.0);
                g.rotate(a * 2.0);
                g.rounded_rectangle(Fill, -22.0, -22.0, 44.0, 44.0, 10.0);
                g.pop();
            }
            g.set_font(&font);
            g.set_color(Color::WHITE);
            g.print_ex(
                "SHADERS",
                DrawParams::at(240.0, 180.0).centered().scale(1.5),
            );
        });

        // Post-process the canvas while drawing it.
        let (name, shader) = &self.effects[self.current];
        g.set_shader(shader.as_ref());
        g.draw_ex(&self.scene, DrawParams::at(w / 2.0, h / 2.0).centered());
        g.set_shader(None);

        g.set_font(&self.font);
        g.set_color(Color::WHITE);
        g.print(&format!("effect: {name}"), 20.0, 16.0);
        g.set_color(Color::rgba(1.0, 1.0, 1.0, 0.6));
        g.print("1-4 switch effect   mouse moves the ripple", 20.0, h - 44.0);
    }

    fn key_pressed(&mut self, ctx: &mut Context, key: Key, _repeat: bool) {
        match key {
            Key::Num1 => self.current = 0,
            Key::Num2 => self.current = 1,
            Key::Num3 => self.current = 2,
            Key::Num4 => self.current = 3,
            Key::Escape => ctx.quit(),
            _ => {}
        }
    }
}

fn main() -> love2drs::Result {
    love2drs::run(Conf::new("love2drs shaders").size(900, 600), Shaders::new)
}
