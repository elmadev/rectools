use crate::across::{self, AcrossSprites};
use crate::atlas::{AtlasBuilder, AtlasRect};
use crate::bike::{BikeDraw, BikeSprites, Sprite, draw_bike};
use crate::camera::bounds;
use crate::grass::{self, GRASS_DISTANCE, Piece, QGRASS_MARGIN, QUPDOWN_MARGIN};
use crate::instance::{BIKE_DISTANCE, Instance, M2P, layer, mode, snap_to_pixel_center};
use crate::minimap::Minimap;
use crate::plan::RenderError;
use elma_formats::Graphics;
use elma_formats::lev::{Clipping, Gravity, Level, ObjectType};
use elma_formats::lgr::{ANIM_WIDTH, Anim, BikePics, Image, Lgr};
use elma_formats::pic::{Pic8, decode_bmp};
use replay::{LevelObject, Session, Vec2};
use std::ops::Range;

const MARGIN_X: f64 = 10000.0 / M2P;
const MARGIN_Y: f64 = 1000.0 / M2P;
const CANVAS_SAFETY_RENDER: i32 = 20;
const CANVAS_SAFETY_LEFT: i32 = 120;
const CANVAS_SAFETY_RIGHT: i32 = 50;
const GRAVITY_ARROW_COLOR: u8 = 0x19;
const GRAVITY_ARROW_BMP: &[u8] = include_bytes!("../assets/gravarrow.bmp");

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
    pub arrows: Option<([AtlasRect; 4], u8)>,
    // Indexed like the session's riders followed by its ghosts
    pub shirts: Vec<Option<Sprite>>,
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
}

// The pictures and grass drawn with the level: those the ground clips, those the sky clips,
// and those nothing clips, as ranges of `instances`
#[derive(Default)]
pub struct Pictures {
    pub instances: Vec<Instance>,
    pub ground: Range<u32>,
    pub sky: Range<u32>,
    pub unclipped: Range<u32>,
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

        // Images the LGR stores at another size are resized exactly like the game does at
        // zoom 1, so the same rows and columns repeat
        let mut add_image = |image: &Image| {
            if image.pic.height == image.height && image.pic.width == image.width {
                atlas.add(&image.pic)
            } else {
                atlas.add(&image.pic.resized(image.height as i32))
            }
        };
        let pictures: Vec<AtlasRect> = lgr.pictures.iter().map(|p| add_image(&p.image)).collect();
        let masks: Vec<AtlasRect> = lgr.masks.iter().map(|m| add_image(&m.image)).collect();
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
        let sprite_instances = |clipping: Clipping, out: &mut Vec<Instance>| {
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
                let (instance, width) = if spr.picture.is_empty() {
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
                    (instance, w)
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
                    (instance, w)
                };
                if x < CANVAS_SAFETY_LEFT || x + width as i32 >= pixel_width - CANVAS_SAFETY_RIGHT {
                    continue;
                }
                out.push(instance.distance(distance));
            }
        };

        let ground_start = instances.len() as u32;
        if options.pictures {
            sprite_instances(Clipping::Ground, &mut instances);
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
        let sky_start = instances.len() as u32;
        if options.pictures {
            sprite_instances(Clipping::Sky, &mut instances);
        }
        let unclipped_start = instances.len() as u32;
        if options.pictures {
            sprite_instances(Clipping::Unclipped, &mut instances);
        }
        let end = instances.len() as u32;

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

        Ok(Scene {
            atlas_size: atlas.layer_size(),
            atlas_layers: atlas.into_layers(),
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
                    instances,
                    ground: ground_start..sky_start,
                    sky: sky_start..unclipped_start,
                    unclipped: unclipped_start..end,
                },
                canvas_corner,
            })),
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

fn add_arrows(atlas: &mut AtlasBuilder) -> Option<([AtlasRect; 4], u8)> {
    let bmp = decode_bmp(GRAVITY_ARROW_BMP).ok()?;
    let transparency = bmp.get(0, 0);
    // The bitmap points down
    let up = bmp.flipped_vertically();
    let left = up.transposed().flipped_vertically();
    let right = bmp.transposed().flipped_vertically();
    Some((
        [
            atlas.add(&up),
            atlas.add(&bmp),
            atlas.add(&left),
            atlas.add(&right),
        ],
        transparency,
    ))
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
    // The game time objects animate by, when it isn't the replay's: viewers keep it running
    // while paused and for levels on their own
    pub object_clock: Option<f64>,
    pub gravity_arrows: bool,
    // Draw objects and bikes over every picture
    pub pictures_in_background: bool,
}

impl Default for DynamicOptions {
    fn default() -> Self {
        DynamicOptions {
            animated_objects: true,
            still_objects: false,
            object_clock: None,
            gravity_arrows: true,
            pictures_in_background: false,
        }
    }
}

// Objects and bikes of the view following main rider `follow`, or ghost `ghost` when it
// follows one: then the apples it has taken are gone instead of the main riders', and it is
// drawn over every other bike. Returns where the other ghosts are in `out`.
pub fn dynamic_instances(
    scene: &Scene,
    session: &Session,
    follow: usize,
    ghost: Option<usize>,
    options: &DynamicOptions,
    out: &mut Vec<Instance>,
) -> std::ops::Range<u32> {
    let time = session.time;
    let taken_by_ghost = ghost.and_then(|g| session.ghosts.get(g)).map(|g| {
        let mut taken = vec![false; session.objects.len()];
        for i in g.touched(time) {
            if let Some(t) = taken.get_mut(i) {
                *t = true;
            }
        }
        taken
    });
    let active = |i: usize, object: &LevelObject| match &taken_by_ghost {
        Some(taken) => !taken[i],
        None => object.active,
    };
    let sprites = match &scene.look {
        Look::Elma(elma) => &elma.sprites,
        Look::Across(sprites) => {
            return across_instances(sprites, session, follow, ghost, options, &active, out);
        }
    };
    let origin = scene.canvas_origin;
    let multiplayer = session.riders.len() > 1;
    let hide_exit = multiplayer && session.flagtag;
    let size = ANIM_WIDTH as f64 / M2P;

    for (i, object) in session.objects.iter().enumerate() {
        let (frames, transparency) = match object.kind {
            ObjectType::Start => continue,
            ObjectType::Food if !active(i, object) => continue,
            ObjectType::Food => {
                let (frames, t) = &sprites.food[object.animation as usize % sprites.food.len()];
                (frames, *t)
            }
            ObjectType::Exit if hide_exit => continue,
            ObjectType::Exit => (&sprites.exit, sprites.exit_transparency),
            ObjectType::Killer => (&sprites.killer, sprites.killer_transparency),
        };
        let floats = matches!(object.kind, ObjectType::Food | ObjectType::Exit);
        let clock = options.object_clock.unwrap_or(time);
        let frame = if options.animated_objects {
            ((clock / 0.014) as i64).rem_euclid(frames.len() as i64) as usize
        } else {
            0
        };
        let bob = if floats && !options.still_objects {
            5.0 * (clock * 15.5 + object.floating_phase).sin() / M2P
        } else {
            0.0
        };
        let x = ((object.r.x - origin.x) * M2P - ANIM_WIDTH as f64 / 2.0) as i32;
        let y = ((object.r.y - origin.y) * M2P - ANIM_WIDTH as f64 / 2.0) as i32;
        let left = origin.x + x as f64 / M2P;
        let bottom = origin.y + y as f64 / M2P + bob;
        let quad =
            Instance::new([left, bottom + size], [size, 0.0], [0.0, -size]).distance(BIKE_DISTANCE);
        out.push(quad.image(frames[frame], Some(transparency)));

        if options.gravity_arrows
            && object.kind == ObjectType::Food
            && let Some((arrows, transparency)) = &sprites.arrows
        {
            let index = match object.gravity {
                Gravity::Up => 0,
                Gravity::Down => 1,
                Gravity::Left => 2,
                Gravity::Right => 3,
                Gravity::None => continue,
            };
            out.push(
                quad.image(arrows[index], Some(*transparency))
                    .mode(mode::RECOLOR)
                    .color(GRAVITY_ARROW_COLOR),
            );
        }
    }

    // The followed bike uses the first bike pictures, everyone else the second
    let (own_bike, other_bike) = if follow == 0 { (0, 1) } else { (1, 0) };
    let shirt = |index: usize| sprites.shirts.get(index).copied().flatten();
    let has_flag = |rider: usize| {
        if !(multiplayer && session.flagtag) {
            return false;
        }
        let a_has_flag = session
            .riders
            .last()
            .map(|r| r.pose.flagtag_a)
            .unwrap_or(false);
        let immunity = session
            .riders
            .last()
            .map(|r| r.pose.flagtag_immunity)
            .unwrap_or(false);
        if (rider == 0) == a_has_flag {
            return true;
        }
        immunity && (time * 30.0) as i64 % 2 != 0
    };

    let draw_ghost = |i: usize, out: &mut Vec<Instance>| {
        let ghost = &session.ghosts[i];
        draw_bike(
            &BikeDraw {
                pose: &ghost.pose,
                animation: &ghost.animation,
                sprites: &sprites.bikes[other_bike],
                shirt: shirt(session.riders.len() + i),
                flag: None,
            },
            out,
        );
    };
    let draw_rider = |index: usize, bike: usize, shirt: Option<Sprite>, out: &mut Vec<Instance>| {
        let rider = &session.riders[index];
        draw_bike(
            &BikeDraw {
                pose: &rider.pose,
                animation: &rider.animation,
                sprites: &sprites.bikes[bike],
                shirt,
                flag: has_flag(index).then_some(sprites.flag),
            },
            out,
        );
    };
    let draw_riders = |out: &mut Vec<Instance>| {
        if multiplayer {
            draw_rider(1 - follow, other_bike, None, out);
        }
        draw_rider(follow, own_bike, shirt(0), out);
    };
    let followed_ghost = ghost.filter(|&i| i < session.ghosts.len());
    let ghosts_start = out.len() as u32;
    for i in (0..session.ghosts.len()).filter(|&i| Some(i) != ghost) {
        draw_ghost(i, out);
    }
    // Following a merged replay makes the main replay's bikes ghosts too
    if followed_ghost.is_some() {
        draw_riders(out);
    }
    let ghosts = ghosts_start..out.len() as u32;
    match followed_ghost {
        // On top, drawn like any bike
        Some(i) => draw_ghost(i, out),
        None => draw_riders(out),
    }
    ghosts
}

// Objects and bikes of an Across view: there is no multiplayer, flag or shirts
fn across_instances(
    sprites: &AcrossSprites,
    session: &Session,
    follow: usize,
    ghost: Option<usize>,
    options: &DynamicOptions,
    active: &dyn Fn(usize, &LevelObject) -> bool,
    out: &mut Vec<Instance>,
) -> std::ops::Range<u32> {
    let clock = options.object_clock.unwrap_or(session.time);
    across::object_instances(
        sprites,
        &session.objects,
        active,
        session.object_time(options.object_clock),
        clock,
        options.animated_objects,
        out,
    );
    let draw = |rider: &replay::Rider, out: &mut Vec<Instance>| {
        across::draw_bike(&rider.pose, &rider.animation, &sprites.bike, out);
    };
    let draw_riders = |out: &mut Vec<Instance>| {
        for (i, rider) in session.riders.iter().enumerate() {
            if i != follow {
                draw(rider, out);
            }
        }
        if let Some(rider) = session.riders.get(follow) {
            draw(rider, out);
        }
    };
    let followed_ghost = ghost.and_then(|i| session.ghosts.get(i));
    let ghosts_start = out.len() as u32;
    for (i, g) in session.ghosts.iter().enumerate() {
        if Some(i) != ghost {
            draw(g, out);
        }
    }
    // Following a merged replay makes the main replay's bike a ghost too
    if followed_ghost.is_some() {
        draw_riders(out);
    }
    let ghosts = ghosts_start..out.len() as u32;
    match followed_ghost {
        Some(g) => draw(g, out),
        None => draw_riders(out),
    }
    ghosts
}
