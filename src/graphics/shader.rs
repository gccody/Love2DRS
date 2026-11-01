use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use super::Color;
use super::image::next_id;
use crate::math::Vec2;

/// Number of `vec4<f32>` uniform slots available to custom shaders.
pub const UNIFORM_SLOTS: usize = 16;

/// The WGSL shared by every pipeline. Custom shaders only provide `effect`.
pub(crate) const PRELUDE: &str = r#"
struct Globals {
    transform: mat4x4<f32>,
    tint: vec4<f32>,
};
struct Uniforms {
    data: array<vec4<f32>, 16>,
};
@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var<uniform> uniforms: Uniforms;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_main(@location(0) pos: vec2<f32>, @location(1) uv: vec2<f32>, @location(2) color: vec4<f32>) -> VsOut {
    var out: VsOut;
    out.position = globals.transform * vec4<f32>(pos, 0.0, 1.0);
    out.uv = uv;
    out.color = color * globals.tint;
    return out;
}

// Instanced quads: corners p0 (uv0), p1 (u1, v0), p2 (u0, v1) and p1 + p2 - p0.
// kind 1 is a triangle (p0, p1, p2); its second half collapses to a point.
@vertex
fn vs_quad(
    @builtin(vertex_index) vi: u32,
    @location(0) p0: vec2<f32>,
    @location(1) p1: vec2<f32>,
    @location(2) p2: vec2<f32>,
    @location(3) uv: vec4<f32>,
    @location(4) color: vec4<f32>,
    @location(5) kind: u32,
) -> VsOut {
    // Two triangles: (0,0) (1,0) (1,1) / (0,0) (1,1) (0,1).
    var cx = select(0.0, 1.0, vi == 1u || vi == 2u || vi == 4u);
    var cy = select(0.0, 1.0, vi == 2u || vi == 4u || vi == 5u);
    if kind != 0u {
        cx = select(0.0, 1.0, vi == 1u);
        cy = select(0.0, 1.0, vi == 2u);
    }
    let pos = p0 + cx * (p1 - p0) + cy * (p2 - p0);
    var out: VsOut;
    out.position = globals.transform * vec4<f32>(pos, 0.0, 1.0);
    out.uv = mix(uv.xy, uv.zw, vec2<f32>(cx, cy));
    out.color = color * globals.tint;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return effect(in.color, tex, samp, in.uv, in.position.xy);
}
"#;

pub(crate) const DEFAULT_EFFECT: &str = r#"
fn effect(color: vec4<f32>, t: texture_2d<f32>, s: sampler, uv: vec2<f32>, screen: vec2<f32>) -> vec4<f32> {
    return textureSample(t, s, uv) * color;
}
"#;

pub(crate) struct ShaderInner {
    pub id: u64,
    pub module: wgpu::ShaderModule,
    pub uniforms: Mutex<[[f32; 4]; UNIFORM_SLOTS]>,
    pub version: AtomicU64,
}

/// A custom pixel shader, the equivalent of a LÖVE `Shader`.
///
/// Write a WGSL function named `effect`; love2drs supplies the vertex stage
/// and bindings:
///
/// ```wgsl
/// fn effect(color: vec4<f32>, t: texture_2d<f32>, s: sampler,
///           uv: vec2<f32>, screen: vec2<f32>) -> vec4<f32> {
///     let time = uniforms.data[0].x;          // set with shader.send(0, time)
///     let c = textureSample(t, s, uv) * color;
///     return vec4<f32>(c.rgb * (0.5 + 0.5 * sin(time)), c.a);
/// }
/// ```
///
/// `screen` is the fragment position in physical pixels. Up to
/// [`UNIFORM_SLOTS`] `vec4` values can be sent with [`Shader::send`]; values
/// sent mid-frame only affect draws issued afterwards.
#[derive(Clone)]
pub struct Shader {
    pub(crate) inner: Arc<ShaderInner>,
}

impl std::fmt::Debug for Shader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shader")
            .field("id", &self.inner.id)
            .finish()
    }
}

impl Shader {
    pub(crate) fn from_module(module: wgpu::ShaderModule) -> Shader {
        Shader {
            inner: Arc::new(ShaderInner {
                id: next_id(),
                module,
                uniforms: Mutex::new([[0.0; 4]; UNIFORM_SLOTS]),
                version: AtomicU64::new(1),
            }),
        }
    }

    /// Sets uniform slot `slot` (`uniforms.data[slot]` in WGSL).
    ///
    /// Accepts `f32`, `[f32; 2..=4]`, `(f32, f32)`, [`Vec2`] and [`Color`].
    pub fn send(&self, slot: usize, value: impl IntoUniform) {
        assert!(
            slot < UNIFORM_SLOTS,
            "uniform slot {slot} out of range (max {})",
            UNIFORM_SLOTS - 1
        );
        let v = value.into_uniform();
        let mut u = self.inner.uniforms.lock().unwrap();
        if u[slot] != v {
            u[slot] = v;
            self.inner.version.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub(crate) fn id(&self) -> u64 {
        self.inner.id
    }
}

/// Values that can be sent to a shader uniform slot.
pub trait IntoUniform {
    fn into_uniform(self) -> [f32; 4];
}

impl IntoUniform for f32 {
    fn into_uniform(self) -> [f32; 4] {
        [self, 0.0, 0.0, 0.0]
    }
}

impl IntoUniform for [f32; 2] {
    fn into_uniform(self) -> [f32; 4] {
        [self[0], self[1], 0.0, 0.0]
    }
}

impl IntoUniform for (f32, f32) {
    fn into_uniform(self) -> [f32; 4] {
        [self.0, self.1, 0.0, 0.0]
    }
}

impl IntoUniform for [f32; 3] {
    fn into_uniform(self) -> [f32; 4] {
        [self[0], self[1], self[2], 0.0]
    }
}

impl IntoUniform for [f32; 4] {
    fn into_uniform(self) -> [f32; 4] {
        self
    }
}

impl IntoUniform for Vec2 {
    fn into_uniform(self) -> [f32; 4] {
        [self.x, self.y, 0.0, 0.0]
    }
}

impl IntoUniform for Color {
    fn into_uniform(self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a]
    }
}
