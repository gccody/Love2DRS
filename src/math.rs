//! Math helpers: vectors, 2D transforms, random numbers and noise
//! (the `love.math` equivalent).

use std::cell::Cell;

pub use glam::{Vec2, vec2};

/// A 2D affine transform.
///
/// Maps `(x, y)` to `(a*x + c*y + tx, b*x + d*y + ty)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    /// X-axis x component (scale / rotation).
    pub a: f32,
    /// X-axis y component (rotation / shear).
    pub b: f32,
    /// Y-axis x component (rotation / shear).
    pub c: f32,
    /// Y-axis y component (scale / rotation).
    pub d: f32,
    /// Translation along x.
    pub tx: f32,
    /// Translation along y.
    pub ty: f32,
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Transform {
    /// The transform that leaves points unchanged.
    pub const IDENTITY: Transform = Transform {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        tx: 0.0,
        ty: 0.0,
    };

    /// A pure translation by `(x, y)`.
    #[inline]
    pub fn translation(x: f32, y: f32) -> Self {
        Transform {
            tx: x,
            ty: y,
            ..Self::IDENTITY
        }
    }

    /// A rotation by `angle` radians (clockwise on screen, since y points down).
    #[inline]
    pub fn rotation(angle: f32) -> Self {
        let (s, c) = sin_cos(angle);
        Transform {
            a: c,
            b: s,
            c: -s,
            d: c,
            tx: 0.0,
            ty: 0.0,
        }
    }

    /// A scale by `sx` horizontally and `sy` vertically.
    #[inline]
    pub fn scaling(sx: f32, sy: f32) -> Self {
        Transform {
            a: sx,
            d: sy,
            ..Self::IDENTITY
        }
    }

    /// Builds the transform LÖVE uses for `draw(x, y, r, sx, sy, ox, oy)`:
    /// translate, rotate, scale, then offset by the origin.
    #[inline]
    pub fn from_params(x: f32, y: f32, r: f32, sx: f32, sy: f32, ox: f32, oy: f32) -> Self {
        let (s, c) = if r == 0.0 { (0.0, 1.0) } else { sin_cos(r) };
        let a = c * sx;
        let b = s * sx;
        let cc = -s * sy;
        let d = c * sy;
        Transform {
            a,
            b,
            c: cc,
            d,
            tx: x - ox * a - oy * cc,
            ty: y - ox * b - oy * d,
        }
    }

    /// `self * other`: applies `other` first, then `self`.
    #[inline]
    pub fn mul(&self, o: &Transform) -> Transform {
        Transform {
            a: self.a * o.a + self.c * o.b,
            b: self.b * o.a + self.d * o.b,
            c: self.a * o.c + self.c * o.d,
            d: self.b * o.c + self.d * o.d,
            tx: self.a * o.tx + self.c * o.ty + self.tx,
            ty: self.b * o.tx + self.d * o.ty + self.ty,
        }
    }

    /// Transforms the point `(x, y)`.
    #[inline]
    pub fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        (
            self.a * x + self.c * y + self.tx,
            self.b * x + self.d * y + self.ty,
        )
    }

    /// Transforms the point `p`.
    #[inline]
    pub fn apply_vec(&self, p: Vec2) -> Vec2 {
        let (x, y) = self.apply(p.x, p.y);
        vec2(x, y)
    }

    /// Appends a translation (applied before the existing transform).
    #[inline]
    pub fn translate(&mut self, x: f32, y: f32) {
        *self = self.mul(&Transform::translation(x, y));
    }

    /// Appends a rotation in radians (applied before the existing transform).
    #[inline]
    pub fn rotate(&mut self, angle: f32) {
        *self = self.mul(&Transform::rotation(angle));
    }

    /// Appends a scale (applied before the existing transform).
    #[inline]
    pub fn scale(&mut self, sx: f32, sy: f32) {
        *self = self.mul(&Transform::scaling(sx, sy));
    }

    /// Appends a shear by factors `kx` and `ky`.
    pub fn shear(&mut self, kx: f32, ky: f32) {
        *self = self.mul(&Transform {
            a: 1.0,
            b: ky,
            c: kx,
            d: 1.0,
            tx: 0.0,
            ty: 0.0,
        });
    }

    /// The inverse transform, or `None` if it is degenerate (zero scale).
    pub fn inverse(&self) -> Option<Transform> {
        let det = self.a * self.d - self.b * self.c;
        if det.abs() < f32::EPSILON {
            return None;
        }
        let inv = 1.0 / det;
        let a = self.d * inv;
        let b = -self.b * inv;
        let c = -self.c * inv;
        let d = self.a * inv;
        Some(Transform {
            a,
            b,
            c,
            d,
            tx: -(a * self.tx + c * self.ty),
            ty: -(b * self.tx + d * self.ty),
        })
    }

    /// Approximate uniform scale factor of the transform (used to pick
    /// tessellation detail for curves).
    #[inline]
    pub fn approx_scale(&self) -> f32 {
        let sx = (self.a * self.a + self.b * self.b).sqrt();
        let sy = (self.c * self.c + self.d * self.d).sqrt();
        sx.max(sy)
    }

    #[inline]
    pub(crate) fn is_axis_aligned_unit(&self) -> bool {
        self.a == 1.0 && self.b == 0.0 && self.c == 0.0 && self.d == 1.0
    }

    /// Column-major 4x4 matrix suitable for a GPU uniform.
    pub(crate) fn to_mat4(self) -> [[f32; 4]; 4] {
        [
            [self.a, self.b, 0.0, 0.0],
            [self.c, self.d, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [self.tx, self.ty, 0.0, 1.0],
        ]
    }
}

/// Fast `(sin, cos)` accurate to ~1e-7, several times quicker than
/// `f32::sin_cos`. Used for every rotated draw.
#[inline]
pub fn sin_cos(x: f32) -> (f32, f32) {
    // Range-reduce in f64 so large accumulated angles stay accurate.
    // Adding 1.5 * 2^52 rounds to the nearest integer without a libm call
    // (`f64::round` is a function call on baseline x86-64), and the low
    // mantissa bits of the sum are that integer modulo 4: the quadrant.
    const ROUND: f64 = 6_755_399_441_055_744.0;
    let xd = x as f64;
    let shifted = xd * std::f64::consts::FRAC_2_PI + ROUND;
    let q = shifted - ROUND;
    let r = (xd - q * std::f64::consts::FRAC_PI_2) as f32;
    let r2 = r * r;
    // Cephes minimax polynomials on [-pi/4, pi/4].
    let s = r + r * r2 * (-1.666_665_5e-1 + r2 * (8.332_161e-3 + r2 * -1.951_529_6e-4));
    let c =
        1.0 - 0.5 * r2 + r2 * r2 * (4.166_664_6e-2 + r2 * (-1.388_731_6e-3 + r2 * 2.443_315_7e-5));
    match shifted.to_bits() & 3 {
        0 => (s, c),
        1 => (c, -s),
        2 => (-s, -c),
        _ => (-c, s),
    }
}

// ---------------------------------------------------------------------------
// Random numbers
// ---------------------------------------------------------------------------

thread_local! {
    static RNG: Cell<u64> = Cell::new(initial_seed());
}

fn initial_seed() -> u64 {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9E37_79B9_7F4A_7C15);
    t ^ 0xA076_1D64_78BD_642F
}

/// A small, fast, seedable random number generator (wyrand).
///
/// Use [`random`] and friends for the global generator, or create your own
/// `Rng` when you need reproducible sequences.
#[derive(Debug, Clone)]
pub struct Rng {
    state: u64,
}

impl Rng {
    /// A generator with the given seed. The same seed always gives the same sequence.
    pub fn new(seed: u64) -> Self {
        Rng { state: seed }
    }

    /// The next raw 64-bit value.
    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0xA076_1D64_78BD_642F);
        let t = (self.state as u128).wrapping_mul((self.state ^ 0xE703_7ED1_A0B4_28DB) as u128);
        ((t >> 64) ^ t) as u64
    }

    /// Uniform `f32` in `[0, 1)`.
    #[inline]
    pub fn random(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 * (1.0 / (1u64 << 24) as f32)
    }

    /// Uniform `f32` in `[min, max)`.
    #[inline]
    pub fn range(&mut self, min: f32, max: f32) -> f32 {
        min + (max - min) * self.random()
    }

    /// Uniform integer in `[min, max]` (inclusive, like `love.math.random(min, max)`).
    #[inline]
    pub fn int(&mut self, min: i32, max: i32) -> i32 {
        if max <= min {
            return min;
        }
        let span = (max as i64 - min as i64 + 1) as u64;
        (min as i64 + (self.next_u64() % span) as i64) as i32
    }

    /// Normally distributed value (Box-Muller).
    pub fn normal(&mut self, mean: f32, stddev: f32) -> f32 {
        let u1 = self.random().max(1e-7);
        let u2 = self.random();
        mean + stddev * (-2.0 * u1.ln()).sqrt() * (std::f32::consts::TAU * u2).cos()
    }
}

fn with_rng<T>(f: impl FnOnce(&mut Rng) -> T) -> T {
    RNG.with(|cell| {
        let mut rng = Rng { state: cell.get() };
        let out = f(&mut rng);
        cell.set(rng.state);
        out
    })
}

/// Uniform `f32` in `[0, 1)` from the global generator.
#[inline]
pub fn random() -> f32 {
    with_rng(|r| r.random())
}

/// Uniform `f32` in `[min, max)` from the global generator.
#[inline]
pub fn random_range(min: f32, max: f32) -> f32 {
    with_rng(|r| r.range(min, max))
}

/// Uniform integer in `[min, max]` (inclusive) from the global generator.
#[inline]
pub fn random_int(min: i32, max: i32) -> i32 {
    with_rng(|r| r.int(min, max))
}

/// Normally distributed random value from the global generator.
pub fn random_normal(mean: f32, stddev: f32) -> f32 {
    with_rng(|r| r.normal(mean, stddev))
}

/// Re-seeds the global generator (per thread) for reproducible runs.
pub fn set_random_seed(seed: u64) {
    RNG.with(|c| c.set(seed));
}

// ---------------------------------------------------------------------------
// Misc helpers
// ---------------------------------------------------------------------------

/// Linear interpolation between `a` and `b`.
#[inline]
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Frame-rate independent exponential smoothing towards `target`.
/// `rate` is roughly "fraction approached per second" (higher = snappier).
#[inline]
pub fn approach(current: f32, target: f32, rate: f32, dt: f32) -> f32 {
    lerp(current, target, 1.0 - (-rate * dt).exp())
}

/// 2D gradient noise in roughly `[-1, 1]` (like `love.math.noise` but signed).
pub fn noise2(x: f32, y: f32) -> f32 {
    fn hash(ix: i32, iy: i32) -> f32 {
        let mut h = (ix as u32).wrapping_mul(0x8DA6_B343) ^ (iy as u32).wrapping_mul(0xD816_3841);
        h = (h ^ (h >> 13)).wrapping_mul(0x5BD1_E995);
        h ^= h >> 15;
        (h as f32 / u32::MAX as f32) * std::f32::consts::TAU
    }
    fn grad(ix: i32, iy: i32, x: f32, y: f32) -> f32 {
        let a = hash(ix, iy);
        (x - ix as f32) * a.cos() + (y - iy as f32) * a.sin()
    }
    fn fade(t: f32) -> f32 {
        t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
    }
    let x0 = x.floor() as i32;
    let y0 = y.floor() as i32;
    let fx = fade(x - x0 as f32);
    let fy = fade(y - y0 as f32);
    let n0 = lerp(grad(x0, y0, x, y), grad(x0 + 1, y0, x, y), fx);
    let n1 = lerp(grad(x0, y0 + 1, x, y), grad(x0 + 1, y0 + 1, x, y), fx);
    (lerp(n0, n1, fy) * 1.414).clamp(-1.0, 1.0)
}

/// 1D gradient noise in roughly `[-1, 1]`.
pub fn noise1(x: f32) -> f32 {
    noise2(x, 0.5)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_inverse_roundtrip() {
        let t = Transform::from_params(10.0, 20.0, 0.7, 2.0, 3.0, 5.0, 6.0);
        let inv = t.inverse().unwrap();
        let (x, y) = t.apply(3.0, 4.0);
        let (bx, by) = inv.apply(x, y);
        assert!((bx - 3.0).abs() < 1e-4 && (by - 4.0).abs() < 1e-4);
    }

    #[test]
    fn from_params_matches_composition() {
        let p = Transform::from_params(10.0, 20.0, 0.5, 2.0, 3.0, 4.0, 5.0);
        let mut c = Transform::translation(10.0, 20.0);
        c.rotate(0.5);
        c.scale(2.0, 3.0);
        c.translate(-4.0, -5.0);
        let (ax, ay) = p.apply(1.0, 2.0);
        let (bx, by) = c.apply(1.0, 2.0);
        assert!((ax - bx).abs() < 1e-4 && (ay - by).abs() < 1e-4);
    }

    #[test]
    fn fast_sin_cos_is_accurate() {
        let mut x = -1000.0f32;
        while x < 1000.0 {
            let (s, c) = sin_cos(x);
            assert!((s - x.sin()).abs() < 2e-6, "sin({x})");
            assert!((c - x.cos()).abs() < 2e-6, "cos({x})");
            x += 0.0137;
        }
    }

    #[test]
    fn rng_ranges() {
        let mut r = Rng::new(42);
        for _ in 0..10_000 {
            let v = r.random();
            assert!((0.0..1.0).contains(&v));
            let i = r.int(-3, 3);
            assert!((-3..=3).contains(&i));
        }
    }
}
