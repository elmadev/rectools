// Which texture column each column of a view shows, and which texture row each row, for the
// ground and sky fills. The shader's fill_texel works it out for every pixel, though a column's
// texel depends only on the column and a row's only on the row; with these, fs_fill looks it
// up. They are worked out here exactly as fill_texel does on the GPU, in whole numbers and the
// same f32 products, for a texture the atlas has at its size in game pixels.

// a / b rounded down, for b > 0
fn div_floor(a: i32, b: i32) -> i32 {
    a.div_euclid(b)
}

// Where tile `k` of a texture `size` pixels long starts at `scale`, as the shader's tile_start
fn tile_start(k: i32, size: i32, scale: f32) -> i32 {
    ((k * size) as f32 * scale).floor() as i32
}

// The tile pixel `p` is in, as its number, start and length, as the shader's locked_tile
// finds it
fn locked_tile(p: i32, size: i32, scale: f32) -> (i32, i32, i32) {
    let mut k = (p as f64 / (size as f64 * scale as f64)).floor() as i32;
    while tile_start(k, size, scale) > p {
        k -= 1;
    }
    while tile_start(k + 1, size, scale) <= p {
        k += 1;
    }
    let start = tile_start(k, size, scale);
    (k, start, (tile_start(k + 1, size, scale) - start).max(1))
}

// Fills the rest of `out` with copies of its first `period` values, which repeat
fn repeat(out: &mut [u16], period: usize) {
    let mut filled = period.min(out.len());
    while filled < out.len() {
        let n = filled.min(out.len() - filled);
        out.copy_within(0..n, filled);
        filled += n;
    }
}

// Texels `n * texels / length` rounded down, for n from `n` on, a step at a time without
// dividing each time
struct Stepper {
    texel: i32,
    // n * texels - texel * length, in 0..length
    rest: i32,
    texels: i32,
    length: i32,
}

impl Stepper {
    fn new(n: i32, texels: i32, length: i32) -> Stepper {
        let product = n * texels;
        Stepper {
            texel: div_floor(product, length),
            rest: product.rem_euclid(length),
            texels,
            length,
        }
    }

    fn up(&mut self) {
        self.rest += self.texels;
        while self.rest >= self.length {
            self.rest -= self.length;
            self.texel += 1;
        }
    }

    fn down(&mut self) {
        self.rest -= self.texels;
        while self.rest < 0 {
            self.rest += self.length;
            self.texel -= 1;
        }
    }
}

// A texture `w` by `h` game pixels drawn at `scale`, tiled like the game (`game`) or with
// each tile where it is at zoom 1
#[derive(Debug, Clone, Copy)]
pub(crate) struct Tiling {
    pub w: i32,
    pub h: i32,
    pub scale: f32,
    pub game: bool,
}

impl Tiling {
    // A tile's height and width at this scale with game textures, as vs_fill works them out
    fn zoomed_tile(&self) -> (i32, i32) {
        let height = ((self.h as f32 * self.scale).floor() as i32).max(1);
        let width = div_floor(self.w * height, self.h.max(1)).max(1);
        (width, height)
    }

    // The texture column at pixel column `z`, counted from where the tiles start
    #[cfg(test)]
    fn column(&self, z: i32) -> u16 {
        let (w, h) = (self.w, self.h);
        let x = if self.scale == 1.0 {
            z.rem_euclid(w)
        } else if self.game {
            let (width, height) = self.zoomed_tile();
            div_floor(z.rem_euclid(width) * h, height).min(w - 1)
        } else {
            let (_, start, length) = locked_tile(z, w, self.scale);
            div_floor((z - start) * w, length).min(w - 1)
        };
        x as u16
    }

    // The texture row at pixel row `z`, from the bottom. Textures are stored bottom row
    // first, and the game resizes them before flipping them.
    #[cfg(test)]
    fn row(&self, z: i32) -> u16 {
        let h = self.h;
        let y = if self.scale == 1.0 {
            z.rem_euclid(h)
        } else if self.game {
            let (_, height) = self.zoomed_tile();
            let y = z.rem_euclid(height);
            h - 1 - div_floor((height - 1 - y) * h, height).min(h - 1)
        } else {
            let (_, start, length) = locked_tile(z, h, self.scale);
            h - 1 - div_floor((length - 1 - (z - start)) * h, length).min(h - 1)
        };
        y as u16
    }

    // `column` for pixel columns from `start` on, one for each of `out`
    pub fn columns(&self, start: i32, out: &mut [u16]) {
        let (w, h) = (self.w, self.h);
        if self.scale == 1.0 {
            let mut x = start.rem_euclid(w);
            for o in out {
                *o = x as u16;
                x = if x + 1 == w { 0 } else { x + 1 };
            }
        } else if self.game {
            // Every tile is the same, so the first is worked out and repeated
            let (width, height) = self.zoomed_tile();
            let mut x = start.rem_euclid(width);
            let mut texel = Stepper::new(x, h, height);
            let period = (width as usize).min(out.len());
            for o in &mut out[..period] {
                *o = texel.texel.min(w - 1) as u16;
                x += 1;
                if x == width {
                    x = 0;
                    texel = Stepper::new(0, h, height);
                } else {
                    texel.up();
                }
            }
            repeat(out, period);
        } else {
            let (mut k, mut tile, mut length) = locked_tile(start, w, self.scale);
            let mut x = start - tile;
            let mut texel = Stepper::new(x, w, length);
            for o in out {
                *o = texel.texel.min(w - 1) as u16;
                x += 1;
                if x == length {
                    k += 1;
                    tile = tile_start(k, w, self.scale);
                    length = (tile_start(k + 1, w, self.scale) - tile).max(1);
                    x = 0;
                    texel = Stepper::new(0, w, length);
                } else {
                    texel.up();
                }
            }
        }
    }

    // `row` for pixel rows from `start` on, one for each of `out`
    pub fn rows(&self, start: i32, out: &mut [u16]) {
        let h = self.h;
        if self.scale == 1.0 {
            let mut y = start.rem_euclid(h);
            for o in out {
                *o = y as u16;
                y = if y + 1 == h { 0 } else { y + 1 };
            }
        } else if self.game {
            let (_, height) = self.zoomed_tile();
            let mut y = start.rem_euclid(height);
            // The row flipped, counted down as y counts up
            let mut texel = Stepper::new(height - 1 - y, h, height);
            let period = (height as usize).min(out.len());
            for o in &mut out[..period] {
                *o = (h - 1 - texel.texel.min(h - 1)) as u16;
                y += 1;
                if y == height {
                    y = 0;
                    texel = Stepper::new(height - 1, h, height);
                } else {
                    texel.down();
                }
            }
            repeat(out, period);
        } else {
            let (mut k, mut tile, mut length) = locked_tile(start, h, self.scale);
            let y = start - tile;
            let mut texel = Stepper::new(length - 1 - y, h, length);
            let mut left = length - y;
            for o in out {
                *o = (h - 1 - texel.texel.min(h - 1)) as u16;
                left -= 1;
                if left == 0 {
                    k += 1;
                    tile = tile_start(k, h, self.scale);
                    length = (tile_start(k + 1, h, self.scale) - tile).max(1);
                    left = length;
                    texel = Stepper::new(length - 1, h, length);
                } else {
                    texel.down();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiling(scale: f32, game: bool) -> Tiling {
        Tiling {
            w: 64,
            h: 32,
            scale,
            game,
        }
    }

    #[test]
    fn at_zoom_1_a_texture_repeats() {
        let t = tiling(1.0, true);
        assert_eq!(t.column(0), 0);
        assert_eq!(t.column(63), 63);
        assert_eq!(t.column(64), 0);
        assert_eq!(t.column(-1), 63);
        assert_eq!(t.row(-1), 31);
    }

    #[test]
    fn at_zoom_2_each_texel_is_two_pixels() {
        for game in [true, false] {
            let t = tiling(2.0, game);
            let columns: Vec<u16> = (0..6).map(|z| t.column(z)).collect();
            assert_eq!(columns, [0, 0, 1, 1, 2, 2]);
            assert_eq!(t.column(128), 0);
            // Rows are flipped after resizing, which at a whole zoom is the same rows
            let rows: Vec<u16> = (0..4).map(|z| t.row(z)).collect();
            assert_eq!(rows, [0, 0, 1, 1]);
        }
    }

    #[test]
    fn stepping_through_a_row_finds_each_pixels_texel() {
        for (w, h) in [(64, 32), (64, 64), (48, 100), (1, 7)] {
            for scale in [0.37, 0.5, 0.75, 1.0, 1.25, 1.6, 2.25, 3.3, 7.0, 50.0] {
                for game in [true, false] {
                    let t = Tiling { w, h, scale, game };
                    for start in [-1237, -64, -1, 0, 1, 31, 999] {
                        let mut columns = [0; 700];
                        let mut rows = [0; 700];
                        t.columns(start, &mut columns);
                        t.rows(start, &mut rows);
                        for (i, (&c, &r)) in columns.iter().zip(&rows).enumerate() {
                            let z = start + i as i32;
                            assert_eq!(c, t.column(z), "column {z} of {w}x{h} at {scale}, {game}");
                            assert_eq!(r, t.row(z), "row {z} of {w}x{h} at {scale}, {game}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn every_pixel_shows_a_texel_of_the_texture() {
        for scale in [0.37, 0.75, 1.25, 2.25, 7.0] {
            for game in [true, false] {
                let t = tiling(scale, game);
                for z in -500..500 {
                    assert!(t.column(z) < 64 && t.row(z) < 32, "{scale} {game} {z}");
                }
            }
        }
    }
}
