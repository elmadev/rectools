use crate::across::{self, AcrossSprites};
use crate::atlas::{AtlasBuilder, AtlasRect, Packer};
use crate::bike::{BikeDraw, BikeSprites, Sprite, draw_bike};
use crate::camera::bounds;
use crate::grass::{self, GRASS_DISTANCE, Piece, QGRASS_MARGIN, QUPDOWN_MARGIN};
use crate::instance::{
    BIKE_DISTANCE, CanvasRect, Instance, M2P, layer, mode, resized_size, snap_to_pixel_center,
};
use crate::minimap::Minimap;
use crate::plan::RenderError;
use crate::world::{Bike, DrawnBike, View, World};
use elma_formats::Graphics;
use elma_formats::lev::{Clipping, Gravity, Level, ObjectType};
use elma_formats::lgr::{ANIM_WIDTH, Anim, BikePics, Image, Lgr};
use elma_formats::pic::{Pic8, decode_bmp};
use replay::{LevelObject, Vec2};
use std::ops::Range;

const MARGIN_X: f64 = 10000.0 / M2P;
const MARGIN_Y: f64 = 1000.0 / M2P;
const CANVAS_SAFETY_RENDER: i32 = 20;
const CANVAS_SAFETY_LEFT: i32 = 120;
const CANVAS_SAFETY_RIGHT: i32 = 50;
const GRAVITY_ARROW_COLOR: u8 = 0x19;
const GRAVITY_ARROW_BMP: &[u8] = include_bytes!("../assets/gravarrow.bmp");
// The largest shirt, as elma_formats::pic::decode_shirt takes them
const SHIRT_CELL: (usize, usize) = (152, 101);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SceneOptions {
    pub default_ground: bool,
    pub default_sky: bool,
    // Draw pictures, and grass. The game skips both in low quality mode.
    pub pictures: bool,
    pub grass: bool,
    // The 256 single color pictures a fancyboosted LGR has, which rectools gives every LGR.
    // Off draws a level like an LGR without them.
    pub fancyboost: bool,
}

impl Default for SceneOptions {
    fn default() -> Self {
        SceneOptions {
            default_ground: false,
            default_sky: false,
            pictures: true,
            grass: true,
            fancyboost: true,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TextureRef {
    pub rect: AtlasRect,
    // In game pixels, which can be fractions for Across textures
    pub size: (f64, f64),
}

// Everything from the LGR that is drawn per frame
pub struct Sprites {
    pub bikes: [BikeSprites; 2],
    pub flag: Sprite,
    pub killer: Vec<AtlasRect>,
    pub killer_transparency: u8,
    pub exit: Vec<AtlasRect>,
    pub exit_transparency: u8,
    pub food: Vec<(Vec<AtlasRect>, u8)>,
    // Up, down, left, right, as they appear on screen
    // The picture of the down arrow, which the others are made from as they are drawn
    pub arrows: Option<(AtlasRect, u8)>,
    // By world::Bike::shirt, which is the session's riders followed by its ghosts
    pub shirts: Vec<Option<Sprite>>,
    // Room in the atlas taken for each shirt given after the scene was built, which the next
    // one given in its place goes in when it fits
    pub shirt_cells: Vec<Option<AtlasRect>>,
}

// A level and its graphics, ready for the GPU: what both games draw, and what only one does
// in `look`
pub struct Scene {
    pub atlas_size: usize,
    pub atlas_layers: Vec<Vec<u8>>,
    pub palette: [u8; 768],
    pub timer_map: [u8; 256],
    // Textures are aligned to it
    pub canvas_origin: Vec2,
    // Triangle fans of the solid polygons, for the even-odd sky test
    pub polygon_vertices: Vec<[f32; 2]>,
    // Covers the ground
    pub foreground: TextureRef,
    // Tiled behind the views, where they don't cover the screen
    pub qframe: TextureRef,
    pub minimap: Minimap,
    pub look: Look,
    // Where there is room left in the atlas
    pub(crate) packer: Packer,
    // Different for each scene made, so what a frame worked out for one isn't taken for
    // another's
    pub(crate) id: u64,
}

pub(crate) fn next_scene_id() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

// What the bikes, objects and sky are drawn with
pub enum Look {
    Elma(Box<ElmaLook>),
    Across(Box<AcrossSprites>),
}

pub struct ElmaLook {
    pub sprites: Sprites,
    // Covers the sky
    pub background: TextureRef,
    pub pictures: Pictures,
    // Bottom left of the level plus the margins, before snapping to pixels, which the game
    // snaps again at each zoom
    pub canvas_corner: Vec2,
    // The right edge of the level plus the margin, where the game's canvas ends
    pub canvas_right: f64,
}

// Instances of one of the pictures' groups, as many as CHUNK in a row, and the box in the level
// they are all in, y up: a view draws the chunks that reach into it and leaves out the rest
#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    pub range: Range<u32>,
    pub min: Vec2,
    pub max: Vec2,
}

const CHUNK: u32 = 64;

// `range` of `instances` in chunks
fn chunks(instances: &[Instance], range: Range<u32>) -> Vec<Chunk> {
    let mut out = Vec::new();
    let mut start = range.start;
    while start < range.end {
        let end = (start + CHUNK).min(range.end);
        let corners = instances[start as usize..end as usize]
            .iter()
            .flat_map(|i| {
                let o = Vec2::new(i.origin[0] as f64, i.origin[1] as f64);
                let u = Vec2::new(i.axis_u[0] as f64, i.axis_u[1] as f64);
                let v = Vec2::new(i.axis_v[0] as f64, i.axis_v[1] as f64);
                [o, o + u, o + v, o + u + v]
            });
        let (min, max) = bounds(corners).unwrap_or_default();
        out.push(Chunk {
            range: start..end,
            min,
            max,
        });
        start = end;
    }
    out
}

// The pictures and grass drawn with the level: those the ground clips, those the sky clips,
// and those nothing clips, as ranges of `instances`
#[derive(Default)]
pub struct Pictures {
    pub instances: Vec<Instance>,
    // By instance, where the game puts each picture and mask; the grass has none
    pub placements: Vec<Option<Placement>>,
    pub ground: Range<u32>,
    pub sky: Range<u32>,
    pub unclipped: Range<u32>,
    // Qgrass can move in front of the bikes: there is qgrass and a picture in front
    pub front_grass: bool,
    // The ground, sky and unclipped groups in chunks
    pub chunks: [Vec<Chunk>; 3],
}

impl ElmaLook {
    // The canvas origin the game snaps to the middle of a pixel at `ppm`
    pub(crate) fn canvas_origin(&self, ppm: f64) -> Vec2 {
        let quantize = |m: f64| snap_to_pixel_center(m, ppm);
        Vec2::new(
            quantize(self.canvas_corner.x),
            quantize(self.canvas_corner.y),
        )
    }

    // Where the game draws each of the pictures' instances at `zoom`, into `out`. Those it
    // leaves out for being too close to the canvas's sides are HIDDEN.
    pub(crate) fn picture_rects(&self, zoom: f64, out: &mut Vec<CanvasRect>) {
        let ppm = crate::camera::pixels_per_meter(zoom);
        let origin = self.canvas_origin(ppm);
        let pixel_width = ((self.canvas_right - origin.x) * ppm) as i32;
        out.extend(self.pictures.placements.iter().map(|p| {
            let Some(p) = p else {
                return CanvasRect::NONE;
            };
            let r = p.rect(origin, ppm, zoom);
            if r.left < CANVAS_SAFETY_LEFT || r.left + r.width >= pixel_width - CANVAS_SAFETY_RIGHT
            {
                CanvasRect::HIDDEN
            } else {
                r
            }
        }));
    }
}

// A level picture or mask as the game places it at any zoom: from the top left corner of the
// sprite, resized from the picture in the file
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    // The sprite's position, y up
    pub r: Vec2,
    // The picture in the file, which the atlas has as it is
    pub source: (usize, usize),
    // Its height at zoom 1
    pub height: i32,
}

impl Placement {
    // Where the game draws it on a canvas with its origin at `origin`, `ppm` pixels a meter
    pub fn rect(&self, origin: Vec2, ppm: f64, zoom: f64) -> CanvasRect {
        let left = ((self.r.x - origin.x) * ppm) as i32;
        let top = ((self.r.y - origin.y) * ppm) as i32;
        CanvasRect::from_top(left, top, self.source, (zoom * self.height as f64) as i32)
    }
}

impl Scene {
    // `shirts` is for Elma, by bike like the session's riders and then its ghosts
    pub fn new(
        graphics: &Graphics,
        level: &Level,
        shirts: &[Option<Pic8>],
        options: &SceneOptions,
        max_atlas_size: usize,
    ) -> Result<Scene, RenderError> {
        match graphics {
            Graphics::Elma(lgr) => Scene::elma(lgr, level, shirts, options, max_atlas_size),
            Graphics::Across(gfx, descriptor) => {
                crate::across::scene(gfx, level, *descriptor, max_atlas_size)
            }
        }
    }

    // The pictures and grass, which Across has none of
    pub fn pictures(&self) -> Option<&Pictures> {
        match &self.look {
            Look::Elma(elma) => Some(&elma.pictures),
            Look::Across(_) => None,
        }
    }

    // Where shirt `index` goes in the atlas once the scene is built: where the shirt given
    // before it went, or room left in the atlas. None when there is nothing to write, for no
    // shirt or Across, which has none.
    pub(crate) fn place_shirt(
        &mut self,
        index: usize,
        shirt: Option<&Pic8>,
    ) -> Result<Option<AtlasRect>, RenderError> {
        let Scene { look, packer, .. } = self;
        let Look::Elma(elma) = look else {
            return Ok(None);
        };
        let sprites = &mut elma.sprites;
        if sprites.shirts.len() <= index {
            sprites.shirts.resize(index + 1, None);
        }
        let Some(pic) = shirt else {
            sprites.shirts[index] = None;
            return Ok(None);
        };
        if sprites.shirt_cells.len() <= index {
            sprites.shirt_cells.resize(index + 1, None);
        }
        let (w, h) = (pic.width, pic.height);
        let fits = |cell: &AtlasRect| w <= cell.width as usize && h <= cell.height as usize;
        let cell = match sprites.shirt_cells[index].filter(fits) {
            Some(cell) => cell,
            // Room for any shirt, so the next one can take its place
            None => {
                let (cw, ch) = (w.max(SHIRT_CELL.0), h.max(SHIRT_CELL.1));
                let cell = packer.find_room(cw, ch).ok_or(RenderError::AtlasFull)?;
                sprites.shirt_cells[index] = Some(cell);
                cell
            }
        };
        let rect = AtlasRect {
            width: w as u32,
            height: h as u32,
            ..cell
        };
        sprites.shirts[index] = Some(Sprite {
            rect,
            transparency: pic.get(0, 0),
        });
        Ok(Some(rect))
    }
}

// The bounds of the level's ground, y pointing up like the bikes
pub(crate) fn ground_bounds(level: &Level) -> (Vec2, Vec2) {
    let solid = level.polygons.iter().filter(|p| !p.is_grass);
    let vertices = solid.flat_map(|p| &p.vertices);
    bounds(vertices.map(|v| Vec2::new(v.x, -v.y))).unwrap_or_default()
}

// The solid polygons as triangle fans, for the stencil's even-odd test
pub(crate) fn polygon_fans(level: &Level) -> Vec<[f32; 2]> {
    let mut out = Vec::new();
    for poly in level.polygons.iter().filter(|p| !p.is_grass) {
        let v: Vec<[f32; 2]> = poly
            .vertices
            .iter()
            .map(|v| [v.x as f32, -v.y as f32])
            .collect();
        for i in 1..v.len().saturating_sub(1) {
            out.extend([v[0], v[i], v[i + 1]]);
        }
    }
    out
}

impl Scene {
    fn elma(
        lgr: &Lgr,
        level: &Level,
        shirts: &[Option<Pic8>],
        options: &SceneOptions,
        max_atlas_size: usize,
    ) -> Result<Scene, RenderError> {
        // At the size they are drawn at, which can be bigger than the file's
        let largest = lgr
            .pictures
            .iter()
            .map(|p| &p.image)
            .chain(lgr.masks.iter().map(|m| &m.image))
            .chain(lgr.textures.iter().map(|t| &t.image))
            .map(|i| i.width.max(i.height).max(i.pic.width).max(i.pic.height))
            .max()
            .unwrap_or(0);
        let atlas_size = atlas_size(largest, max_atlas_size)?;
        let mut atlas = AtlasBuilder::new(atlas_size);

        // Pictures and masks are resized as they are drawn, from the file's, at any zoom
        let pictures: Vec<AtlasRect> = lgr
            .pictures
            .iter()
            .map(|p| atlas.add(&p.image.pic))
            .collect();
        let masks: Vec<AtlasRect> = lgr.masks.iter().map(|m| atlas.add(&m.image.pic)).collect();
        // Textures the LGR stores at another size are resized exactly like the game does at
        // zoom 1, so the same rows and columns repeat
        let mut add_image = |image: &Image| {
            if image.pic.height == image.height && image.pic.width == image.width {
                atlas.add(&image.pic)
            } else {
                atlas.add(&image.pic.resized(image.height as i32))
            }
        };
        // Textures are flipped after the game resizes them
        let textures: Vec<AtlasRect> = lgr
            .textures
            .iter()
            .map(|t| {
                let image = &t.image;
                if image.pic.height == image.height && image.pic.width == image.width {
                    add_image(image)
                } else {
                    let resized = image.pic.flipped_vertically().resized(image.height as i32);
                    add_image(&Image {
                        pic: resized.flipped_vertically(),
                        width: image.width,
                        height: image.height,
                    })
                }
            })
            .collect();
        let texture_ref = |i: usize| TextureRef {
            rect: textures[i],
            size: (
                lgr.textures[i].image.width as f64,
                lgr.textures[i].image.height as f64,
            ),
        };

        let sprites = Sprites {
            bikes: [
                add_bike(&mut atlas, &lgr.bikes[0]),
                add_bike(&mut atlas, &lgr.bikes[1]),
            ],
            flag: add_sprite(&mut atlas, &lgr.flag),
            killer: add_anim(&mut atlas, &lgr.killer),
            killer_transparency: lgr.killer.transparency,
            exit: add_anim(&mut atlas, &lgr.exit),
            exit_transparency: lgr.exit.transparency,
            food: lgr
                .food
                .iter()
                .map(|f| (add_anim(&mut atlas, f), f.transparency))
                .collect(),
            arrows: add_arrows(&mut atlas),
            shirts: shirts
                .iter()
                .map(|s| s.as_ref().map(|pic| add_sprite(&mut atlas, pic)))
                .collect(),
            shirt_cells: Vec::new(),
        };

        // Canvas origin at zoom 1: the level's bounding box plus a margin, snapped to the middle
        // of a pixel. Textures are aligned to it.
        let (min, max) = ground_bounds(level);
        let quantize = |m: f64| snap_to_pixel_center(m, M2P);
        let canvas_corner = Vec2::new(min.x - MARGIN_X, min.y - MARGIN_Y);
        let origin = Vec2::new(quantize(canvas_corner.x), quantize(canvas_corner.y));
        let pixel_width = (((max.x + MARGIN_X) - origin.x) * M2P) as i32;
        let pixel_height = (((max.y + MARGIN_Y) - origin.y) * M2P) as i32;
        let to_world = |x: f64, y: f64| [origin.x + x / M2P, origin.y + y / M2P];

        let qgrass = lgr.texture("qgrass");
        let mut instances = Vec::new();
        let mut placements = Vec::new();
        let sprite_instances =
            |clipping: Clipping, out: &mut Vec<Instance>, placed: &mut Vec<Option<Placement>>| {
                for spr in &level.sprites {
                    if spr.clipping != Some(clipping) {
                        continue;
                    }
                    let x = ((spr.r.x - origin.x) * M2P) as i32;
                    let y = ((-spr.r.y - origin.y) * M2P) as i32;
                    let distance = spr.distance as f64;
                    let front = if spr.distance <= 500 {
                        layer::FRONT
                    } else {
                        layer::OTHER
                    };
                    let (instance, image) = if spr.picture.is_empty() {
                        if spr.texture.is_empty() || spr.mask.is_empty() {
                            continue;
                        }
                        let (Some(t), Some(m)) = (lgr.texture(&spr.texture), lgr.mask(&spr.mask))
                        else {
                            continue;
                        };
                        let mask = &lgr.masks[m];
                        let (w, h) = (mask.image.width, mask.image.height);
                        let is_qgrass = Some(t) == qgrass;
                        let layer = match (is_qgrass, spr.distance > 500) {
                            (true, true) => layer::QGRASS,
                            (true, false) => layer::OTHER,
                            _ => front,
                        };
                        let instance = rect_instance(to_world(x as f64, (y + 1) as f64), w, h)
                            .image(masks[m], mask.transparency)
                            .texture(texture_ref(t).rect, texture_ref(t).size)
                            .mode(mode::MASKED_TEXTURE)
                            .layer(layer);
                        (instance, &mask.image)
                    } else {
                        if !spr.texture.is_empty() || !spr.mask.is_empty() {
                            continue;
                        }
                        let Some(p) = lgr.picture(&spr.picture) else {
                            continue;
                        };
                        let picture = &lgr.pictures[p];
                        if picture.solid.is_some() && !options.fancyboost {
                            continue;
                        }
                        let (w, h) = (picture.image.width, picture.image.height);
                        let mut instance =
                            rect_instance(to_world(x as f64, (y + 1) as f64), w, h).layer(front);
                        instance = match picture.solid {
                            Some(color) => instance.mode(mode::SOLID).color(color),
                            None => instance.image(pictures[p], Some(picture.transparency)),
                        };
                        (instance, &picture.image)
                    };
                    // Which are too close to the canvas's sides depends on the zoom
                    placed.push(Some(Placement {
                        r: Vec2::new(spr.r.x, -spr.r.y),
                        source: (image.pic.width, image.pic.height),
                        height: image.height as i32,
                    }));
                    let over_bikes = if distance < BIKE_DISTANCE {
                        layer::OVER_BIKES
                    } else {
                        0
                    };
                    let layer = instance.layer | over_bikes;
                    out.push(instance.distance(distance).layer(layer));
                }
            };

        let ground_start = instances.len() as u32;
        if options.pictures {
            sprite_instances(Clipping::Ground, &mut instances, &mut placements);
        }
        if options.grass
            && lgr.has_grass()
            && let Some(qgrass) = qgrass
        {
            grass_instances(
                lgr,
                level,
                &mut atlas,
                texture_ref(qgrass),
                origin,
                (pixel_width, pixel_height),
                &mut instances,
            );
        }
        // The grass is placed at zoom 1
        placements.resize(instances.len(), None);
        let sky_start = instances.len() as u32;
        if options.pictures {
            sprite_instances(Clipping::Sky, &mut instances, &mut placements);
        }
        let unclipped_start = instances.len() as u32;
        if options.pictures {
            sprite_instances(Clipping::Unclipped, &mut instances, &mut placements);
        }
        let end = instances.len() as u32;
        let front_grass = [layer::QGRASS, layer::FRONT]
            .iter()
            .all(|&l| instances.iter().any(|i| i.layer & layer::KIND == l));

        let qframe = TextureRef {
            rect: atlas.add(&lgr.qframe),
            size: (lgr.qframe.width as f64, lgr.qframe.height as f64),
        };
        let (fg, bg) = lgr.default_textures(
            &level.foreground,
            &level.background,
            options.default_ground,
            options.default_sky,
        );

        let atlas_size = atlas.layer_size();
        let (atlas_layers, packer) = atlas.finish();
        Ok(Scene {
            id: next_scene_id(),
            atlas_size,
            atlas_layers,
            palette: lgr.palette,
            timer_map: lgr.timer_palette_map(),
            canvas_origin: origin,
            polygon_vertices: polygon_fans(level),
            foreground: texture_ref(fg),
            qframe,
            minimap: Minimap::elma(&lgr.minimap),
            look: Look::Elma(Box::new(ElmaLook {
                sprites,
                background: texture_ref(bg),
                pictures: Pictures {
                    chunks: [
                        chunks(&instances, ground_start..sky_start),
                        chunks(&instances, sky_start..unclipped_start),
                        chunks(&instances, unclipped_start..end),
                    ],
                    instances,
                    placements,
                    ground: ground_start..sky_start,
                    sky: sky_start..unclipped_start,
                    unclipped: unclipped_start..end,
                    front_grass,
                },
                canvas_corner,
                canvas_right: max.x + MARGIN_X,
            })),
            packer,
        })
    }
}

// The size of the atlas's layers, which the largest image has to fit in
pub(crate) fn atlas_size(largest: usize, max_atlas_size: usize) -> Result<usize, RenderError> {
    let limit = max_atlas_size.max(2048);
    if largest > limit {
        return Err(RenderError::ImageTooLarge {
            size: largest,
            max: limit,
        });
    }
    Ok(largest.next_power_of_two().clamp(2048, limit))
}

// An axis aligned picture whose top left corner is at `top_left`, `w` by `h` game pixels
fn rect_instance(top_left: [f64; 2], w: usize, h: usize) -> Instance {
    Instance::new(top_left, [w as f64 / M2P, 0.0], [0.0, -(h as f64) / M2P])
}

fn add_sprite(atlas: &mut AtlasBuilder, pic: &Pic8) -> Sprite {
    Sprite {
        rect: atlas.add(pic),
        transparency: pic.get(0, 0),
    }
}

fn add_bike(atlas: &mut AtlasBuilder, bike: &BikePics) -> BikeSprites {
    BikeSprites {
        parts: [
            add_sprite(atlas, &bike.parts[0]),
            add_sprite(atlas, &bike.parts[1]),
            add_sprite(atlas, &bike.parts[2]),
            add_sprite(atlas, &bike.parts[3]),
        ],
        body: add_sprite(atlas, &bike.body),
        thigh: add_sprite(atlas, &bike.thigh),
        leg: add_sprite(atlas, &bike.leg),
        wheel: add_sprite(atlas, &bike.wheel),
        susp1: add_sprite(atlas, &bike.susp1),
        susp2: add_sprite(atlas, &bike.susp2),
        forarm: add_sprite(atlas, &bike.forarm),
        up_arm: add_sprite(atlas, &bike.up_arm),
        head: add_sprite(atlas, &bike.head),
    }
}

fn add_anim(atlas: &mut AtlasBuilder, anim: &Anim) -> Vec<AtlasRect> {
    anim.frames.iter().map(|f| atlas.add(f)).collect()
}

// The bitmap points down
fn add_arrows(atlas: &mut AtlasBuilder) -> Option<(AtlasRect, u8)> {
    let bmp = decode_bmp(GRAVITY_ARROW_BMP).ok()?;
    let transparency = bmp.get(0, 0);
    Some((atlas.add(&bmp), transparency))
}

#[allow(clippy::too_many_arguments)]
fn grass_instances(
    lgr: &Lgr,
    level: &Level,
    atlas: &mut AtlasBuilder,
    qgrass: TextureRef,
    origin: Vec2,
    (pixel_width, pixel_height): (i32, i32),
    out: &mut Vec<Instance>,
) {
    let pieces: Vec<Piece> = lgr
        .grass
        .iter()
        .map(|g| Piece::new(&g.image.pic, g.is_up, g.target_height))
        .collect();
    let rects: Vec<(AtlasRect, Option<AtlasRect>)> = pieces
        .iter()
        .map(|p| (atlas.add(&p.pic), p.mask.as_ref().map(|m| atlas.add(m))))
        .collect();
    let qgrass_margin = QGRASS_MARGIN - QUPDOWN_MARGIN;
    let to_world = |x: i32, y: i32| [origin.x + x as f64 / M2P, origin.y + y as f64 / M2P];

    for poly in level.polygons.iter().filter(|p| p.is_grass) {
        let Some((x0, heights)) = grass::heightmap(poly, origin) else {
            continue;
        };
        for placement in grass::place(&pieces, x0, &heights) {
            let piece = &pieces[placement.piece];
            let (rect, mask_rect) = rects[placement.piece];
            let (w, h) = (piece.pic.width as i32, piece.pic.height as i32);
            let (x, y) = (placement.x, placement.y);
            if x < CANVAS_SAFETY_LEFT
                || x + w >= pixel_width - CANVAS_SAFETY_RIGHT
                || y < CANVAS_SAFETY_RENDER
                || y + h + qgrass_margin >= pixel_height - CANVAS_SAFETY_RENDER
            {
                continue;
            }
            let top = y + h;
            out.push(
                rect_instance(to_world(x, top), w as usize, h as usize)
                    .image(rect, Some(piece.pic.get(0, 0)))
                    .distance(GRASS_DISTANCE),
            );
            if let (Some(mask), Some(mask_rect)) = (&piece.mask, mask_rect) {
                out.push(
                    rect_instance(to_world(x, top), mask.width, mask.height)
                        .image(mask_rect, Some(0))
                        .texture(qgrass.rect, qgrass.size)
                        .mode(mode::MASKED_TEXTURE)
                        .layer(layer::QGRASS)
                        .distance(GRASS_DISTANCE),
                );
                out.push(
                    rect_instance(
                        to_world(x, top + qgrass_margin),
                        w as usize,
                        qgrass_margin as usize,
                    )
                    .texture(qgrass.rect, qgrass.size)
                    .mode(mode::TEXTURE)
                    .layer(layer::QGRASS)
                    .distance(GRASS_DISTANCE),
                );
            }
        }
    }
}

// Bike pictures and flags of one view, after the game's choice of which bike is which
#[derive(Debug, Clone, Copy)]
pub struct DynamicOptions {
    // Apples, killers and the exit turn through their frames
    pub animated_objects: bool,
    // Apples and the exit don't bob up and down
    pub still_objects: bool,
    pub gravity_arrows: bool,
    // Draw objects and bikes over every picture
    pub pictures_in_background: bool,
}

impl Default for DynamicOptions {
    fn default() -> Self {
        DynamicOptions {
            animated_objects: true,
            still_objects: false,
            gravity_arrows: true,
            pictures_in_background: false,
        }
    }
}

// Objects and bikes of `view`: the objects that are there in it, then its bikes in order.
// Returns where the translucent ones are in `out`.
// The objects are where the game puts them at the view's zoom, by their index in `out` in
// `placed`.
pub(crate) fn dynamic_instances(
    scene: &Scene,
    world: &World,
    view: &View,
    options: &DynamicOptions,
    out: &mut Vec<Instance>,
    placed: &mut Vec<(usize, CanvasRect)>,
) -> std::ops::Range<u32> {
    let time = world.time;
    let active = |i: usize, object: &LevelObject| view.object_there(i, object);
    let (sprites, elma) = match &scene.look {
        Look::Elma(elma) => (&elma.sprites, elma),
        Look::Across(sprites) => {
            across::object_instances(
                sprites,
                world.objects,
                active,
                time,
                options.animated_objects,
                out,
            );
            return bike_instances(world, view, out, |bike, _, out| {
                across::draw_bike(&bike.pose, &bike.animation, &sprites.bike, out);
            });
        }
    };
    let zoom = view.zoom;
    let origin = scene.canvas_origin;
    // The game's canvas at this zoom, which it puts the objects on in whole pixels
    let ppm = crate::camera::pixels_per_meter(zoom);
    let canvas = elma.canvas_origin(ppm);
    let object_size = (ANIM_WIDTH as f64 * zoom) as i32;
    let size = ANIM_WIDTH as f64 / M2P;

    for (i, object) in world.objects.iter().enumerate() {
        let (frames, transparency) = match object.kind {
            ObjectType::Start => continue,
            ObjectType::Food if !active(i, object) => continue,
            ObjectType::Food => {
                let (frames, t) = &sprites.food[object.animation as usize % sprites.food.len()];
                (frames, *t)
            }
            ObjectType::Exit if world.hide_exit => continue,
            ObjectType::Exit => (&sprites.exit, sprites.exit_transparency),
            ObjectType::Killer => (&sprites.killer, sprites.killer_transparency),
        };
        let floats = matches!(object.kind, ObjectType::Food | ObjectType::Exit);
        let frame = if options.animated_objects {
            ((time / 0.014) as i64).rem_euclid(frames.len() as i64) as usize
        } else {
            0
        };
        let phase = (time * 15.5 + object.floating_phase).sin();
        let bob = if floats && !options.still_objects {
            5.0 * phase / M2P
        } else {
            0.0
        };
        let bob_pixels = if floats && !options.still_objects {
            (5.0 * zoom * phase) as i32
        } else {
            0
        };
        let offset = ANIM_WIDTH as f64 / 2.0 * zoom;
        let canvas_x = ((object.r.x - canvas.x) * ppm - offset) as i32;
        let canvas_y = ((object.r.y - canvas.y) * ppm - offset) as i32 + bob_pixels;
        let source = |r: AtlasRect| (r.width as usize, r.height as usize);
        let x = ((object.r.x - origin.x) * M2P - ANIM_WIDTH as f64 / 2.0) as i32;
        let y = ((object.r.y - origin.y) * M2P - ANIM_WIDTH as f64 / 2.0) as i32;
        let left = origin.x + x as f64 / M2P;
        let bottom = origin.y + y as f64 / M2P + bob;
        let quad =
            Instance::new([left, bottom + size], [size, 0.0], [0.0, -size]).distance(BIKE_DISTANCE);
        placed.push((
            out.len(),
            CanvasRect::from_bottom(canvas_x, canvas_y, source(frames[frame]), object_size),
        ));
        out.push(quad.image(frames[frame], Some(transparency)));

        if options.gravity_arrows
            && object.kind == ObjectType::Food
            && let Some((arrow, transparency)) = sprites.arrows
        {
            // The game resizes the down arrow, then flips it up and turns those left and right
            use crate::instance::orientation::{FLIP_COLUMNS, FLIP_ROWS, TRANSPOSE};
            let turned = match object.gravity {
                Gravity::Up => FLIP_ROWS,
                Gravity::Down => 0,
                Gravity::Left => FLIP_ROWS | FLIP_COLUMNS | TRANSPOSE,
                Gravity::Right => FLIP_ROWS | TRANSPOSE,
                Gravity::None => continue,
            };
            // Resized like the objects, in the middle of the object
            let half = object_size / 2;
            let (w, h) = resized_size(source(arrow), object_size);
            placed.push((
                out.len(),
                CanvasRect {
                    orientation: turned,
                    ..CanvasRect::from_bottom(
                        canvas_x + half - w / 2,
                        canvas_y + half - h / 2,
                        source(arrow),
                        object_size,
                    )
                },
            ));
            out.push(
                quad.image(arrow, Some(transparency))
                    .mode(mode::RECOLOR)
                    .color(GRAVITY_ARROW_COLOR),
            );
        }
    }

    let shirt = |index: Option<usize>| index.and_then(|i| sprites.shirts.get(i).copied().flatten());
    bike_instances(world, view, out, |bike, drawn, out| {
        draw_bike(
            &BikeDraw {
                pose: &bike.pose,
                animation: &bike.animation,
                sprites: &sprites.bikes[drawn.pictures.min(1)],
                shirt: shirt(bike.shirt),
                flag: drawn.flag.then_some(sprites.flag),
            },
            out,
        );
    })
}

// The view's bikes with `draw`, in order, returning where its translucent ones are in `out`
fn bike_instances(
    world: &World,
    view: &View,
    out: &mut Vec<Instance>,
    draw: impl Fn(&Bike, &DrawnBike, &mut Vec<Instance>),
) -> std::ops::Range<u32> {
    let count = view.bikes.len();
    let start = view.translucent.start.min(count);
    let end = view.translucent.end.clamp(start, count);
    let draw_all = |drawn: &[DrawnBike], out: &mut Vec<Instance>| {
        for d in drawn {
            if let Some(bike) = world.bikes.get(d.bike) {
                draw(bike, d, out);
            }
        }
        out.len() as u32
    };
    let translucent_start = draw_all(&view.bikes[..start], out);
    let translucent_end = draw_all(&view.bikes[start..end], out);
    draw_all(&view.bikes[end..], out);
    translucent_start..translucent_end
}
