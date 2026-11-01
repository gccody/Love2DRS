use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use ab_glyph::{Font as _, FontArc, GlyphId, PxScale, ScaleFont};

use super::image::{FilterMode, Gpu, Image, next_id};
use crate::util::FxBuild;
use crate::{Error, Result};

static DEFAULT_FONT_BYTES: &[u8] = include_bytes!("default_font/Hack-Regular.ttf");

pub(crate) struct FontFace {
    pub id: u64,
    pub font: FontArc,
}

impl FontFace {
    /// Pixel scale that makes one em equal `size` pixels (LÖVE's size convention).
    #[inline]
    pub fn scale_for(&self, size: f32) -> PxScale {
        let upem = self.font.units_per_em().unwrap_or(1000.0);
        PxScale::from(size * self.font.height_unscaled() / upem)
    }
}

fn default_face() -> Arc<FontFace> {
    static FACE: OnceLock<Arc<FontFace>> = OnceLock::new();
    FACE.get_or_init(|| {
        let font = FontArc::try_from_slice(DEFAULT_FONT_BYTES).expect("bundled font is valid");
        Arc::new(FontFace {
            id: next_id(),
            font,
        })
    })
    .clone()
}

/// Horizontal alignment for [`Graphics::printf`](super::Graphics::printf).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    /// Flush with the left edge of the box.
    #[default]
    Left,
    /// Centered in the box.
    Center,
    /// Flush with the right edge of the box.
    Right,
}

/// A TrueType/OpenType font at a given size. Cheap to clone.
///
/// Glyphs are rasterized on demand into a shared GPU atlas, so all text
/// (even in different fonts) is drawn in as few batches as possible.
#[derive(Clone)]
pub struct Font {
    pub(crate) face: Arc<FontFace>,
    size: f32,
    line_height: f32,
}

impl std::fmt::Debug for Font {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Font")
            .field("face", &self.face.id)
            .field("size", &self.size)
            .finish()
    }
}

impl Font {
    /// The bundled default font (Hack) at the given size.
    pub fn default_with_size(size: f32) -> Font {
        Font {
            face: default_face(),
            size,
            line_height: 1.0,
        }
    }

    /// Parses a TTF/OTF font from memory, e.g. `include_bytes!("font.ttf").to_vec()`.
    pub fn from_bytes(bytes: Vec<u8>, size: f32) -> Result<Font> {
        let font = FontArc::try_from_vec(bytes).map_err(|e| Error::Font(e.to_string()))?;
        Ok(Font {
            face: Arc::new(FontFace {
                id: next_id(),
                font,
            }),
            size,
            line_height: 1.0,
        })
    }

    /// Loads a TTF/OTF file.
    pub fn load(path: impl AsRef<Path>, size: f32) -> Result<Font> {
        let bytes = std::fs::read(path.as_ref()).map_err(|e| Error::io(path.as_ref(), e))?;
        Self::from_bytes(bytes, size)
    }

    /// The same typeface at another size (shares the parsed font data).
    pub fn with_size(&self, size: f32) -> Font {
        Font {
            face: self.face.clone(),
            size,
            line_height: self.line_height,
        }
    }

    /// Size in pixels (one em).
    pub fn size(&self) -> f32 {
        self.size
    }

    fn scaled(&self) -> ab_glyph::PxScaleFont<&FontArc> {
        self.face.font.as_scaled(self.face.scale_for(self.size))
    }

    /// Distance from the top of a line to the baseline.
    pub fn ascent(&self) -> f32 {
        self.scaled().ascent()
    }

    /// Distance from the baseline to the bottom of a line (negative).
    pub fn descent(&self) -> f32 {
        self.scaled().descent()
    }

    /// Height of a line of text, excluding the line-height multiplier.
    pub fn height(&self) -> f32 {
        let s = self.scaled();
        s.ascent() - s.descent()
    }

    /// Distance between consecutive baselines.
    pub fn line_advance(&self) -> f32 {
        let s = self.scaled();
        (s.ascent() - s.descent() + s.line_gap()) * self.line_height
    }

    /// Line spacing multiplier (`font:setLineHeight`). Default `1.0`.
    pub fn set_line_height(&mut self, multiplier: f32) {
        self.line_height = multiplier;
    }

    /// The line spacing multiplier.
    pub fn line_height(&self) -> f32 {
        self.line_height
    }

    /// Width of the widest line of `text`.
    pub fn width(&self, text: &str) -> f32 {
        text.split('\n')
            .map(|l| self.line_width(l))
            .fold(0.0, f32::max)
    }

    fn line_width(&self, line: &str) -> f32 {
        let s = self.scaled();
        let mut w = 0.0;
        let mut prev: Option<GlyphId> = None;
        for ch in line.chars() {
            let id = s.glyph_id(ch);
            if let Some(p) = prev {
                w += s.kern(p, id);
            }
            w += s.h_advance(id);
            prev = Some(id);
        }
        w
    }

    /// Breaks `text` into lines no wider than `limit` (word wrapping).
    pub fn wrap(&self, text: &str, limit: f32) -> Vec<String> {
        let mut lines = Vec::new();
        for para in text.split('\n') {
            let mut line = String::new();
            let mut line_w = 0.0;
            for word in split_keep_spaces(para) {
                let ww = self.line_width(word);
                if line_w + ww > limit && !line.trim().is_empty() {
                    lines.push(line.trim_end().to_string());
                    line.clear();
                    line_w = 0.0;
                    if word.trim().is_empty() {
                        continue; // drop the space that caused the break
                    }
                }
                if ww > limit && line.is_empty() {
                    // A single word wider than the limit: break it by characters.
                    for ch in word.chars() {
                        let cw = self.line_width(ch.encode_utf8(&mut [0; 4]));
                        if line_w + cw > limit && !line.is_empty() {
                            lines.push(std::mem::take(&mut line));
                            line_w = 0.0;
                        }
                        line.push(ch);
                        line_w += cw;
                    }
                    continue;
                }
                line.push_str(word);
                line_w += ww;
            }
            lines.push(line.trim_end().to_string());
        }
        lines
    }
}

/// Splits into alternating runs of non-space and space characters.
fn split_keep_spaces(s: &str) -> impl Iterator<Item = &str> {
    let mut rest = s;
    std::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        let first_space = rest.starts_with(' ');
        let end = rest
            .find(|c: char| (c == ' ') != first_space)
            .unwrap_or(rest.len());
        let (word, tail) = rest.split_at(end);
        rest = tail;
        Some(word)
    })
}

// ---------------------------------------------------------------------------
// Glyph atlas
// ---------------------------------------------------------------------------

#[derive(Hash, PartialEq, Eq, Clone, Copy)]
struct GlyphKey {
    face: u64,
    glyph: u16,
    px: u32,
}

/// Where a rasterized glyph lives in the atlas, in physical pixels relative to
/// the pen position on the baseline.
#[derive(Clone, Copy)]
pub(crate) struct GlyphEntry {
    pub page: usize,
    pub uv: [f32; 4],
    pub offset: [f32; 2],
    pub size: [f32; 2],
}

struct Shelf {
    y: u32,
    h: u32,
    x: u32,
}

struct AtlasPage {
    image: Image,
    pixels: Vec<u8>,
    shelves: Vec<Shelf>,
    next_y: u32,
    dirty: Option<(u32, u32)>,
}

impl AtlasPage {
    fn alloc(&mut self, w: u32, h: u32, size: u32) -> Option<(u32, u32)> {
        for shelf in &mut self.shelves {
            if h <= shelf.h && h * 10 >= shelf.h * 7 && shelf.x + w <= size {
                let pos = (shelf.x, shelf.y);
                shelf.x += w;
                return Some(pos);
            }
        }
        if self.next_y + h <= size && w <= size {
            let y = self.next_y;
            self.shelves.push(Shelf { y, h, x: w });
            self.next_y += h;
            return Some((0, y));
        }
        None
    }
}

pub(crate) struct GlyphAtlas {
    gpu: Arc<Gpu>,
    size: u32,
    pages: Vec<AtlasPage>,
    cache: HashMap<GlyphKey, Option<GlyphEntry>, FxBuild>,
    white_uv: [f32; 2],
}

const PAD: u32 = 1;
/// When more pages than this exist at the start of a frame, the atlas is rebuilt.
const MAX_PAGES: usize = 4;

impl GlyphAtlas {
    pub fn new(gpu: Arc<Gpu>) -> Self {
        let max = gpu.device.limits().max_texture_dimension_2d;
        let mut atlas = GlyphAtlas {
            gpu,
            size: 2048.min(max),
            pages: Vec::new(),
            cache: HashMap::default(),
            white_uv: [0.0; 2],
        };
        atlas.reserve_white();
        atlas
    }

    /// Reserves an opaque white block on page 0. Shapes sample it, so shapes and
    /// text on the first page share a texture and batch together.
    fn reserve_white(&mut self) {
        let (page, x, y) = self
            .alloc(4, 4)
            .expect("atlas has room for the white block");
        let size = self.size;
        let p = &mut self.pages[page];
        for yy in y..y + 4 {
            for xx in x..x + 4 {
                p.pixels[((yy * size + xx) * 4 + 3) as usize] = 255;
            }
        }
        p.dirty = Some((y, y + 4));
        self.white_uv = [
            (x as f32 + 2.0) / size as f32,
            (y as f32 + 2.0) / size as f32,
        ];
    }

    /// The image and texture coordinate to use for untextured geometry.
    pub fn white(&self) -> (Image, [f32; 2]) {
        (self.pages[0].image.clone(), self.white_uv)
    }

    /// Drops every cached glyph if the atlas grew too large (e.g. text drawn at
    /// many animated sizes). Must only be called between frames.
    pub fn trim(&mut self) -> bool {
        if self.pages.len() <= MAX_PAGES {
            return false;
        }
        self.pages.clear();
        self.cache.clear();
        self.reserve_white();
        true
    }

    pub fn page_image(&self, page: usize) -> &Image {
        &self.pages[page].image
    }

    /// Returns the atlas entry for a glyph at a physical pixel scale, rasterizing
    /// it on first use. `None` for glyphs without an outline (e.g. spaces).
    pub fn glyph(&mut self, face: &FontFace, id: GlyphId, scale: PxScale) -> Option<GlyphEntry> {
        let key = GlyphKey {
            face: face.id,
            glyph: id.0,
            px: scale.y.to_bits(),
        };
        if let Some(e) = self.cache.get(&key) {
            return *e;
        }
        let entry = self.rasterize(face, id, scale);
        self.cache.insert(key, entry);
        entry
    }

    fn rasterize(&mut self, face: &FontFace, id: GlyphId, scale: PxScale) -> Option<GlyphEntry> {
        let glyph = id.with_scale_and_position(scale, ab_glyph::point(0.0, 0.0));
        let outline = face.font.outline_glyph(glyph)?;
        let bounds = outline.px_bounds();
        let w = bounds.width().ceil() as u32;
        let h = bounds.height().ceil() as u32;
        if w == 0 || h == 0 {
            return None;
        }
        let (page_idx, x, y) = self.alloc(w + PAD * 2, h + PAD * 2)?;
        let size = self.size;
        let page = &mut self.pages[page_idx];
        let (ox, oy) = (x + PAD, y + PAD);
        outline.draw(|gx, gy, cov| {
            let px = ox + gx;
            let py = oy + gy;
            if px < size && py < size {
                let i = ((py * size + px) * 4) as usize;
                page.pixels[i + 3] = (cov.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            }
        });
        let (d0, d1) = page.dirty.unwrap_or((y, y + h + PAD * 2));
        page.dirty = Some((d0.min(y), d1.max(y + h + PAD * 2)));
        let inv = 1.0 / size as f32;
        Some(GlyphEntry {
            page: page_idx,
            uv: [
                ox as f32 * inv,
                oy as f32 * inv,
                (ox + w) as f32 * inv,
                (oy + h) as f32 * inv,
            ],
            offset: [bounds.min.x, bounds.min.y],
            size: [w as f32, h as f32],
        })
    }

    fn alloc(&mut self, w: u32, h: u32) -> Option<(usize, u32, u32)> {
        if w > self.size || h > self.size {
            log::warn!("glyph of {w}x{h} px is larger than the glyph atlas; skipping it");
            return None;
        }
        if let Some(page) = self.pages.last_mut()
            && let Some((x, y)) = page.alloc(w, h, self.size)
        {
            return Some((self.pages.len() - 1, x, y));
        }
        let size = self.size;
        // White texels; only alpha carries coverage, so text uses the same shader as sprites.
        let mut pixels = vec![255u8; (size * size * 4) as usize];
        pixels.iter_mut().skip(3).step_by(4).for_each(|a| *a = 0);
        let image = Image::create(
            &self.gpu,
            size,
            size,
            Some(&pixels),
            FilterMode::Linear,
            false,
        );
        self.pages.push(AtlasPage {
            image,
            pixels,
            shelves: Vec::new(),
            next_y: 0,
            dirty: None,
        });
        let page = self.pages.last_mut().unwrap();
        let (x, y) = page.alloc(w, h, size)?;
        Some((self.pages.len() - 1, x, y))
    }

    /// Uploads glyphs rasterized since the last call.
    pub fn flush(&mut self) {
        let size = self.size;
        for page in &mut self.pages {
            if let Some((y0, y1)) = page.dirty.take() {
                let y1 = y1.min(size);
                let start = (y0 * size * 4) as usize;
                let end = (y1 * size * 4) as usize;
                page.image
                    .write_region(0, y0, size, y1 - y0, &page.pixels[start..end]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_respects_limit() {
        let font = Font::default_with_size(16.0);
        let text = "the quick brown fox jumps over the lazy dog";
        let limit = font.width("the quick brown");
        let lines = font.wrap(text, limit);
        assert!(lines.len() >= 3);
        for l in &lines {
            assert!(font.width(l) <= limit + 0.01, "{l:?} too wide");
        }
        assert_eq!(lines.join(" "), text);
    }

    #[test]
    fn metrics_are_sane() {
        let font = Font::default_with_size(20.0);
        assert!(font.height() > 15.0 && font.height() < 40.0);
        assert!(font.width("WWW") > font.width("W"));
        assert_eq!(font.width("a\nbbb"), font.width("bbb"));
    }
}
