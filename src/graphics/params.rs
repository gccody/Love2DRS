use super::Color;
use crate::math::{Transform, Vec2};

/// Placement of a drawable: LÖVE's `x, y, r, sx, sy, ox, oy` arguments as a builder.
///
/// ```
/// # use love2drs::graphics::DrawParams;
/// let p = DrawParams::at(100.0, 50.0).rotation(0.5).scale(2.0).centered();
/// ```
///
/// `(x, y)` tuples and [`Vec2`] convert into `DrawParams` automatically.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawParams {
    /// X position in pixels.
    pub x: f32,
    /// Y position in pixels.
    pub y: f32,
    /// Rotation in radians (clockwise, since y points down).
    pub rotation: f32,
    /// Horizontal scale (negative mirrors).
    pub sx: f32,
    /// Vertical scale (negative mirrors).
    pub sy: f32,
    /// Origin offset in pixels (the point that sits at `x, y` and is rotated around).
    pub ox: f32,
    /// Vertical origin offset in pixels (see `ox`).
    pub oy: f32,
    /// Origin as a fraction of the drawable's size; overrides `ox/oy` when set.
    pub anchor: Option<(f32, f32)>,
    /// Color override for this draw (defaults to the current color).
    pub color: Option<Color>,
}

impl Default for DrawParams {
    #[inline]
    fn default() -> Self {
        DrawParams {
            x: 0.0,
            y: 0.0,
            rotation: 0.0,
            sx: 1.0,
            sy: 1.0,
            ox: 0.0,
            oy: 0.0,
            anchor: None,
            color: None,
        }
    }
}

impl DrawParams {
    /// Identity params: at `(0, 0)`, no rotation, scale 1.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Params positioned at `(x, y)`.
    #[inline]
    pub fn at(x: f32, y: f32) -> Self {
        DrawParams {
            x,
            y,
            ..Default::default()
        }
    }

    /// Sets the position.
    #[inline]
    pub fn pos(mut self, x: f32, y: f32) -> Self {
        self.x = x;
        self.y = y;
        self
    }

    /// Sets the rotation in radians (clockwise on screen).
    #[inline]
    pub fn rotation(mut self, radians: f32) -> Self {
        self.rotation = radians;
        self
    }

    /// Uniform scale.
    #[inline]
    pub fn scale(mut self, s: f32) -> Self {
        self.sx = s;
        self.sy = s;
        self
    }

    /// Non-uniform scale.
    #[inline]
    pub fn scale_xy(mut self, sx: f32, sy: f32) -> Self {
        self.sx = sx;
        self.sy = sy;
        self
    }

    /// Mirror horizontally (keeps the current scale magnitude).
    #[inline]
    pub fn flip_x(mut self, flip: bool) -> Self {
        self.sx = if flip { -self.sx.abs() } else { self.sx.abs() };
        self
    }

    /// Origin in pixels.
    #[inline]
    pub fn origin(mut self, ox: f32, oy: f32) -> Self {
        self.ox = ox;
        self.oy = oy;
        self.anchor = None;
        self
    }

    /// Origin as a fraction of the drawable's size (`0.5, 0.5` = center).
    #[inline]
    pub fn anchor(mut self, ax: f32, ay: f32) -> Self {
        self.anchor = Some((ax, ay));
        self
    }

    /// Shorthand for `anchor(0.5, 0.5)`: position, rotate and scale around the center.
    #[inline]
    pub fn centered(self) -> Self {
        self.anchor(0.5, 0.5)
    }

    /// Tints this draw with `color` instead of the current [`Graphics::set_color`](super::Graphics::set_color).
    #[inline]
    pub fn color(mut self, color: impl Into<Color>) -> Self {
        self.color = Some(color.into());
        self
    }

    /// The local-to-parent transform for a drawable of size `w x h`.
    #[inline]
    pub fn to_transform(&self, w: f32, h: f32) -> Transform {
        let (ox, oy) = match self.anchor {
            Some((ax, ay)) => (ax * w, ay * h),
            None => (self.ox, self.oy),
        };
        Transform::from_params(self.x, self.y, self.rotation, self.sx, self.sy, ox, oy)
    }
}

impl From<(f32, f32)> for DrawParams {
    #[inline]
    fn from((x, y): (f32, f32)) -> Self {
        DrawParams::at(x, y)
    }
}

impl From<Vec2> for DrawParams {
    #[inline]
    fn from(v: Vec2) -> Self {
        DrawParams::at(v.x, v.y)
    }
}
