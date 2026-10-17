// An .abc font, as the game writes its text with (elma-classic's abc8): a sprite for each
// character it has, in the colors of the palette it is drawn with
use crate::Error;
use crate::pic::Pic8;
use crate::reader::Reader;

#[derive(Debug, Clone, PartialEq)]
pub struct Glyph {
    // Rows from the bottom, as the game stores and draws them
    pub pic: Pic8,
    // Whether each pixel is drawn, in the same order
    pub solid: Vec<bool>,
    // How far above the line the bottom row goes
    pub y_offset: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Font {
    glyphs: Vec<Option<Glyph>>,
}

impl Font {
    pub fn glyph(&self, c: u8) -> Option<&Glyph> {
        self.glyphs[c as usize].as_ref()
    }
}

pub fn decode_abc(data: &[u8]) -> Result<Font, Error> {
    let mut r = Reader::new(data);
    if r.bytes(4)? != b"RA1\0" {
        return r.invalid("not an .abc font");
    }
    let count = r.i16()?;
    if !(1..=256).contains(&count) {
        return r.invalid("character count invalid");
    }
    let mut glyphs = vec![None; 256];
    for _ in 0..count {
        if r.bytes(7)? != b"EGYMIX\0" {
            return r.invalid("character header invalid");
        }
        let c = r.u8()? as usize;
        let y_offset = r.i16()? as i32;
        let glyph = sprite(&mut r, y_offset)?;
        if glyphs[c].is_some() {
            return r.invalid("character given twice");
        }
        glyphs[c] = Some(glyph);
    }
    Ok(Font { glyphs })
}

// A .spr sprite, keeping which pixels its transparency runs leave out: per row, 'K' and a
// length for solid pixels, 'N' and a length for transparent ones
fn sprite(r: &mut Reader, y_offset: i32) -> Result<Glyph, Error> {
    if r.u8()? != 0x2d {
        return r.invalid("sprite header invalid");
    }
    let width = r.u16()? as usize;
    let height = r.u16()? as usize;
    if width < 1 || height < 1 {
        return r.invalid("sprite size invalid");
    }
    let pixels = r.bytes(width * height)?.to_vec();
    if r.bytes(7)? != b"SPRITE\0" {
        return r.invalid("sprite data header invalid");
    }
    let length = r.u16()? as usize;
    let mut runs = Reader::new(r.bytes(length)?);
    let mut solid = Vec::with_capacity(width * height);
    for _ in 0..height {
        let row_end = solid.len() + width;
        while solid.len() < row_end {
            let kind = runs.u8()?;
            let n = runs.u8()? as usize;
            // A run of nothing would never finish the row
            if n == 0 {
                return runs.invalid("sprite run empty");
            }
            if solid.len() + n > row_end {
                return runs.invalid("sprite run past the end of its row");
            }
            match kind {
                b'K' => solid.resize(solid.len() + n, true),
                b'N' => solid.resize(solid.len() + n, false),
                _ => return runs.invalid("sprite run invalid"),
            }
        }
    }
    Ok(Glyph {
        pic: Pic8 {
            width,
            height,
            pixels,
        },
        solid,
        y_offset,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SMALL: &[u8] = include_bytes!("../../render/assets/small.abc");

    // The rows from the top, # for a solid pixel
    fn picture(g: &Glyph) -> Vec<String> {
        let w = g.pic.width;
        (0..g.pic.height)
            .rev()
            .map(|y| {
                (0..w)
                    .map(|x| if g.solid[y * w + x] { '#' } else { '.' })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn reads_the_games_small_font() {
        let font = decode_abc(SMALL).unwrap();
        for c in b' '..=b'~' {
            assert!(font.glyph(c).is_some(), "{}", c as char);
        }
        let seven = font.glyph(b'7').unwrap();
        assert_eq!(seven.y_offset, 3);
        assert_eq!(
            picture(seven),
            [
                "######", ".....#", "....#.", "....#.", "...#..", "...#..", "..#...", "..#...",
                ".#....",
            ]
        );
        // Every solid pixel in the one color
        let g = font.glyph(b'g').unwrap();
        assert!(
            (0..g.solid.len())
                .filter(|&i| g.solid[i])
                .all(|i| g.pic.pixels[i] == 25)
        );
    }

    #[test]
    fn a_cut_file_is_an_error() {
        assert!(decode_abc(&SMALL[..SMALL.len() - 1]).is_err());
        assert!(decode_abc(b"RA1\0").is_err());
    }
}
