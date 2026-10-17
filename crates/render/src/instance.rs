use crate::atlas::AtlasRect;
use bytemuck::{Pod, Zeroable};

pub mod mode {
    // Sample `image`, skipping the transparent index
    pub const IMAGE: u32 = 0;
    // `image` is a mask, the pixels come from `texture` at their world position
    pub const MASKED_TEXTURE: u32 = 1;
    // A rectangle of `color`
    pub const SOLID: u32 = 2;
    // The opaque pixels of `image` in `color`
    pub const RECOLOR: u32 = 3;
    // `texture` at the world position, aligned to the canvas origin
    pub const TEXTURE: u32 = 4;
    // `texture` anchored to the bottom of the view, scrolling at half speed
    pub const SKY: u32 = 5;
    // Like IMAGE, for the bike's pictures, sampled at each pixel's corner as the game does
    pub const AFFINE: u32 = 6;
}

// Which camera corner an instance moves with. The game draws the level at whole
// pixels, but the bikes at their exact position.
pub mod snap {
    pub const EXACT: u32 = 0;
    pub const PIXEL: u32 = 1;
}

// What the bikes need to know about the level at each pixel: what kind of picture is in
// front, and whether it hides them
pub mod layer {
    pub const OTHER: u32 = 0;
    pub const FRONT: u32 = 1;
    pub const QGRASS: u32 = 2;
    // The kind, without OVER_BIKES
    pub const KIND: u32 = 3;
    // Added to a picture closer than the bikes, which go behind it
    pub const OVER_BIKES: u32 = 4;
}

pub const NO_TRANSPARENCY: u32 = 256;

// Game pixels per meter at zoom 1
pub const M2P: f64 = 48.0;

// Like the game, snap `meters` to the middle of a pixel at `ppm` pixels per meter
pub fn snap_to_pixel_center(meters: f64, ppm: f64) -> f64 {
    ((meters * ppm) as i64 as f64 + 0.5) / ppm
}

// One textured parallelogram in world space: `origin` is the top left corner of the
// image, `axis_u` runs along its rows and `axis_v` down its columns.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Instance {
    pub origin: [f32; 2],
    pub axis_u: [f32; 2],
    pub axis_v: [f32; 2],
    pub depth: f32,
    pub snap: u32,
    pub image: [u32; 4],
    pub texture: [u32; 4],
    // Size of `texture` in game pixels, which can differ from its stored size
    pub texture_size: [f32; 2],
    pub mode: u32,
    pub transparency: u32,
    pub color: u32,
    pub layer: u32,
    // The atlas layers of `image` and `texture`: image | texture << 8
    pub atlas_layers: u32,
}

impl Instance {
    pub fn new(origin: [f64; 2], axis_u: [f64; 2], axis_v: [f64; 2]) -> Instance {
        Instance {
            origin: [origin[0] as f32, origin[1] as f32],
            axis_u: [axis_u[0] as f32, axis_u[1] as f32],
            axis_v: [axis_v[0] as f32, axis_v[1] as f32],
            depth: 0.0,
            snap: snap::PIXEL,
            image: [0; 4],
            texture: [0; 4],
            texture_size: [1.0, 1.0],
            mode: mode::IMAGE,
            transparency: NO_TRANSPARENCY,
            color: 0,
            layer: layer::OTHER,
            atlas_layers: 0,
        }
    }

    pub fn image(mut self, rect: AtlasRect, transparency: Option<u8>) -> Instance {
        self.image = rect.packed();
        self.atlas_layers = (self.atlas_layers & !0xff) | rect.layer;
        self.transparency = transparency.map(u32::from).unwrap_or(NO_TRANSPARENCY);
        self
    }

    pub fn texture(mut self, rect: AtlasRect, game_size: (f64, f64)) -> Instance {
        self.texture = rect.packed();
        self.atlas_layers = (self.atlas_layers & 0xff) | rect.layer << 8;
        self.texture_size = [game_size.0 as f32, game_size.1 as f32];
        self
    }

    pub fn mode(mut self, mode: u32) -> Instance {
        self.mode = mode;
        self
    }

    pub fn color(mut self, color: u8) -> Instance {
        self.color = color as u32;
        self
    }

    pub fn distance(mut self, distance: f64) -> Instance {
        self.depth = distance_to_depth(distance);
        self
    }

    pub fn snap(mut self, snap: u32) -> Instance {
        self.snap = snap;
        self
    }

    pub fn layer(mut self, layer: u32) -> Instance {
        self.layer = layer;
        self
    }
}

// Where the game puts a picture at the view's zoom, in whole pixels of the canvas from its
// origin, y up: the left column, the top row and the size it resizes the picture to. A level
// picture's or object's at the zoom, and drawn there instead of its quad; others have
// height 0 and are drawn as their quad says.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Pod, Zeroable)]
pub struct CanvasRect {
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
    // See resize_rounding
    pub rounding: f32,
    // Flipped or turned after resizing, see the orientation module
    pub orientation: u32,
}

// How a placed picture is flipped or turned after it is resized, as the game makes the
// gravity arrows from one picture: pixel (row, column) of the rect from its top left is
// mirrored by FLIP_ROWS and FLIP_COLUMNS, then the two are swapped by TRANSPOSE, and it shows
// that pixel of the resized picture
pub mod orientation {
    pub const FLIP_ROWS: u32 = 1;
    pub const FLIP_COLUMNS: u32 = 2;
    pub const TRANSPOSE: u32 = 4;
}

impl CanvasRect {
    // Drawn as its quad says
    pub const NONE: CanvasRect = CanvasRect {
        left: 0,
        top: 0,
        width: 0,
        height: 0,
        rounding: 0.0,
        orientation: 0,
    };
    // Left out at this zoom, as the game leaves out pictures too close to the canvas's sides
    pub const HIDDEN: CanvasRect = CanvasRect {
        left: 0,
        top: 0,
        width: 0,
        height: -1,
        rounding: 0.0,
        orientation: 0,
    };

    // A picture `source` pixels big, which the game resizes to `target_height` rows like
    // pic8::resize does, with its top left pixel at (left, top)
    pub fn from_top(left: i32, top: i32, source: (usize, usize), target_height: i32) -> CanvasRect {
        let (width, height) = resized_size(source, target_height);
        CanvasRect {
            left,
            top,
            width,
            height,
            rounding: resize_rounding(source.1, height),
            orientation: 0,
        }
    }

    // Like from_top, with its bottom left pixel at (left, bottom)
    pub fn from_bottom(
        left: i32,
        bottom: i32,
        source: (usize, usize),
        target_height: i32,
    ) -> CanvasRect {
        let rect = CanvasRect::from_top(left, 0, source, target_height);
        CanvasRect {
            top: bottom + rect.height - 1,
            ..rect
        }
    }
}

// The size pic8::resize gives a picture of `source` pixels resized to `target_height` rows
pub fn resized_size((width, height): (usize, usize), target_height: i32) -> (i32, i32) {
    let target_height = target_height.max(1);
    if target_height as usize == height {
        return (width as i32, target_height);
    }
    let scale = target_height as f64 / height as f64;
    (((width as f64 * scale) as i32).max(1), target_height)
}

// pic8::resize takes pixel i of a picture resized from `height` rows to `resized` from
// (int)(i / scale), with scale = resized / height in doubles. That is i * height / resized
// rounded down, but where that comes out a whole number k, the doubles can come out just under
// it and give k - 1. They all do by the same ratio, the scale's rounding error d, and k - 1
// is what the doubles give when k * d is over half the gap to the double below k. This is
// d * 2^53, which the shader compares with k: 0 when the scale rounds down and they never do.
pub fn resize_rounding(height: usize, resized: i32) -> f32 {
    let scale = resized as f64 / height as f64;
    // Exact, as it is a small multiple of the scale's last bit
    let error = scale.mul_add(height as f64, -(resized as f64));
    if error <= 0.0 {
        return 0.0;
    }
    (error / (scale * height as f64) * 2f64.powi(53)) as f32
}

// Lower distances are closer. The empty canvas is at depth 1.
pub fn distance_to_depth(distance: f64) -> f32 {
    (distance.clamp(0.0, 2000.0) / 2048.0) as f32
}

pub const BIKE_DISTANCE: f64 = 500.0;

#[cfg(test)]
mod tests {
    use super::*;

    // The shader's resize_source, which resize_rounding is for
    fn resize_source(i: i32, height: i32, resized: i32, rounding: f32) -> i32 {
        let k = (i * height).div_euclid(resized);
        if rounding == 0.0 || k <= 0 || k * resized != i * height {
            return k;
        }
        let e = 1 << (31 - k.leading_zeros());
        let below = if e == k {
            rounding > 0.5
        } else {
            k as f32 * rounding > e as f32
        };
        k - below as i32
    }

    #[test]
    fn resized_pixels_come_from_where_the_games_doubles_take_them() {
        let mut rounded_down = 0;
        for height in 1..300usize {
            for zoom in (25..400).map(|z| z as f64 / 100.0) {
                let resized = ((zoom * height as f64) as i32).max(1);
                let scale = resized as f64 / height as f64;
                let rounding = resize_rounding(height, resized);
                for i in 0..resized * 2 {
                    let game = (i as f64 / scale) as i32;
                    assert_eq!(
                        resize_source(i, height as i32, resized, rounding),
                        game,
                        "{i} of {height} resized to {resized}"
                    );
                    rounded_down += (game * resized < i * height as i32) as usize
                        * (((game + 1) * resized) == i * height as i32) as usize;
                }
            }
        }
        // The case it is for comes up
        assert!(rounded_down > 0);
    }
}
