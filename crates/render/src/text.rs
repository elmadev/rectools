// Text in the games' bitmap fonts (.abc files), written as the game's abc8 writes it and drawn
// over the frame as it goes to the output

use crate::hud::{PixelRect, TextRect};
use elma_formats::abc::decode_abc;
use std::sync::OnceLock;

// The width of a space, which the fonts have no picture for (abc8.cpp)
const SPACE_WIDTH: i32 = 5;

// Pixels of one color side by side in a row of a character, from its bottom left
#[derive(Debug, Clone, Copy)]
struct Run {
    x: i32,
    y: i32,
    width: i32,
    color: u8,
}

struct Glyph {
    width: i32,
    runs: Vec<Run>,
}

pub struct Font {
    glyphs: Vec<Option<Glyph>>,
    spacing: i32,
    line_height: i32,
}

impl Font {
    // An .abc font with `spacing` pixels between characters and lines `line_height` apart
    pub fn from_abc(
        data: &[u8],
        spacing: i32,
        line_height: i32,
    ) -> Result<Font, elma_formats::Error> {
        let abc = decode_abc(data)?;
        let glyphs = (0..=255u8)
            .map(|c| {
                let g = abc.glyph(c)?;
                let w = g.pic.width;
                let mut runs = Vec::new();
                for row in 0..g.pic.height {
                    let line = row * w..(row + 1) * w;
                    let (solid, pixels) = (&g.solid[line.clone()], &g.pic.pixels[line]);
                    let mut start = 0;
                    while start < w {
                        if !solid[start] {
                            start += 1;
                            continue;
                        }
                        let color = pixels[start];
                        let end = (start..w)
                            .find(|&i| !solid[i] || pixels[i] != color)
                            .unwrap_or(w);
                        runs.push(Run {
                            x: start as i32,
                            y: g.y_offset + row as i32,
                            width: (end - start) as i32,
                            color,
                        });
                        start = end;
                    }
                }
                Some(Glyph {
                    width: w as i32,
                    runs,
                })
            })
            .collect();
        Ok(Font {
            glyphs,
            spacing,
            line_height,
        })
    }

    // The game's fonts, as it loads them (init_renderer): elma-classic's resources/small.abc,
    // medium.abc and large.abc, each without what its file has after the font
    pub fn small() -> &'static Font {
        static FONT: OnceLock<Font> = OnceLock::new();
        FONT.get_or_init(|| built_in(include_bytes!("../assets/small.abc"), 1, 12))
    }

    pub fn medium() -> &'static Font {
        static FONT: OnceLock<Font> = OnceLock::new();
        FONT.get_or_init(|| built_in(include_bytes!("../assets/medium.abc"), 1, 24))
    }

    pub fn large() -> &'static Font {
        static FONT: OnceLock<Font> = OnceLock::new();
        FONT.get_or_init(|| built_in(include_bytes!("../assets/large.abc"), 2, 90))
    }

    pub fn line_height(&self) -> i32 {
        self.line_height
    }

    // How wide `text` is in pixels, as abc8::len
    pub fn width(&self, text: &[u8]) -> i32 {
        text.iter()
            .map(|&c| match &self.glyphs[c as usize] {
                Some(g) => g.width + self.spacing,
                None if c == b' ' => SPACE_WIDTH,
                None => 0,
            })
            .sum()
    }

    // `text` from `x`, on the line at `y`, as abc8::write. Positions are in pixels from the
    // bottom left, and a pixel of the font is `scale` pixels square.
    pub(crate) fn write(&self, text: &[u8], x: i32, y: i32, scale: i32, out: &mut Vec<TextRect>) {
        let mut x = x;
        for &c in text {
            let Some(g) = &self.glyphs[c as usize] else {
                if c == b' ' {
                    x += SPACE_WIDTH;
                }
                continue;
            };
            out.extend(g.runs.iter().map(|r| TextRect {
                rect: PixelRect {
                    x: (x + r.x) * scale,
                    y: (y + r.y) * scale,
                    width: r.width * scale,
                    height: scale,
                },
                color: r.color,
            }));
            x += g.width + self.spacing;
        }
    }
}

fn built_in(data: &[u8], spacing: i32, line_height: i32) -> Font {
    Font::from_abc(data, spacing, line_height).expect("the built in font reads")
}

// Which part of the text `x` is at
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Left,
    // As abc8::write_centered
    Center,
    // As abc8::write_right_align
    Right,
}

// A line of text, in pixels from the bottom left of the output as the game writes into its
// upside-down screen: `y` is the line it sits on, and `x` where it starts, or with `align` its
// middle or end. The characters are the font's, one byte each.
#[derive(Clone, Copy)]
pub struct Text<'a> {
    pub font: &'a Font,
    pub text: &'a [u8],
    pub x: i32,
    pub y: i32,
    pub align: Align,
}

impl Text<'_> {
    pub(crate) fn rects(&self, out: &mut Vec<TextRect>) {
        let width = self.font.width(self.text);
        let x = match self.align {
            Align::Left => self.x,
            Align::Center => self.x - width / 2,
            Align::Right => self.x - width,
        };
        self.font.write(self.text, x, self.y, 1, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_built_in_fonts_read() {
        for font in [Font::small(), Font::medium(), Font::large()] {
            assert!(font.width(b"Elasto Mania") > 0);
        }
        assert_eq!(Font::large().line_height(), 90);
    }

    #[test]
    fn alignment_moves_the_text_by_its_width() {
        let rects = |align| {
            let mut out = Vec::new();
            Text {
                font: Font::small(),
                text: b"speed",
                x: 100,
                y: 20,
                align,
            }
            .rects(&mut out);
            out
        };
        let width = Font::small().width(b"speed");
        let (left, center, right) = (
            rects(Align::Left),
            rects(Align::Center),
            rects(Align::Right),
        );
        assert!(!left.is_empty());
        for ((l, c), r) in left.iter().zip(&center).zip(&right) {
            assert_eq!(c.rect.x, l.rect.x - width / 2);
            assert_eq!(r.rect.x, l.rect.x - width);
            assert_eq!(l.rect.y, r.rect.y);
        }
    }

    #[test]
    fn a_text_is_as_wide_as_its_characters() {
        let font = Font::medium();
        assert_eq!(
            font.width(b"Elma 1"),
            [b"E", b"l", b"m", b"a", b" ", b"1"]
                .iter()
                .map(|c| font.width(*c))
                .sum::<i32>()
        );
    }
}
