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
}

// Which camera corner an instance moves with. The game draws the level at whole
// pixels, but the bikes at their exact position.
pub mod snap {
    pub const EXACT: u32 = 0;
    pub const PIXEL: u32 = 1;
}

// What the front grass pass needs to know about each pixel
pub mod layer {
    pub const OTHER: u32 = 0;
    pub const FRONT: u32 = 1;
    pub const QGRASS: u32 = 2;
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
        }
    }

    pub fn image(mut self, rect: AtlasRect, transparency: Option<u8>) -> Instance {
        self.image = rect.packed();
        self.transparency = transparency.map(u32::from).unwrap_or(NO_TRANSPARENCY);
        self
    }

    pub fn texture(mut self, rect: AtlasRect, game_size: (f64, f64)) -> Instance {
        self.texture = rect.packed();
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

// Lower distances are closer. The empty canvas is at depth 1.
pub fn distance_to_depth(distance: f64) -> f32 {
    (distance.clamp(0.0, 2000.0) / 2048.0) as f32
}

pub const BIKE_DISTANCE: f64 = 500.0;
