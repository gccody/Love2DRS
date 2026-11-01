/// An RGBA color with components in `0.0..=1.0` (LÖVE 11 convention).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    /// Red, `0.0..=1.0`.
    pub r: f32,
    /// Green, `0.0..=1.0`.
    pub g: f32,
    /// Blue, `0.0..=1.0`.
    pub b: f32,
    /// Alpha (opacity), `0.0..=1.0`.
    pub a: f32,
}

impl Default for Color {
    fn default() -> Self {
        Color::WHITE
    }
}

impl Color {
    /// Opaque white `(1, 1, 1)`.
    pub const WHITE: Color = Color::rgb(1.0, 1.0, 1.0);
    /// Opaque black `(0, 0, 0)`.
    pub const BLACK: Color = Color::rgb(0.0, 0.0, 0.0);
    /// Fully transparent black.
    pub const TRANSPARENT: Color = Color::rgba(0.0, 0.0, 0.0, 0.0);
    /// Opaque red `(1, 0, 0)`.
    pub const RED: Color = Color::rgb(1.0, 0.0, 0.0);
    /// Opaque green `(0, 1, 0)`.
    pub const GREEN: Color = Color::rgb(0.0, 1.0, 0.0);
    /// Opaque blue `(0, 0, 1)`.
    pub const BLUE: Color = Color::rgb(0.0, 0.0, 1.0);
    /// Opaque yellow `(1, 1, 0)`.
    pub const YELLOW: Color = Color::rgb(1.0, 1.0, 0.0);
    /// Opaque cyan `(0, 1, 1)`.
    pub const CYAN: Color = Color::rgb(0.0, 1.0, 1.0);
    /// Opaque magenta `(1, 0, 1)`.
    pub const MAGENTA: Color = Color::rgb(1.0, 0.0, 1.0);
    /// Opaque orange `(1, 0.6, 0)`.
    pub const ORANGE: Color = Color::rgb(1.0, 0.6, 0.0);
    /// Opaque mid gray `(0.5, 0.5, 0.5)`.
    pub const GRAY: Color = Color::rgb(0.5, 0.5, 0.5);

    /// An opaque color from `0.0..=1.0` components.
    #[inline]
    pub const fn rgb(r: f32, g: f32, b: f32) -> Color {
        Color { r, g, b, a: 1.0 }
    }

    /// A color from `0.0..=1.0` components including alpha.
    #[inline]
    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Color {
        Color { r, g, b, a }
    }

    /// From 0-255 byte components.
    pub const fn rgba8(r: u8, g: u8, b: u8, a: u8) -> Color {
        Color {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a: a as f32 / 255.0,
        }
    }

    /// From a `0xRRGGBB` literal, e.g. `Color::hex(0xff8800)`.
    pub const fn hex(rgb: u32) -> Color {
        Color::rgba8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, 255)
    }

    /// From hue (0..360), saturation and value (0..1).
    pub fn hsv(h: f32, s: f32, v: f32) -> Color {
        let h = h.rem_euclid(360.0) / 60.0;
        let c = v * s;
        let x = c * (1.0 - (h % 2.0 - 1.0).abs());
        let (r, g, b) = match h as u32 {
            0 => (c, x, 0.0),
            1 => (x, c, 0.0),
            2 => (0.0, c, x),
            3 => (0.0, x, c),
            4 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        let m = v - c;
        Color::rgb(r + m, g + m, b + m)
    }

    /// Same color with a different alpha.
    #[inline]
    pub const fn with_alpha(self, a: f32) -> Color {
        Color { a, ..self }
    }

    /// Component-wise interpolation.
    pub fn lerp(self, other: Color, t: f32) -> Color {
        Color {
            r: self.r + (other.r - self.r) * t,
            g: self.g + (other.g - self.g) * t,
            b: self.b + (other.b - self.b) * t,
            a: self.a + (other.a - self.a) * t,
        }
    }

    /// Packed 8-bit representation used for vertices.
    #[inline]
    pub fn to_rgba8(self) -> [u8; 4] {
        #[inline(always)]
        #[allow(clippy::manual_clamp)] // `clamp` keeps NaN, which the cast must not see
        fn c(v: f32) -> u8 {
            // Unlike `clamp`, `max`/`min` turn NaN into 0, so `v` is always in
            // [0.5, 255.5] and the unchecked cast (no saturation fix-ups, so
            // it vectorizes) is sound.
            let v = v.max(0.0).min(1.0) * 255.0 + 0.5;
            // SAFETY: `v` is finite and truncates to 0..=255.
            unsafe { v.to_int_unchecked::<u8>() }
        }
        [c(self.r), c(self.g), c(self.b), c(self.a)]
    }

    pub(crate) fn to_wgpu(self) -> wgpu::Color {
        wgpu::Color {
            r: self.r as f64,
            g: self.g as f64,
            b: self.b as f64,
            a: self.a as f64,
        }
    }
}

impl From<(f32, f32, f32)> for Color {
    #[inline]
    fn from((r, g, b): (f32, f32, f32)) -> Self {
        Color::rgb(r, g, b)
    }
}

impl From<(f32, f32, f32, f32)> for Color {
    #[inline]
    fn from((r, g, b, a): (f32, f32, f32, f32)) -> Self {
        Color::rgba(r, g, b, a)
    }
}

impl From<[f32; 3]> for Color {
    #[inline]
    fn from([r, g, b]: [f32; 3]) -> Self {
        Color::rgb(r, g, b)
    }
}

impl From<[f32; 4]> for Color {
    #[inline]
    fn from([r, g, b, a]: [f32; 4]) -> Self {
        Color::rgba(r, g, b, a)
    }
}

impl From<Color> for [f32; 4] {
    fn from(c: Color) -> Self {
        [c.r, c.g, c.b, c.a]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_rgba8_rounds_and_saturates() {
        assert_eq!(
            Color::rgba(0.0, 1.0, 0.5, 0.2).to_rgba8(),
            [0, 255, 128, 51]
        );
        assert_eq!(
            Color::rgba(-3.0, 7.0, f32::INFINITY, f32::NEG_INFINITY).to_rgba8(),
            [0, 255, 255, 0]
        );
        assert_eq!(Color::rgba(f32::NAN, 1.0, 1.0, 1.0).to_rgba8()[0], 0);
    }
}
