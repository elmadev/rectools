use crate::reader::Reader;
use crate::{Error, invalid};

// An 8-bit palette-indexed image, rows stored top to bottom
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pic8 {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
}

impl Pic8 {
    pub fn new(width: usize, height: usize) -> Pic8 {
        Pic8 {
            width,
            height,
            pixels: vec![0; width * height],
        }
    }

    pub fn filled(width: usize, height: usize, index: u8) -> Pic8 {
        Pic8 {
            width,
            height,
            pixels: vec![index; width * height],
        }
    }

    pub fn get(&self, x: usize, y: usize) -> u8 {
        self.pixels[y * self.width + x]
    }

    pub fn set(&mut self, x: usize, y: usize, index: u8) {
        self.pixels[y * self.width + x] = index;
    }

    pub fn row(&self, y: usize) -> &[u8] {
        &self.pixels[y * self.width..(y + 1) * self.width]
    }

    // Copy of the rectangle (x1, y1)-(x2, y2), inclusive, with zeros outside the picture
    pub fn crop(&self, x1: i32, y1: i32, x2: i32, y2: i32) -> Pic8 {
        let mut out = Pic8::new((x2 - x1 + 1) as usize, (y2 - y1 + 1) as usize);
        for y in 0..out.height {
            for x in 0..out.width {
                let (sx, sy) = (x1 + x as i32, y1 + y as i32);
                if sx >= 0 && sy >= 0 && (sx as usize) < self.width && (sy as usize) < self.height {
                    out.set(x, y, self.get(sx as usize, sy as usize));
                }
            }
        }
        out
    }

    pub fn flipped_vertically(&self) -> Pic8 {
        let mut out = Pic8::new(self.width, self.height);
        for y in 0..self.height {
            let dst = self.height - 1 - y;
            out.pixels[dst * self.width..(dst + 1) * self.width].copy_from_slice(self.row(y));
        }
        out
    }

    pub fn transposed(&self) -> Pic8 {
        let mut out = Pic8::new(self.height, self.width);
        for y in 0..self.height {
            for x in 0..self.width {
                out.set(y, x, self.get(x, y));
            }
        }
        out
    }

    // Nearest neighbour rescale to `target_height`, exactly like the game's pic8::resize
    pub fn resized(&self, target_height: i32) -> Pic8 {
        let target_height = target_height.max(1) as usize;
        if target_height == self.height {
            return self.clone();
        }
        let scale = target_height as f64 / self.height as f64;
        let target_width = ((self.width as f64 * scale) as i64).max(1) as usize;
        let mut out = Pic8::new(target_width, target_height);
        for y in 0..target_height {
            let sy = ((y as f64 / scale) as usize).min(self.height - 1);
            for x in 0..target_width {
                let sx = ((x as f64 / scale) as usize).min(self.width - 1);
                out.set(x, y, self.get(sx, sy));
            }
        }
        out
    }

    // Width the game gives this picture after resizing it to `target_height`
    pub fn resized_width(&self, target_height: i32) -> usize {
        let target_height = target_height.max(1) as usize;
        if target_height == self.height {
            return self.width;
        }
        let scale = target_height as f64 / self.height as f64;
        ((self.width as f64 * scale) as i64).max(1) as usize
    }
}

// Decode an 8-bit run-length encoded PCX
pub fn decode_pcx(data: &[u8]) -> Result<Pic8, Error> {
    let mut r = Reader::new(data);
    let header = r.bytes(128)?;
    let manufacturer = header[0];
    let version = header[1];
    let encoding = header[2];
    let bits_per_plane = header[3];
    let word = |offset: usize| i16::from_le_bytes([header[offset], header[offset + 1]]);
    let (xmin, ymin, xmax, ymax) = (word(4), word(6), word(8), word(10));
    let planes = header[65];
    let bytes_per_line = word(66) as i32;
    if version != 5 || manufacturer != 10 || encoding != 1 || bits_per_plane != 8 || planes != 1 {
        return r.invalid("PCX header invalid or not supported");
    }
    let width = (xmax as i32 - xmin as i32 + 1).max(0) as usize;
    let height = (ymax as i32 - ymin as i32 + 1).max(0) as usize;
    if width == 0 || height == 0 {
        return r.invalid("PCX has no pixels");
    }
    // A run is two bytes for at most 63 pixels, so a short file can't claim a huge picture
    r.has_items(width.div_ceil(63) * height, 2)?;
    let mut pic = Pic8::new(width, height);
    for y in 0..height {
        let mut x: i32 = 0;
        loop {
            let c = r.u8()?;
            if c & 0xc0 == 0xc0 {
                let count = c & 0x3f;
                let value = r.u8()?;
                for _ in 0..count {
                    if (x as usize) < width {
                        pic.set(x as usize, y, value);
                    }
                    x += 1;
                }
            } else {
                if (x as usize) < width {
                    pic.set(x as usize, y, c);
                }
                x += 1;
            }
            // The game reads at least one byte per row, like a do-while loop
            if x >= bytes_per_line {
                break;
            }
        }
    }
    Ok(pic)
}

// The 256 color palette appended to a PCX file
pub fn pcx_palette(data: &[u8]) -> Result<[u8; 768], Error> {
    if data.len() < 769 {
        return Err(Error::Eof { at: data.len() });
    }
    let tail = &data[data.len() - 769..];
    if tail[0] != 0x0c {
        return invalid("PCX palette header invalid");
    }
    Ok(tail[1..].try_into().unwrap())
}

// A .spr sprite: raw pixels plus transparency runs, which are skipped
pub fn decode_spr(data: &[u8]) -> Result<Pic8, Error> {
    let mut r = Reader::new(data);
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
    r.bytes(length)?;
    Ok(Pic8 {
        width,
        height,
        pixels,
    })
}

// An 8-bit indexed BMP, the palette is ignored like in the game
pub fn decode_bmp(data: &[u8]) -> Result<Pic8, Error> {
    let mut r = Reader::new(data);
    if r.u16()? != 0x4d42 {
        return r.invalid("not a BMP file");
    }
    let _size = r.u32()?;
    let _reserved = r.u32()?;
    let offset = r.u32()? as usize;
    let dib_size = r.u32()?;
    let width = r.i32()?;
    let height = r.i32()?;
    let _planes = r.u16()?;
    let bpp = r.u16()?;
    if dib_size == 12 || dib_size == 64 || dib_size == 16 {
        return r.invalid("unsupported BMP header");
    }
    if bpp != 8 || height < 1 || width < 1 {
        return r.invalid("BMP must be 8 bits per pixel and stored bottom up");
    }
    let (width, height) = (width as usize, height as usize);
    let padded_width = width.div_ceil(4) * 4;
    r.seek(offset)?;
    let size = padded_width
        .checked_mul(height)
        .ok_or(Error::Eof { at: r.pos() })?;
    let pixels = r.bytes(size)?;
    let mut pic = Pic8::new(width, height);
    for y in 0..height {
        let src = height - 1 - y;
        pic.pixels[y * width..(y + 1) * width]
            .copy_from_slice(&pixels[src * padded_width..src * padded_width + width]);
    }
    Ok(pic)
}

// A player's shirt (bmp/<nick>.bmp), with the size limits the game checks
pub fn decode_shirt(data: &[u8]) -> Option<Pic8> {
    let pic = decode_bmp(data).ok()?;
    ((149..=152).contains(&pic.width) && pic.height <= 101).then_some(pic)
}

// A shirt from its colors, `width` by `height` RGBA pixels. Sites show shirts upright, the
// bmp turned a quarter counter-clockwise, with the bmp's transparent color as alpha 0; such a
// picture is turned back. Each color becomes the nearest one of `palette`, the LGR's, and the
// transparent pixels a palette index none of the others use, as the top left corner like a bmp.
pub fn shirt_from_rgba(
    width: usize,
    height: usize,
    rgba: &[u8],
    palette: &[u8; 768],
) -> Option<Pic8> {
    if rgba.len() != width * height * 4 {
        return None;
    }
    let upright = (149..=152).contains(&height) && width <= 101;
    let (w, h) = if upright {
        (height, width)
    } else {
        (width, height)
    };
    if !(149..=152).contains(&w) || h > 101 {
        return None;
    }
    // The bmp pixel (x, y) is at (y, w - 1 - x) of an upright picture
    let source = |x: usize, y: usize| {
        let (sx, sy) = if upright { (y, w - 1 - x) } else { (x, y) };
        let i = (sy * width + sx) * 4;
        &rgba[i..i + 4]
    };
    let mut nearest = std::collections::HashMap::new();
    let mut index = |c: &[u8]| {
        *nearest.entry((c[0], c[1], c[2])).or_insert_with(|| {
            (0..256)
                .min_by_key(|&i| {
                    let d = |k: usize| (palette[i * 3 + k] as i32 - c[k] as i32).pow(2);
                    d(0) + d(1) + d(2)
                })
                .unwrap() as u8
        })
    };
    let mut colors: Vec<Option<u8>> = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let c = source(x, y);
            colors.push((c[3] >= 128).then(|| index(c)));
        }
    }
    let mut used = [false; 256];
    for &c in colors.iter().flatten() {
        used[c as usize] = true;
    }
    let mut transparent = used.iter().position(|&u| !u)? as u8;
    // A picture without a transparent corner keeps it, like a bmp would
    if let Some(corner) = colors[0] {
        transparent = corner;
    }
    let mut pic = Pic8::new(w, h);
    pic.pixels = colors
        .into_iter()
        .map(|c| c.unwrap_or(transparent))
        .collect();
    Some(pic)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turns_an_upright_shirt_back_into_palette_colors() {
        let mut palette = [0u8; 768];
        palette[3..6].copy_from_slice(&[200, 0, 0]);
        palette[6..9].copy_from_slice(&[0, 0, 200]);
        // Upright 101 x 149: transparent, except red at its top left and blue next to its
        // bottom left
        let (width, height) = (101, 149);
        let mut rgba = vec![0u8; width * height * 4];
        rgba[..4].copy_from_slice(&[210, 5, 0, 255]);
        let blue = ((height - 1) * width + 1) * 4;
        rgba[blue..blue + 4].copy_from_slice(&[0, 0, 190, 255]);
        let pic = shirt_from_rgba(width, height, &rgba, &palette).unwrap();
        assert_eq!((pic.width, pic.height), (149, 101));
        // The upright top left is the bmp's top right, its bottom left the bmp's top left
        assert_eq!(pic.get(148, 0), 1);
        assert_eq!(pic.get(0, 1), 2);
        // Transparent pixels get an index the shirt doesn't use
        let transparent = pic.get(10, 10);
        assert!(transparent != 1 && transparent != 2);
        assert!(shirt_from_rgba(10, 10, &[0; 400], &palette).is_none());
    }

    fn pcx(width: u16, height: u16, body: &[u8]) -> Vec<u8> {
        let mut header = [0u8; 128];
        header[0] = 10;
        header[1] = 5;
        header[2] = 1;
        header[3] = 8;
        header[8..10].copy_from_slice(&(width - 1).to_le_bytes());
        header[10..12].copy_from_slice(&(height - 1).to_le_bytes());
        header[65] = 1;
        header[66..68].copy_from_slice(&width.next_multiple_of(2).to_le_bytes());
        let mut out = header.to_vec();
        out.extend_from_slice(body);
        out
    }

    #[test]
    fn decodes_pcx_runs() {
        // Row 0: run of 3 x 7, then a literal 2 (pad byte 0)
        // Row 1: literal 1, run of 3 x 0xc5 (needs escaping)
        let data = pcx(3, 2, &[0xc3, 7, 0xc1, 0, 1, 0xc3, 0xc5]);
        let pic = decode_pcx(&data).unwrap();
        assert_eq!(pic.row(0), &[7, 7, 7]);
        assert_eq!(pic.row(1), &[1, 0xc5, 0xc5]);
    }

    #[test]
    fn rejects_a_pcx_larger_than_its_data() {
        // 32768 x 32768 is the largest the signed header coordinates describe
        let data = pcx(32768, 32768, &[0xff, 0]);
        // Right after the header
        assert_eq!(decode_pcx(&data), Err(Error::Eof { at: 128 }));
    }

    #[test]
    fn resizes_like_the_game() {
        let pic = Pic8 {
            width: 4,
            height: 2,
            pixels: vec![1, 2, 3, 4, 5, 6, 7, 8],
        };
        let half = pic.resized(1);
        assert_eq!((half.width, half.height), (2, 1));
        assert_eq!(half.pixels, vec![1, 3]);
        let double = pic.resized(4);
        assert_eq!((double.width, double.height), (8, 4));
        assert_eq!(double.row(3), &[5, 5, 6, 6, 7, 7, 8, 8]);
    }

    #[test]
    fn decodes_bottom_up_bmp() {
        let mut data = Vec::new();
        data.extend(0x4d42u16.to_le_bytes());
        data.extend(0u32.to_le_bytes());
        data.extend(0u32.to_le_bytes());
        data.extend(54u32.to_le_bytes());
        data.extend(40u32.to_le_bytes());
        data.extend(3i32.to_le_bytes());
        data.extend(2i32.to_le_bytes());
        data.extend(1u16.to_le_bytes());
        data.extend(8u16.to_le_bytes());
        data.resize(54, 0);
        data.extend([4, 5, 6, 0, 1, 2, 3, 0]);
        let pic = decode_bmp(&data).unwrap();
        assert_eq!(pic.row(0), &[1, 2, 3]);
        assert_eq!(pic.row(1), &[4, 5, 6]);
    }
}
