//! A procedural bitmap font. Every glyph is a hand-written 5x7 pixel pattern in this file (clean
//! room: nothing is taken from any game or font), laid out proportionally and rasterised into a
//! small RGBA image that is shown with nearest-neighbour scaling, which gives the blocky,
//! pixel-perfect look at every UI scale.
//!
//! [`PixelText`] is the component all UI text uses; a system turns changed strings into images.

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::ui::widget::NodeImageMode;
use std::collections::HashMap;
use std::sync::OnceLock;

/// Rows of every glyph cell (capitals use rows 0..7, descenders reach row 8).
pub const GLYPH_ROWS: usize = 9;
/// Height of one text line in font pixels.
pub const LINE_HEIGHT: usize = 10;
const SPACE_ADVANCE: usize = 3;

/// `(character, rows)` — rows are separated by `/`, `#` is a lit pixel and `.` an empty one.
/// Missing trailing rows are empty; glyphs shorter than the cell are padded on the right.
const GLYPHS: &[(char, &str)] = &[
    ('A', ".###./#...#/#...#/#####/#...#/#...#/#...#"),
    ('B', "####./#...#/#...#/####./#...#/#...#/####."),
    ('C', ".###./#...#/#..../#..../#..../#...#/.###."),
    ('D', "####./#...#/#...#/#...#/#...#/#...#/####."),
    ('E', "#####/#..../#..../####./#..../#..../#####"),
    ('F', "#####/#..../#..../####./#..../#..../#...."),
    ('G', ".###./#...#/#..../#.###/#...#/#...#/.###."),
    ('H', "#...#/#...#/#...#/#####/#...#/#...#/#...#"),
    ('I', "###/.#./.#./.#./.#./.#./###"),
    ('J', "..###/...#./...#./...#./...#./#..#./.##.."),
    ('K', "#...#/#..#./#.#../##.../#.#../#..#./#...#"),
    ('L', "#..../#..../#..../#..../#..../#..../#####"),
    ('M', "#...#/##.##/#.#.#/#.#.#/#...#/#...#/#...#"),
    ('N', "#...#/##..#/#.#.#/#..##/#...#/#...#/#...#"),
    ('O', ".###./#...#/#...#/#...#/#...#/#...#/.###."),
    ('P', "####./#...#/#...#/####./#..../#..../#...."),
    ('Q', ".###./#...#/#...#/#...#/#.#.#/#..#./.##.#"),
    ('R', "####./#...#/#...#/####./#.#../#..#./#...#"),
    ('S', ".####/#..../#..../.###./....#/....#/####."),
    ('T', "#####/..#../..#../..#../..#../..#../..#.."),
    ('U', "#...#/#...#/#...#/#...#/#...#/#...#/.###."),
    ('V', "#...#/#...#/#...#/#...#/#...#/.#.#./..#.."),
    ('W', "#...#/#...#/#...#/#.#.#/#.#.#/##.##/#...#"),
    ('X', "#...#/#...#/.#.#./..#../.#.#./#...#/#...#"),
    ('Y', "#...#/#...#/.#.#./..#../..#../..#../..#.."),
    ('Z', "#####/....#/...#./..#../.#.../#..../#####"),
    ('a', "//.###./....#/.####/#...#/.####"),
    ('b', "#..../#..../####./#...#/#...#/#...#/####."),
    ('c', "//.###./#..../#..../#..../.###."),
    ('d', "....#/....#/.####/#...#/#...#/#...#/.####"),
    ('e', "//.###./#...#/#####/#..../.###."),
    ('f', "..##/.#../####/.#../.#../.#../.#.."),
    ('g', "//.####/#...#/#...#/.####/....#/#...#/.###."),
    ('h', "#..../#..../####./#...#/#...#/#...#/#...#"),
    ('i', "./#/./#/#/#/#"),
    ('j', "../.#/../.#/.#/.#/.#/.#/#."),
    ('k', "#.../#.../#..#/#.#./##../#.#./#..#"),
    ('l', "##./.#./.#./.#./.#./.#./..#"),
    ('m', "//##.#./#.#.#/#.#.#/#.#.#/#.#.#"),
    ('n', "//####./#...#/#...#/#...#/#...#"),
    ('o', "//.###./#...#/#...#/#...#/.###."),
    ('p', "//####./#...#/#...#/####./#..../#..../#...."),
    ('q', "//.####/#...#/#...#/.####/....#/....#/....#"),
    ('r', "//#.##/##../#.../#.../#..."),
    ('s', "//.####/#..../.###./....#/####."),
    ('t', ".#.../.#.../####./.#.../.#.../.#.../..##."),
    ('u', "//#...#/#...#/#...#/#...#/.####"),
    ('v', "//#...#/#...#/#...#/.#.#./..#.."),
    ('w', "//#...#/#...#/#.#.#/#.#.#/.#.#."),
    ('x', "//#...#/.#.#./..#../.#.#./#...#"),
    ('y', "//#...#/#...#/#...#/.####/....#/#...#/.###."),
    ('z', "//#####/...#./..#../.#.../#####"),
    ('0', ".###./#...#/#..##/#.#.#/##..#/#...#/.###."),
    ('1', "..#../.##../..#../..#../..#../..#../.###."),
    ('2', ".###./#...#/....#/...#./..#../.#.../#####"),
    ('3', ".###./#...#/....#/..##./....#/#...#/.###."),
    ('4', "...#./..##./.#.#./#..#./#####/...#./...#."),
    ('5', "#####/#..../####./....#/....#/#...#/.###."),
    ('6', ".###./#..../#..../####./#...#/#...#/.###."),
    ('7', "#####/....#/...#./..#../.#.../.#.../.#..."),
    ('8', ".###./#...#/#...#/.###./#...#/#...#/.###."),
    ('9', ".###./#...#/#...#/.####/....#/....#/.###."),
    ('!', "#/#/#/#/#/./#"),
    ('"', "#.#/#.#"),
    ('#', ".#.#./.#.#./#####/.#.#./#####/.#.#./.#.#."),
    ('$', "..#../.####/#.#../.###./..#.#/####./..#.."),
    ('%', "##..#/##..#/...#./..#../.#.../#..##/#..##"),
    ('&', ".##../#..#./#.#../.#.../#.#.#/#..#./.##.#"),
    ('\'', "#/#"),
    ('(', "..#/.#./#../#../#../.#./..#"),
    (')', "#../.#./..#/..#/..#/.#./#.."),
    ('*', "/#.#.#/.###./#####/.###./#.#.#"),
    ('+', "/..#../..#../#####/..#../..#.."),
    (',', "//////.#/.#/#."),
    ('-', "///####"),
    ('.', "//////#"),
    ('/', "....#/...#./..#../..#../.#.../#..../#...."),
    (':', "//#///#"),
    (';', "//.#///.#/.#/#."),
    ('<', "...#/..#./.#../#.../.#../..#./...#"),
    ('=', "//#####//#####"),
    ('>', "#.../.#../..#./...#/..#./.#../#..."),
    ('?', ".###./#...#/....#/...#./..#../...../..#.."),
    ('@', ".###./#...#/#.###/#.#.#/#.###/#..../.###."),
    ('[', "###/#../#../#../#../#../###"),
    ('\\', "#..../#..../.#.../..#../..#../...#./....#"),
    (']', "###/..#/..#/..#/..#/..#/###"),
    ('^', "..#../.#.#./#...#"),
    ('_', "///////#####"),
    ('`', "#./.#"),
    ('{', "..##/.#../.#../#.../.#../.#../..##"),
    ('|', "#/#/#/#/#/#/#"),
    ('}', "##../..#./..#./...#/..#./..#./##.."),
    ('~', "///.##.#/#.##."),
];

/// The glyph drawn for characters the font does not know.
const FALLBACK: &str = "#####/#...#/#...#/#...#/#...#/#...#/#####";

#[derive(Clone, Debug)]
pub struct Glyph {
    /// One bit per column, bit 0 = leftmost column.
    pub rows: [u8; GLYPH_ROWS],
    pub width: usize,
}

fn parse(rows: &str) -> Glyph {
    let mut out = [0u8; GLYPH_ROWS];
    let mut width = 0;
    for (y, row) in rows.split('/').take(GLYPH_ROWS).enumerate() {
        for (x, c) in row.chars().enumerate() {
            if c == '#' {
                out[y] |= 1 << x;
                width = width.max(x + 1);
            }
        }
    }
    Glyph { rows: out, width }
}

fn table() -> &'static HashMap<char, Glyph> {
    static TABLE: OnceLock<HashMap<char, Glyph>> = OnceLock::new();
    TABLE.get_or_init(|| GLYPHS.iter().map(|(c, rows)| (*c, parse(rows))).collect())
}

pub fn glyph(c: char) -> Glyph {
    table().get(&c).cloned().unwrap_or_else(|| parse(FALLBACK))
}

fn advance(c: char) -> usize {
    if c == ' ' { SPACE_ADVANCE } else { glyph(c).width + 1 }
}

/// Width of the widest line of `text` in font pixels (without the trailing letter gap).
pub fn text_width(text: &str) -> usize {
    text.split('\n').map(|line| line.chars().map(advance).sum::<usize>().saturating_sub(1)).max().unwrap_or(0)
}

pub fn line_count(text: &str) -> usize {
    text.split('\n').count().max(1)
}

/// An RGBA bitmap of rendered text (white glyphs, black drop shadow).
#[derive(Debug, Clone, PartialEq)]
pub struct TextBitmap {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u8>,
}

/// Renders `text` at 1x: white glyph pixels and, if `shadow`, a black copy shifted one pixel
/// down and right behind them.
pub fn rasterize(text: &str, shadow: bool) -> TextBitmap {
    let extra = shadow as usize;
    let width = text_width(text).max(1) + extra;
    let height = line_count(text) * LINE_HEIGHT - 1 + extra;
    let mut data = vec![0u8; width * height * 4];
    let mut put = |x: usize, y: usize, rgba: [u8; 4]| {
        if x < width && y < height {
            let i = (y * width + x) * 4;
            data[i..i + 4].copy_from_slice(&rgba);
        }
    };
    for pass in 0..=extra {
        // Pass 0 draws the shadow (if any), the last pass the text itself.
        let is_shadow = shadow && pass == 0;
        let (off, color) = if is_shadow { (1, [0, 0, 0, 255]) } else { (0, [255, 255, 255, 255]) };
        for (line_no, line) in text.split('\n').enumerate() {
            let mut x = 0;
            for c in line.chars() {
                if c != ' ' {
                    let g = glyph(c);
                    for (gy, bits) in g.rows.iter().enumerate() {
                        for gx in 0..g.width {
                            if bits >> gx & 1 == 1 {
                                put(x + gx + off, line_no * LINE_HEIGHT + gy + off, color);
                            }
                        }
                    }
                }
                x += advance(c);
            }
        }
    }
    TextBitmap { width, height, data }
}

/// Size in UI pixels a text of `scale` takes up.
pub fn text_size(text: &str, scale: f32, shadow: bool) -> Vec2 {
    let extra = shadow as usize;
    Vec2::new(
        (text_width(text).max(1) + extra) as f32 * scale,
        (line_count(text) * LINE_HEIGHT - 1 + extra) as f32 * scale,
    )
}

/// UI text in the pixel font. Insert it on any entity; it brings its own `Node` and `ImageNode`.
#[derive(Component, Clone, Debug, PartialEq)]
#[require(Node, ImageNode)]
pub struct PixelText {
    pub text: String,
    /// UI pixels per font pixel (2.0 = 14 px tall capitals).
    pub scale: f32,
    pub color: Color,
    pub shadow: bool,
}

impl PixelText {
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into(), scale: 2.0, color: Color::WHITE, shadow: true }
    }

    pub fn scale(mut self, scale: f32) -> Self {
        self.scale = scale;
        self
    }

    pub fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    pub fn no_shadow(mut self) -> Self {
        self.shadow = false;
        self
    }

    /// Replaces the string, touching the component only if it actually differs.
    pub fn set(this: &mut Mut<PixelText>, text: &str) {
        if this.text != text {
            this.text = text.to_string();
        }
    }
}

fn bitmap_image(bitmap: TextBitmap) -> Image {
    let mut image = Image::new(
        Extent3d { width: bitmap.width as u32, height: bitmap.height as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        bitmap.data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::nearest();
    image
}

/// Rebuilds the image of every text whose string, size or colour changed.
pub fn render_pixel_text(
    mut images: ResMut<Assets<Image>>,
    mut texts: Query<(&PixelText, &mut ImageNode, &mut Node), Changed<PixelText>>,
) {
    for (text, mut image, mut node) in &mut texts {
        let bitmap = rasterize(&text.text, text.shadow);
        let size = Vec2::new(bitmap.width as f32, bitmap.height as f32) * text.scale;
        image.image = images.add(bitmap_image(bitmap));
        image.color = text.color;
        image.image_mode = NodeImageMode::Stretch;
        node.width = Val::Px(size.x);
        node.height = Val::Px(size.y);
        node.flex_shrink = 0.0;
    }
}

pub struct PixelFontPlugin;

impl Plugin for PixelFontPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostUpdate, render_pixel_text.before(bevy::ui::UiSystems::Prepare));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_printable_ascii_character_has_a_glyph() {
        for c in (' '..='~').filter(|c| *c != ' ') {
            assert!(table().contains_key(&c), "missing glyph for {c:?}");
            let g = glyph(c);
            assert!(g.width >= 1 && g.width <= 5, "{c:?} is {} wide", g.width);
            assert!(g.rows.iter().any(|r| *r != 0), "{c:?} is blank");
        }
        assert_eq!(table().len(), 94);
    }

    #[test]
    fn glyph_table_has_no_duplicate_characters() {
        let mut seen = std::collections::HashSet::new();
        for (c, _) in GLYPHS {
            assert!(seen.insert(*c), "duplicate glyph {c:?}");
        }
    }

    #[test]
    fn capitals_are_seven_rows_tall_and_descenders_hang_below() {
        let rows_used = |c: char| -> (usize, usize) {
            let g = glyph(c);
            let used: Vec<usize> = (0..GLYPH_ROWS).filter(|r| g.rows[*r] != 0).collect();
            (used[0], *used.last().unwrap())
        };
        assert_eq!(rows_used('H'), (0, 6));
        assert_eq!(rows_used('x'), (2, 6));
        assert_eq!(rows_used('g').1, 8, "g has a descender");
        assert_eq!(rows_used('p').1, 8);
        assert_eq!(rows_used('l').0, 0, "ascender");
    }

    #[test]
    fn widths_are_proportional() {
        assert_eq!(text_width("I"), 3);
        assert_eq!(text_width("i"), 1);
        assert_eq!(text_width("HH"), 5 + 1 + 5);
        assert_eq!(text_width("a b"), (5 + 1) + 3 + (5 + 1) - 1);
        assert!(text_width("Minecraft") > text_width("Mine"));
        assert_eq!(text_width(""), 0);
        // The widest line wins.
        assert_eq!(text_width("HH\nH"), text_width("HH"));
    }

    #[test]
    fn rasterizing_draws_glyph_pixels_and_a_shadow() {
        let plain = rasterize("I", false);
        assert_eq!((plain.width, plain.height), (3, 9));
        // The top bar of the I: three white pixels in row 0.
        for x in 0..3 {
            assert_eq!(&plain.data[x * 4..x * 4 + 4], &[255, 255, 255, 255]);
        }
        let shadowed = rasterize("I", true);
        assert_eq!((shadowed.width, shadowed.height), (4, 10));
        // Pixel (3, 1) is the shadow of (2, 0): black but opaque; (0, 1) is background... the stem of
        // the I covers (1, 1) so it is white.
        let px = |x: usize, y: usize| &shadowed.data[(y * shadowed.width + x) * 4..(y * shadowed.width + x) * 4 + 4];
        assert_eq!(px(3, 1), &[0, 0, 0, 255]);
        assert_eq!(px(1, 1), &[255, 255, 255, 255]);
        assert_eq!(px(0, 9)[3], 0, "far corner stays transparent");
        // Same text, same pixels.
        assert_eq!(rasterize("Hello", true), rasterize("Hello", true));
    }

    #[test]
    fn multi_line_text_stacks_lines() {
        let one = rasterize("Hi", false);
        let two = rasterize("Hi\nHi", false);
        assert_eq!(two.height, one.height + LINE_HEIGHT);
        // The second line is a copy of the first.
        let row = |b: &TextBitmap, y: usize| b.data[y * b.width * 4..(y + 1) * b.width * 4].to_vec();
        assert_eq!(row(&two, 0), row(&two, LINE_HEIGHT));
    }

    #[test]
    fn unknown_characters_use_the_fallback_box() {
        let g = glyph('\u{00e4}');
        assert_eq!(g.width, 5);
        assert_eq!(g.rows[0], 0b11111);
    }

    #[test]
    fn text_size_scales_with_the_pixel_size() {
        let s1 = text_size("Hello", 1.0, true);
        let s3 = text_size("Hello", 3.0, true);
        assert_eq!(s3, s1 * 3.0);
        assert_eq!(text_size("A\nB", 2.0, false).y, (2 * LINE_HEIGHT - 1) as f32 * 2.0);
    }
}
