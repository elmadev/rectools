// The CPU half of drawing a frame: where each view looks, and every instance, timer digit and
// minimap it draws. It needs no GPU, so it can be tested, and gpu.rs only uploads the result
// and encodes the passes.

use crate::camera::{self, world_view_corner};
use crate::fill_map::Tiling;
use crate::hud::{PixelRect, last_apple_rects, timer_rects};
use crate::instance::{CanvasRect, Instance, mode, snap_to_pixel_center};
use crate::options::{CameraMode, GhostStyle, RenderOptions, TextureZoom};
use crate::scene::{Chunk, Look, Pictures, Scene, TextureRef, dynamic_instances};
use crate::world::{View, World};
use bytemuck::{Pod, Zeroable};
use replay::Vec2;
use std::ops::Range;

pub(crate) const UNIFORM_STRIDE: u64 = 256;
pub(crate) const MAX_VIEWS: usize = 3;
// Each view has a slot, then its minimap and the minimap's border, and the last is for the
// frame behind the views
pub(crate) const MINIMAP_SLOT: usize = MAX_VIEWS;
pub(crate) const MINIMAP_BORDER_SLOT: usize = 2 * MAX_VIEWS;
pub(crate) const BACKGROUND_SLOT: usize = 3 * MAX_VIEWS;
pub(crate) const UNIFORM_SLOTS: u64 = BACKGROUND_SLOT as u64 + 1;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, Pod, Zeroable)]
pub(crate) struct ViewUniform {
    corner: [f32; 2],
    level_corner: [f32; 2],
    canvas_origin: [f32; 2],
    size: [f32; 2],
    offset: [f32; 2],
    // The view's bottom left pixel on the canvas, counted from the canvas origin
    canvas_pixel: [f32; 2],
    pixels_per_meter: f32,
    texture_scale: f32,
    sky_x: f32,
    // 0 when the frame has no qgrass and front pictures
    front_grass_rows: f32,
    // 1 to tile textures from the canvas origin like the game does at each zoom, 0 to keep
    // each tile where it is at zoom 1
    game_textures: u32,
    // In the background slot, which the present pass reads: 1 when the frame draws translucent
    // ghosts into their own layer, which is otherwise left out
    translucent_ghosts: u32,
    // 1 when the bikes go over everything, pictures in the background
    bikes_on_top: u32,
    // The first of the view's four rows in the fill maps plus 1, or 0 when the fills work out
    // each pixel's texel themselves
    fill_maps: u32,
}

const _: () = assert!(std::mem::size_of::<ViewUniform>().is_multiple_of(8));

// A rectangle of the timer or the info panel's text, in target pixels from the bottom left:
// the text in a color of the palette, the timer as TIMER_COLOR
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub(crate) struct HudRect {
    pub rect: [f32; 4],
    pub color: u32,
}

// Each pixel mapped to the brightest or darkest color by the LGR's timer palette map
pub(crate) const TIMER_COLOR: u32 = u32::MAX;

impl HudRect {
    pub(crate) fn new(r: PixelRect, color: u32) -> HudRect {
        HudRect {
            rect: [r.x as f32, r.y as f32, r.width as f32, r.height as f32],
            color,
        }
    }
}

// Within how many pixels of a bike the objects get a pass of their own: two of the GPU's tiles,
// as a tiled GPU would put them in the same ones
const OBJECT_PASS_MARGIN: f32 = 64.0;

// Where `instance` reaches in the world, as its left, bottom, right and top
fn reach(instance: &Instance) -> [f32; 4] {
    let ([x, y], [ux, uy], [vx, vy]) = (instance.origin, instance.axis_u, instance.axis_v);
    let xs = [x, x + ux, x + vx, x + ux + vx];
    let ys = [y, y + uy, y + vy, y + uy + vy];
    let min = |v: [f32; 4]| v.into_iter().fold(f32::INFINITY, f32::min);
    let max = |v: [f32; 4]| v.into_iter().fold(f32::NEG_INFINITY, f32::max);
    [min(xs), min(ys), max(xs), max(ys)]
}

// Whether any of `objects` comes within `margin` meters of the bikes. Drawn in one pass, a bike
// over an object lost the WebGL2 context on a Pixel 10 Pro (PowerVR), so then the objects get a
// pass of their own, which costs a pass the rest of the time.
fn objects_near_bikes(objects: &[Instance], bikes: &[Instance], margin: f32) -> bool {
    let Some(b) = bikes.iter().map(reach).reduce(|a, r| {
        [
            a[0].min(r[0]),
            a[1].min(r[1]),
            a[2].max(r[2]),
            a[3].max(r[3]),
        ]
    }) else {
        return false;
    };
    objects.iter().map(reach).any(|o| {
        o[0] <= b[2] + margin
            && o[2] >= b[0] - margin
            && o[1] <= b[3] + margin
            && o[3] >= b[1] - margin
    })
}

// What one view draws, as ranges of the per-frame buffers
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ViewDraw {
    // What covers the ground and the sky
    pub(crate) ground_fill: Range<u32>,
    pub(crate) sky_fill: Range<u32>,
    // The objects, then the bikes, with the translucent ones in `ghosts`
    pub(crate) dynamic: Range<u32>,
    // The objects when they get a pass of their own, before the bikes, see objects_near_bikes.
    // Otherwise empty, at the start of `dynamic`, and the bikes pass draws them.
    pub(crate) objects: Range<u32>,
    pub(crate) ghosts: Range<u32>,
    pub(crate) timer: Range<u32>,
    pub(crate) minimap: Option<MinimapDraw>,
    // The runs of the pictures' ground, sky and unclipped groups the view draws, as ranges of
    // FramePlan::picture_runs
    pub(crate) pictures: [Range<usize>; 3],
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MinimapDraw {
    pub(crate) inner: [u32; 4],
    pub(crate) border: [u32; 4],
    // The border, then the ground and the sky fill
    pub(crate) fills: u32,
    pub(crate) icons: Range<u32>,
}

// Why a frame can't be drawn
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum RenderError {
    #[error("{0} views, but a frame has at most {MAX_VIEWS}")]
    TooManyViews(usize),
    #[error("an image of {size} pixels is bigger than the GPU's largest texture, {max}")]
    ImageTooLarge { size: usize, max: usize },
    #[error("no room is left in the atlas for another shirt: build the scene with it")]
    AtlasFull,
}

// What a frame draws. Kept between frames so its buffers are reused.
#[derive(Debug, Default)]
pub struct FramePlan {
    // A ViewUniform per slot, UNIFORM_STRIDE bytes apart
    pub(crate) uniforms: Vec<u8>,
    pub(crate) instances: Vec<Instance>,
    // By instance, where the game puts it: objects at the view's zoom
    pub(crate) rects: Vec<CanvasRect>,
    // The level pictures' rects of each view in turn, as many as the scene's level instances
    pub(crate) level_rects: Vec<CanvasRect>,
    // They changed since the frame before, and go to the GPU again
    pub(crate) level_rects_changed: bool,
    placed: Vec<(usize, CanvasRect)>,
    // What level_rects are for: the scene, and each view's zoom
    level_rects_for: Vec<(u64, u64)>,
    pub(crate) picture_runs: Vec<Range<u32>>,
    pub(crate) timer: Vec<HudRect>,
    pub(crate) views: Vec<ViewDraw>,
    // The instance of the frame behind the views, None when they cover the target
    pub(crate) background: Option<u32>,
    pub(crate) translucent_ghosts: bool,
    // Rows of `fill_map_width`, four for each view that has them: the ground's texel for
    // each column and row of the view, then the sky's, see fill_map.rs
    pub(crate) fill_maps: Vec<u16>,
    pub(crate) fill_map_width: u32,
}

fn view_uniform(
    scene: &Scene,
    world: &World,
    params: &View,
    target_width: u32,
    options: &RenderOptions,
    front_grass: bool,
) -> ViewUniform {
    let [vx, vy, vw, vh] = params.viewport;
    let ppm = camera::pixels_per_meter(params.zoom);
    let corner = world_view_corner(world, params, target_width);
    let canvas_corner = match &scene.look {
        Look::Elma(elma) => elma.canvas_corner,
        Look::Across(_) => scene.canvas_origin,
    };
    // The game snaps the canvas origin to the middle of a pixel at the current zoom and the
    // level to whole pixels from it. Then the pixels are at the same places on the level
    // wherever the camera is, and each shows the same texture pixel: between pixels, which
    // texture pixels come out wider would change as the camera moves, and textures shimmer.
    let quantize = |m: f64| snap_to_pixel_center(m, ppm);
    let origin = Vec2::new(quantize(canvas_corner.x), quantize(canvas_corner.y));
    let view_left = ((corner.x - origin.x) * ppm).floor();
    let view_bottom = ((corner.y - origin.y) * ppm).floor();
    let level_corner = Vec2::new(origin.x + view_left / ppm, origin.y + view_bottom / ppm);
    let sky_x = (view_left / 2.0).trunc();
    let f2 = |v: Vec2| [v.x as f32, v.y as f32];
    ViewUniform {
        corner: f2(corner),
        level_corner: f2(level_corner),
        canvas_origin: f2(origin),
        size: [vw as f32, vh as f32],
        offset: [vx as f32, vy as f32],
        canvas_pixel: [view_left as f32, view_bottom as f32],
        pixels_per_meter: ppm as f32,
        texture_scale: if options.zoom_textures {
            params.zoom as f32
        } else {
            1.0
        },
        sky_x: sky_x as f32,
        front_grass_rows: if front_grass {
            params.zoom.round().max(1.0) as f32
        } else {
            0.0
        },
        game_textures: (options.textures == TextureZoom::Game) as u32,
        bikes_on_top: options.dynamic.pictures_in_background as u32,
        ..Default::default()
    }
}

// Adds the view's four rows of fill maps to `maps`, rows `width` long, and returns the first,
// when the ground and sky are Elma textures the atlas has at their size
fn fill_maps(
    maps: &mut Vec<u16>,
    width: u32,
    scene: &Scene,
    u: &ViewUniform,
    [_, _, vw, vh]: [u32; 4],
) -> Option<u32> {
    let Look::Elma(elma) = &scene.look else {
        return None;
    };
    let tiling = |t: &TextureRef| {
        let exact = t.rect.width as f64 == t.size.0 && t.rect.height as f64 == t.size.1;
        exact.then_some(Tiling {
            w: t.size.0 as i32,
            h: t.size.1 as i32,
            scale: u.texture_scale,
            game: u.game_textures != 0,
        })
    };
    let (ground, sky) = (tiling(&scene.foreground)?, tiling(&elma.background)?);
    let row = (maps.len() / width as usize) as u32;
    let start = maps.len();
    maps.resize(start + 4 * width as usize, 0);
    let rows = &mut maps[start..];
    let (left, bottom) = (u.canvas_pixel[0] as i32, u.canvas_pixel[1] as i32);
    let sky_x = u.sky_x as i32;
    let w = width as usize;
    let (vw, vh) = (vw as usize, vh as usize);
    ground.columns(left, &mut rows[..vw]);
    ground.rows(bottom, &mut rows[w..w + vh]);
    sky.columns(sky_x, &mut rows[2 * w..2 * w + vw]);
    sky.rows(0, &mut rows[3 * w..3 * w + vh]);
    Some(row)
}

// Whether `views` cover every pixel of the target. Their edges cut it into cells that are
// each inside a view or outside them all.
fn covers(views: &[View], (width, height): (u32, u32)) -> bool {
    let mut xs = vec![0, width];
    let mut ys = vec![0, height];
    for v in views {
        let [x, y, w, h] = v.viewport;
        xs.extend([x.min(width), (x + w).min(width)]);
        ys.extend([y.min(height), (y + h).min(height)]);
    }
    for edges in [&mut xs, &mut ys] {
        edges.sort_unstable();
        edges.dedup();
    }
    let inside = |x: u32, y: u32| {
        views.iter().any(|v| {
            let [vx, vy, vw, vh] = v.viewport;
            (vx..vx + vw).contains(&x) && (vy..vy + vh).contains(&y)
        })
    };
    xs.windows(2)
        .all(|x| ys.windows(2).all(|y| inside(x[0], y[0])))
}

// Works out a frame of `views` of `scene` into `frame`, whose buffers are reused
pub fn plan(
    frame: &mut FramePlan,
    scene: &Scene,
    world: &World,
    views: &[View],
    options: &RenderOptions,
    (width, height): (u32, u32),
) -> Result<(), RenderError> {
    if views.len() > MAX_VIEWS {
        return Err(RenderError::TooManyViews(views.len()));
    }
    // Each view's fill maps are rows as long as its longer side
    let map_width = views
        .iter()
        .map(|v| v.viewport[2].max(v.viewport[3]))
        .max()
        .unwrap_or(0);
    frame.fill_maps.clear();
    frame.picture_runs.clear();
    let picture_runs = &mut frame.picture_runs;
    frame.fill_map_width = map_width;
    let frame_maps = &mut frame.fill_maps;
    frame.uniforms.clear();
    frame
        .uniforms
        .resize((UNIFORM_STRIDE * UNIFORM_SLOTS) as usize, 0);
    let hud_scale = options.hud_scale.max(1) as i32;
    let uniforms = &mut frame.uniforms;
    let mut write_uniform = |slot: usize, u: &ViewUniform| {
        let offset = slot * UNIFORM_STRIDE as usize;
        uniforms[offset..offset + std::mem::size_of::<ViewUniform>()]
            .copy_from_slice(bytemuck::bytes_of(u));
    };
    let draws = &mut frame.views;
    draws.clear();
    let timer_data = &mut frame.timer;
    timer_data.clear();
    let scratch = &mut frame.instances;
    scratch.clear();
    frame.placed.clear();
    // With pictures in the background, the bikes go over everything
    let front_grass =
        !options.dynamic.pictures_in_background && scene.pictures().is_some_and(|p| p.front_grass);
    for (i, params) in views.iter().enumerate() {
        let mut u = view_uniform(scene, world, params, width, options, front_grass);
        if let Some(row) = fill_maps(frame_maps, map_width, scene, &u, params.viewport) {
            u.fill_maps = row + 1;
        }
        write_uniform(i, &u);

        // Ground and sky cover the whole view, a meter beyond each edge
        let ppm = u.pixels_per_meter as f64;
        let corner = [u.level_corner[0] as f64, u.level_corner[1] as f64];
        let (w, h) = (
            params.viewport[2] as f64 / ppm,
            params.viewport[3] as f64 / ppm,
        );
        let fill = |texture: &TextureRef, mode_| {
            Instance::new(
                [corner[0] - 1.0, corner[1] + h + 1.0],
                [w + 2.0, 0.0],
                [0.0, -(h + 2.0)],
            )
            .texture(texture.rect, texture.size)
            .mode(mode_)
            .distance(2000.0)
        };
        let ground_start = scratch.len() as u32;
        scratch.push(fill(&scene.foreground, mode::TEXTURE));
        let sky_start = scratch.len() as u32;
        match &scene.look {
            Look::Elma(elma) => scratch.push(fill(&elma.background, mode::SKY)),
            Look::Across(sprites) => crate::across::sky_instances(
                sprites,
                Vec2::new(u.corner[0] as f64, u.corner[1] as f64),
                (params.viewport[2] as f64, params.viewport[3] as f64),
                ppm,
                scratch,
            ),
        }
        let dynamic_start = scratch.len() as u32;
        let (objects_end, ghosts) = dynamic_instances(
            scene,
            world,
            params,
            &options.dynamic,
            scratch,
            &mut frame.placed,
        );

        let timer_start = timer_data.len() as u32;
        if let Some(timer) = params.timer {
            let rects = timer_rects(
                params.viewport[2] as i32,
                params.viewport[3] as i32,
                timer.best,
                timer.flagtag,
                timer.centiseconds,
                matches!(scene.look, Look::Across(_)),
                hud_scale,
            );
            timer_data.extend(rects.iter().map(|&r| HudRect::new(r, TIMER_COLOR)));
        }
        if let Some((apples, last)) = params.last_apple
            && !matches!(scene.look, Look::Across(_))
        {
            let rects = last_apple_rects(params.viewport[2] as i32, apples, last, hud_scale);
            timer_data.extend(rects.iter().map(|r| HudRect::new(r.rect, r.color as u32)));
        }
        // From the view's bottom left to the target's
        let [vx, vy, _, vh] = params.viewport;
        for r in &mut timer_data[timer_start as usize..] {
            r.rect[0] += vx as f32;
            r.rect[1] += height.saturating_sub(vy + vh) as f32;
        }
        let dynamic_end = scratch.len() as u32;
        let (start, end) = (dynamic_start as usize, objects_end as usize);
        let near = objects_near_bikes(
            &scratch[start..end],
            &scratch[end..dynamic_end as usize],
            OBJECT_PASS_MARGIN / u.pixels_per_meter,
        );
        let objects = dynamic_start..if near { objects_end } else { dynamic_start };
        let minimap = if params.minimap {
            // Without a bike to follow, as if at the level's origin facing left
            let rider = params.followed(world).copied().unwrap_or_default();
            let phase = rider.animation.camera_turning.turn_phase;
            let ppm_view = u.pixels_per_meter as f64;
            let view_corner = Vec2::new(u.corner[0] as f64, u.corner[1] as f64);
            let view_size = Vec2::new(
                params.viewport[2] as f64 / ppm_view,
                params.viewport[3] as f64 / ppm_view,
            );
            let (anchor, fraction) = match params.center {
                Some(center) => (center, 0.5),
                None if scene.minimap.bike_like_view => (
                    rider.pose.bike_r,
                    (rider.pose.bike_r.x - view_corner.x) / view_size.x,
                ),
                None if params.camera == CameraMode::Centered => (rider.pose.bike_r, 0.5),
                None => (rider.pose.bike_r, 0.2 + phase * 0.6),
            };
            crate::minimap::place(&scene.minimap, params.viewport, anchor, fraction).map(|p| {
                let uniform = |rect: [u32; 4], corner: Vec2| {
                    let f2 = |v: Vec2| [v.x as f32, v.y as f32];
                    ViewUniform {
                        corner: f2(corner),
                        level_corner: f2(corner),
                        canvas_origin: f2(scene.canvas_origin),
                        size: [rect[2] as f32, rect[3] as f32],
                        offset: [rect[0] as f32, rect[1] as f32],
                        pixels_per_meter: p.pixels_per_meter as f32,
                        texture_scale: 1.0,
                        ..Default::default()
                    }
                };
                let pixel = 1.0 / p.pixels_per_meter;
                write_uniform(MINIMAP_SLOT + i, &uniform(p.inner, p.corner));
                write_uniform(
                    MINIMAP_BORDER_SLOT + i,
                    &uniform(p.border, p.corner - Vec2::new(pixel, pixel)),
                );
                let m = &scene.minimap;
                let fills = scratch.len() as u32;
                scratch.push(crate::minimap::fill(&p, m.border, true));
                scratch.push(crate::minimap::fill(&p, m.ground, false));
                scratch.push(crate::minimap::fill(&p, m.sky, false));
                let icons_start = scratch.len() as u32;
                crate::minimap::icons(m, &p, world, params, scratch);
                MinimapDraw {
                    inner: p.inner,
                    border: p.border,
                    fills,
                    icons: icons_start..scratch.len() as u32,
                }
            })
        } else {
            None
        };
        let pictures = scene
            .pictures()
            .map(|p| visible_runs(p, &u, params.viewport, picture_runs))
            .unwrap_or_default();
        draws.push(ViewDraw {
            pictures,
            minimap,
            ground_fill: ground_start..sky_start,
            sky_fill: sky_start..dynamic_start,
            dynamic: dynamic_start..dynamic_end,
            objects,
            ghosts,
            timer: timer_start..timer_data.len() as u32,
        });
    }

    let translucent_ghosts =
        options.ghosts == GhostStyle::Translucent && draws.iter().any(|d| !d.ghosts.is_empty());
    // The game tiles qframe.pcx behind the views, flipped like everything it draws
    write_uniform(
        BACKGROUND_SLOT,
        &ViewUniform {
            size: [width as f32, height as f32],
            pixels_per_meter: 1.0,
            texture_scale: 1.0,
            game_textures: 1,
            translucent_ghosts: translucent_ghosts as u32,
            ..Default::default()
        },
    );
    frame.background = (!covers(views, (width, height))).then(|| {
        let (w, h) = (width as f64, height as f64);
        scratch.push(
            Instance::new([0.0, h], [w, 0.0], [0.0, -h])
                .texture(scene.qframe.rect, scene.qframe.size)
                .mode(mode::SKY),
        );
        scratch.len() as u32 - 1
    });
    frame.translucent_ghosts = translucent_ghosts;
    frame.rects.clear();
    frame.rects.resize(frame.instances.len(), CanvasRect::NONE);
    for &(i, rect) in &frame.placed {
        frame.rects[i] = rect;
    }
    // Worked out again and sent to the GPU when the zoom or the scene changes
    let level_rects_for: Vec<(u64, u64)> =
        views.iter().map(|v| (scene.id, v.zoom.to_bits())).collect();
    frame.level_rects_changed = level_rects_for != frame.level_rects_for;
    if frame.level_rects_changed {
        frame.level_rects.clear();
        if let Look::Elma(elma) = &scene.look {
            for params in views {
                elma.picture_rects(params.zoom, &mut frame.level_rects);
            }
        }
        frame.level_rects_for = level_rects_for;
    }
    Ok(())
}

// Adds the runs of chunks of each of the pictures' groups that reach into the view to `runs`,
// and returns where each group's are. A picture all outside the view would change none of its
// pixels, and those drawn keep their order, which decides between pictures at one distance.
fn visible_runs(
    pictures: &Pictures,
    u: &ViewUniform,
    [_, _, vw, vh]: [u32; 4],
    runs: &mut Vec<Range<u32>>,
) -> [Range<usize>; 3] {
    let ppm = u.pixels_per_meter as f64;
    // A few pixels to spare: placed pictures can be a pixel or so off their quads
    let margin = 4.0 / ppm;
    let min = Vec2::new(
        u.level_corner[0] as f64 - margin,
        u.level_corner[1] as f64 - margin,
    );
    let max = Vec2::new(
        u.level_corner[0] as f64 + vw as f64 / ppm + margin,
        u.level_corner[1] as f64 + vh as f64 / ppm + margin,
    );
    let mut group = |chunks: &[Chunk]| {
        let start = runs.len();
        for c in chunks {
            if c.max.x < min.x || c.min.x > max.x || c.max.y < min.y || c.min.y > max.y {
                continue;
            }
            let joins = runs.len() > start && runs.last().is_some_and(|r| r.end == c.range.start);
            match runs.last_mut() {
                Some(run) if joins => run.end = c.range.end,
                _ => runs.push(c.range.clone()),
            }
        }
        start..runs.len()
    };
    [
        group(&pictures.chunks[0]),
        group(&pictures.chunks[1]),
        group(&pictures.chunks[2]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::ViewParams;
    use crate::scene::SceneOptions;
    use crate::world::{Bike, DrawnBike, SessionFrame};
    use elma_formats::Graphics;
    use elma_formats::lev::{
        Gravity, Level, Object, ObjectType, Polygon, TopTens, Vec2 as LevVec2,
    };
    use elma_formats::lgr::Lgr;
    use replay::{Session, Settings};
    use std::sync::Arc;

    // Plans `views` of a replay as Renderer::render does
    fn plan_session(
        frame: &mut FramePlan,
        scene: &Scene,
        session: &Session,
        views: &[ViewParams],
        options: &RenderOptions,
        size: (u32, u32),
    ) -> Result<(), RenderError> {
        let mut built = SessionFrame::default();
        built.build(session, views);
        let views: Vec<View> = built.views().collect();
        plan(frame, scene, &built.world(session), &views, options, size)
    }

    fn session_uniform(
        scene: &Scene,
        session: &Session,
        params: &ViewParams,
        options: &RenderOptions,
    ) -> ViewUniform {
        let mut built = SessionFrame::default();
        built.build(session, std::slice::from_ref(params));
        let view = built.views().next().unwrap();
        view_uniform(scene, &built.world(session), &view, 640, options, false)
    }

    fn level() -> Level {
        let v = |x, y| LevVec2 { x, y };
        let object = |x, y, kind| Object {
            r: v(x, y),
            kind,
            gravity: Gravity::None,
            animation: 0,
            motion: None,
        };
        Level {
            level_id: 1,
            name: String::new(),
            lgr: String::new(),
            foreground: String::new(),
            background: String::new(),
            polygons: vec![Polygon {
                is_grass: false,
                vertices: vec![v(0.0, 0.0), v(40.0, 0.0), v(40.0, 10.0), v(0.0, 10.0)],
            }],
            objects: vec![
                object(5.0, 8.0, ObjectType::Start),
                object(20.0, 8.0, ObjectType::Food),
                object(30.0, 8.0, ObjectType::Exit),
            ],
            sprites: vec![],
            topology_errors: false,
            top_tens: TopTens::default(),
        }
    }

    // The level with its start replay, and `ghosts` copies of it as ghosts
    fn setup(ghosts: usize) -> (Scene, Session) {
        let level = level();
        let graphics = Graphics::Elma(Box::new(Lgr::stub()));
        let scene = Scene::new(&graphics, &level, &[], &SceneOptions::default(), 4096).unwrap();
        let rec = replay::start_rec(&level);
        let ghost = Arc::new(rec.rides[0].clone());
        let session = Session::new(&level, &rec, &vec![ghost; ghosts], Settings::default());
        (scene, session)
    }

    fn view(follow: usize, timer: bool, minimap: bool) -> ViewParams {
        ViewParams {
            follow,
            viewport: [0, 0, 640, 480],
            zoom: 1.0,
            camera: CameraMode::Centered,
            pan: Vec2::default(),
            timer,
            best_time: None,
            center: None,
            minimap,
            last_apple_time: false,
        }
    }

    fn planned(scene: &Scene, session: &Session, views: &[ViewParams]) -> FramePlan {
        let mut frame = FramePlan::default();
        plan_session(
            &mut frame,
            scene,
            session,
            views,
            &RenderOptions::default(),
            (640, 480),
        )
        .unwrap();
        frame
    }

    #[test]
    fn a_view_draws_its_fills_bikes_and_objects() {
        let (scene, session) = setup(0);
        let frame = planned(&scene, &session, &[view(0, false, false)]);
        assert_eq!(frame.views.len(), 1);
        let draw = &frame.views[0];
        assert_eq!(draw.ground_fill.len(), 1);
        assert_eq!(draw.sky_fill.len(), 1);
        // The bike's parts, the food and the exit
        assert!(draw.dynamic.len() > 2);
        assert!(draw.ghosts.is_empty());
        assert!(draw.timer.is_empty());
        assert_eq!(draw.minimap, None);
        assert_eq!(
            frame.uniforms.len(),
            (UNIFORM_STRIDE * UNIFORM_SLOTS) as usize
        );
        // One view covers the target, so the frame behind it isn't drawn
        assert_eq!(frame.background, None);
    }

    #[test]
    fn objects_near_a_bike_get_a_pass_of_their_own() {
        let (scene, mut session) = setup(1);
        let draw =
            |session: &Session| planned(&scene, session, &[view(0, false, false)]).views[0].clone();
        // The bike is at the start, 15 m from the apple
        let far = draw(&session);
        assert!(far.objects.is_empty());
        assert_eq!(far.objects.start, far.dynamic.start);
        let kind = |k| session.objects.iter().position(|o| o.kind == k).unwrap();
        let (food, start) = (kind(ObjectType::Food), kind(ObjectType::Start));
        session.objects[food].r = session.objects[start].r;
        // The apple and the exit, then the ghost and the bike
        let near = draw(&session);
        assert_eq!(near.objects, near.dynamic.start..near.dynamic.start + 2);
        assert_eq!(near.ghosts.start, near.objects.end);
        assert!(!near.ghosts.is_empty() && near.ghosts.end < near.dynamic.end);
    }

    #[test]
    fn a_stretched_bike_reaches_objects_far_from_its_wheels_and_head() {
        let quad = |origin: [f64; 2], u: [f64; 2], v: [f64; 2]| Instance::new(origin, u, v);
        // Wheels and a head near the origin, and the suspension stretched 20 m to the right
        let bike = [
            quad([-0.4, 0.4], [0.8, 0.0], [0.0, -0.8]),
            quad([0.0, 1.5], [0.6, 0.0], [0.0, -0.6]),
            quad([0.0, 0.05], [20.0, 0.0], [0.0, -0.1]),
        ];
        let apple = |x: f64, y: f64| [quad([x - 0.4, y + 0.4], [0.8, 0.0], [0.0, -0.8])];
        // An apple under the far end of the suspension, and one well clear of all of it
        assert!(objects_near_bikes(&apple(19.5, 0.0), &bike, 0.5));
        assert!(!objects_near_bikes(&apple(10.0, 6.0), &bike, 0.5));
        // Within the margin of its end
        assert!(objects_near_bikes(&apple(20.6, 0.0), &bike, 0.5));
    }

    #[test]
    fn the_frame_behind_the_views_is_drawn_where_they_leave_gaps() {
        let (scene, session) = setup(0);
        let mut left = view(0, false, false);
        left.viewport = [0, 0, 320, 480];
        let mut right = left;
        right.viewport = [320, 0, 320, 480];
        assert_eq!(planned(&scene, &session, &[left, right]).background, None);
        // A column between them
        right.viewport = [330, 0, 310, 480];
        let frame = planned(&scene, &session, &[left, right]);
        assert_eq!(frame.background, Some(frame.instances.len() as u32 - 1));
        // Overlapping views that leave out a corner
        let mut big = view(0, false, false);
        big.viewport = [0, 0, 640, 240];
        let mut small = big;
        small.viewport = [0, 120, 320, 360];
        assert!(
            planned(&scene, &session, &[big, small])
                .background
                .is_some()
        );
    }

    #[test]
    fn each_view_shows_its_own_riders_last_apple() {
        let level = level();
        let graphics = Graphics::Elma(Box::new(Lgr::stub()));
        let scene = Scene::new(&graphics, &level, &[], &SceneOptions::default(), 4096).unwrap();
        let mut rec = replay::start_rec(&level);
        // Events point into the objects as the session orders them
        let food = Session::new(&level, &rec, &[], Settings::default())
            .objects
            .iter()
            .position(|o| o.kind == ObjectType::Food)
            .unwrap();
        let apple = elma_formats::rec::Event {
            time: 0.0,
            object_id: food as i16,
            event_id: elma_formats::rec::WavEvent::Food,
            volume: 0.99,
        };
        let mut took_apple = rec.rides[0].clone();
        took_apple.events.push(apple);
        // A two player replay where only the first player took the apple, and a ghost that
        // took it too
        rec.multiplayer = true;
        rec.rides = vec![took_apple.clone(), rec.rides[0].clone()];
        let ghosts = [Arc::new(took_apple)];
        let session = Session::new(&level, &rec, &ghosts, Settings::default());
        let with_apples = |follow| ViewParams {
            last_apple_time: true,
            ..view(follow, false, false)
        };
        let frame = planned(&scene, &session, &[with_apples(0), with_apples(1)]);
        assert!(!frame.views[0].timer.is_empty());
        // The ghost's apple isn't the second player's
        assert!(frame.views[1].timer.is_empty());
        // Following the ghost shows its own
        let frame = planned(&scene, &session, &[with_apples(2)]);
        assert!(!frame.views[0].timer.is_empty());
    }

    #[test]
    fn the_timer_and_minimap_are_drawn_when_asked() {
        let (scene, session) = setup(0);
        let frame = planned(&scene, &session, &[view(0, true, true)]);
        let draw = &frame.views[0];
        assert!(!draw.timer.is_empty());
        let minimap = draw.minimap.as_ref().unwrap();
        // Bottom right, inside its border
        assert!(minimap.inner[0] > 320 && minimap.inner[1] > 240);
        assert_eq!(minimap.border[0] + 1, minimap.inner[0]);
        assert!(!minimap.icons.is_empty());
    }

    #[test]
    fn the_minimap_shows_the_apples_of_the_bike_followed() {
        let (scene, mut session) = setup(1);
        // The main replay took the apple and the ghost didn't
        let food = session
            .objects
            .iter()
            .position(|o| o.kind == ObjectType::Food)
            .unwrap();
        session.objects[food].active = false;
        let icons = |follow| {
            let frame = planned(&scene, &session, &[view(follow, false, true)]);
            frame.views[0].minimap.as_ref().unwrap().icons.len()
        };
        // Four pieces of a ring
        assert_eq!(icons(1), icons(0) + 4);
    }

    #[test]
    fn following_a_ghost_draws_the_main_bike_as_a_ghost() {
        let (scene, session) = setup(1);
        let main = planned(&scene, &session, &[view(0, false, false)]);
        let ghost = planned(&scene, &session, &[view(1, false, false)]);
        let len = |f: &FramePlan| f.views[0].ghosts.len();
        assert!(len(&main) > 0);
        // Following the ghost, the main bike is the ghost-drawn one, with as many parts
        assert_eq!(len(&ghost), len(&main));
    }

    #[test]
    fn split_views_each_get_their_own_slot() {
        let (scene, session) = setup(0);
        let mut top = view(0, true, false);
        top.viewport = [0, 0, 640, 240];
        let mut bottom = top;
        bottom.viewport = [0, 240, 640, 240];
        let frame = planned(&scene, &session, &[top, bottom]);
        assert_eq!(frame.views.len(), 2);
        let offset_y = |slot: usize| {
            let at = slot * UNIFORM_STRIDE as usize;
            let u: &ViewUniform =
                bytemuck::from_bytes(&frame.uniforms[at..at + std::mem::size_of::<ViewUniform>()]);
            u.offset[1]
        };
        assert_eq!((offset_y(0), offset_y(1)), (0.0, 240.0));
    }

    #[test]
    fn the_timers_are_in_the_target_inside_their_views() {
        let (scene, session) = setup(0);
        let mut top = view(0, true, false);
        top.viewport = [0, 0, 640, 240];
        let mut bottom = top;
        bottom.viewport = [0, 240, 640, 240];
        let frame = planned(&scene, &session, &[top, bottom]);
        for (params, draw) in [top, bottom].iter().zip(&frame.views) {
            assert!(!draw.timer.is_empty());
            let [vx, vy, vw, vh] = params.viewport.map(|v| v as f32);
            // The view's rows from the target's bottom
            let (low, high) = (480.0 - vy - vh, 480.0 - vy);
            for r in &frame.timer[draw.timer.start as usize..draw.timer.end as usize] {
                let [x, y, w, h] = r.rect;
                assert!(x >= vx && x + w <= vx + vw, "{r:?}");
                assert!(y >= low && y + h <= high, "{r:?}");
            }
        }
    }

    // A view of `bikes` drawn in order, as a game would ask for one, without a replay
    fn world_view(bikes: &[DrawnBike], translucent: Range<usize>) -> View<'_> {
        View {
            viewport: [0, 0, 640, 480],
            zoom: 1.0,
            follow: 0,
            camera: CameraMode::Classic,
            camera_edge: None,
            pan: Vec2::default(),
            center: None,
            bikes,
            translucent,
            objects: None,
            timer: Some(crate::world::Timer {
                centiseconds: 1234,
                best: None,
                flagtag: None,
            }),
            last_apple: Some((1, 500)),
            minimap: true,
        }
    }

    #[test]
    fn a_world_without_bikes_draws_its_objects() {
        let (scene, session) = setup(0);
        let world = World {
            time: 0.0,
            bikes: &[],
            objects: &session.objects,
            hide_exit: false,
        };
        let mut frame = FramePlan::default();
        let views = [world_view(&[], 0..0)];
        plan(
            &mut frame,
            &scene,
            &world,
            &views,
            &RenderOptions::default(),
            (640, 480),
        )
        .unwrap();
        let draw = &frame.views[0];
        // The apple and the exit
        assert_eq!(draw.dynamic.len(), 2);
        // Near no bike, so they need no pass of their own
        assert!(draw.objects.is_empty());
        assert!(draw.ghosts.is_empty());
        assert!(!draw.timer.is_empty());
        assert!(draw.minimap.is_some());
    }

    #[test]
    fn the_translucent_bikes_are_where_the_view_says() {
        let (scene, session) = setup(0);
        let bike = Bike {
            pose: session.riders[0].pose,
            ..Bike::default()
        };
        let bikes = [bike; 3];
        let world = World {
            time: 0.0,
            bikes: &bikes,
            objects: &[],
            hide_exit: false,
        };
        let drawn: Vec<DrawnBike> = (0..3)
            .map(|bike| DrawnBike {
                bike,
                pictures: 1,
                flag: false,
            })
            .collect();
        let ranges = |translucent: Range<usize>| {
            let mut frame = FramePlan::default();
            let views = [world_view(&drawn, translucent)];
            plan(
                &mut frame,
                &scene,
                &world,
                &views,
                &RenderOptions::default(),
                (640, 480),
            )
            .unwrap();
            let draw = frame.views[0].clone();
            (draw.dynamic, draw.ghosts)
        };
        let (dynamic, middle) = ranges(1..2);
        let per_bike = dynamic.len() as u32 / 3;
        assert_eq!(
            middle,
            dynamic.start + per_bike..dynamic.start + 2 * per_bike
        );
        // An empty range still sits between the bikes, which are drawn around it
        let (dynamic, none) = ranges(2..2);
        assert_eq!(
            none,
            dynamic.start + 2 * per_bike..dynamic.start + 2 * per_bike
        );
        let (dynamic, all) = ranges(0..9);
        assert_eq!(all, dynamic);
    }

    #[test]
    fn the_ghost_layer_is_kept_only_for_translucent_ghosts() {
        let kept = |ghosts: usize, style: GhostStyle| {
            let (scene, session) = setup(ghosts);
            let mut frame = FramePlan::default();
            let options = RenderOptions {
                ghosts: style,
                ..Default::default()
            };
            let views = [view(0, false, false)];
            plan_session(&mut frame, &scene, &session, &views, &options, (640, 480)).unwrap();
            let at = BACKGROUND_SLOT * UNIFORM_STRIDE as usize;
            let u: &ViewUniform =
                bytemuck::from_bytes(&frame.uniforms[at..at + std::mem::size_of::<ViewUniform>()]);
            assert_eq!(u.translucent_ghosts, frame.translucent_ghosts as u32);
            frame.translucent_ghosts
        };
        assert!(kept(1, GhostStyle::Translucent));
        assert!(!kept(0, GhostStyle::Translucent));
        assert!(!kept(1, GhostStyle::Normal));
    }

    #[test]
    fn an_image_bigger_than_the_gpu_allows_is_an_error() {
        let mut lgr = Lgr::stub();
        lgr.textures[0].image.pic = elma_formats::pic::Pic8::filled(5000, 1, 1);
        let graphics = Graphics::Elma(Box::new(lgr));
        let scene = Scene::new(&graphics, &level(), &[], &SceneOptions::default(), 4096);
        assert_eq!(
            scene.err(),
            Some(RenderError::ImageTooLarge {
                size: 5000,
                max: 4096
            })
        );
    }

    #[test]
    fn a_shirt_given_later_takes_room_left_in_the_atlas() {
        use elma_formats::pic::Pic8;
        let graphics = Graphics::Elma(Box::new(Lgr::stub()));
        let options = SceneOptions::default();
        let mut scene = Scene::new(&graphics, &level(), &[], &options, 2048).unwrap();
        let shirts = |scene: &Scene| match &scene.look {
            Look::Elma(elma) => elma.sprites.shirts.clone(),
            Look::Across(_) => unreachable!(),
        };
        let shirt = Pic8::filled(152, 101, 7);
        let first = scene.place_shirt(3, Some(&shirt)).unwrap().unwrap();
        assert_eq!(shirts(&scene).len(), 4);
        assert_eq!(shirts(&scene)[3].map(|s| s.rect), Some(first));
        // A smaller one goes where it was
        let again = scene
            .place_shirt(3, Some(&Pic8::filled(149, 90, 8)))
            .unwrap()
            .unwrap();
        assert_eq!(
            (again.layer, again.x, again.y),
            (first.layer, first.x, first.y)
        );
        assert_eq!((again.width, again.height), (149, 90));
        assert_eq!(scene.place_shirt(3, None), Ok(None));
        assert!(shirts(&scene)[3].is_none());
        // Until the atlas is full
        let full = (4..2000)
            .map(|i| scene.place_shirt(i, Some(&shirt)))
            .find(Result::is_err);
        assert_eq!(full, Some(Err(RenderError::AtlasFull)));
    }

    #[test]
    fn fancyboost_pictures_go_when_it_is_off() {
        use elma_formats::lev::{Clipping, Sprite};
        let mut level = level();
        level.sprites.push(Sprite {
            picture: "zz000000".into(),
            texture: String::new(),
            mask: String::new(),
            r: LevVec2 { x: 20.0, y: 5.0 },
            distance: 500,
            clipping: Some(Clipping::Unclipped),
        });
        let graphics = Graphics::Elma(Box::new(Lgr::stub()));
        let pictures = |fancyboost| {
            let options = SceneOptions {
                fancyboost,
                ..SceneOptions::default()
            };
            let scene = Scene::new(&graphics, &level, &[], &options, 4096).unwrap();
            scene.pictures().unwrap().instances.len()
        };
        assert_eq!(pictures(true), 1);
        assert_eq!(pictures(false), 0);
    }

    #[test]
    fn a_picture_in_front_needs_qgrass_for_front_grass() {
        use crate::instance::layer;
        use elma_formats::lev::{Clipping, Sprite};
        let mut level = level();
        level.sprites.push(Sprite {
            picture: "zz000000".into(),
            texture: String::new(),
            mask: String::new(),
            r: LevVec2 { x: 20.0, y: 5.0 },
            distance: 400,
            clipping: Some(Clipping::Unclipped),
        });
        // The stub LGR has no qgrass
        let graphics = Graphics::Elma(Box::new(Lgr::stub()));
        let options = SceneOptions {
            fancyboost: true,
            ..SceneOptions::default()
        };
        let scene = Scene::new(&graphics, &level, &[], &options, 4096).unwrap();
        let pictures = scene.pictures().unwrap();
        let front = layer::FRONT | layer::OVER_BIKES;
        assert!(pictures.instances.iter().any(|i| i.layer == front));
        assert!(!pictures.front_grass);
    }

    #[test]
    fn pictures_are_resized_and_placed_like_the_game_at_each_zoom() {
        use elma_formats::lev::{Clipping, Sprite};
        let mut level = level();
        level.sprites.push(Sprite {
            picture: "zz000000".into(),
            texture: String::new(),
            mask: String::new(),
            r: LevVec2 { x: 20.0, y: 5.0 },
            distance: 500,
            clipping: Some(Clipping::Unclipped),
        });
        let graphics = Graphics::Elma(Box::new(Lgr::stub()));
        let scene = Scene::new(&graphics, &level, &[], &SceneOptions::default(), 4096).unwrap();
        let Look::Elma(elma) = &scene.look else {
            unreachable!()
        };
        let rect = |zoom: f64| {
            let mut rects = Vec::new();
            elma.picture_rects(zoom, &mut rects);
            rects[0]
        };
        for zoom in [1.0, 1.5, 0.8, 2.25] {
            let ppm = camera::pixels_per_meter(zoom);
            let origin = elma.canvas_origin(ppm);
            let r = rect(zoom);
            // A fancyboost square is 200 pixels at zoom 1, from its top left corner
            let size = (zoom * 200.0) as i32;
            assert_eq!((r.width, r.height), (size, size), "{zoom}");
            assert_eq!(r.left, ((20.0 - origin.x) * ppm) as i32);
            assert_eq!(r.top, ((-5.0 - origin.y) * ppm) as i32);
        }
        // The game leaves out pictures too close to the canvas's right side, which depends on
        // how wide the picture comes out
        let right = elma.canvas_right;
        let at = |x: f64| {
            let mut level = level.clone();
            level.sprites[0].r.x = x;
            let scene = Scene::new(&graphics, &level, &[], &SceneOptions::default(), 4096).unwrap();
            let Look::Elma(elma) = &scene.look else {
                unreachable!()
            };
            let mut rects = Vec::new();
            elma.picture_rects(1.0, &mut rects);
            rects[0]
        };
        assert_eq!(at(right - 2.0), CanvasRect::HIDDEN);
        assert_ne!(at(right - 10.0), CanvasRect::HIDDEN);
    }

    #[test]
    fn too_many_views_is_an_error() {
        let (scene, session) = setup(0);
        let views = vec![view(0, false, false); MAX_VIEWS + 1];
        let mut frame = FramePlan::default();
        let options = RenderOptions::default();
        assert_eq!(
            plan_session(&mut frame, &scene, &session, &views, &options, (640, 480)),
            Err(RenderError::TooManyViews(MAX_VIEWS + 1))
        );
    }

    #[test]
    fn planning_again_reuses_the_buffers() {
        let (scene, session) = setup(1);
        let mut frame = FramePlan::default();
        let views = [view(0, true, true)];
        let options = RenderOptions::default();
        plan_session(&mut frame, &scene, &session, &views, &options, (640, 480)).unwrap();
        let (instances, capacity) = (frame.instances.len(), frame.instances.capacity());
        plan_session(&mut frame, &scene, &session, &views, &options, (640, 480)).unwrap();
        assert_eq!(frame.instances.len(), instances);
        assert_eq!(frame.instances.capacity(), capacity);
    }

    #[test]
    fn textures_keep_their_size_without_zoom_textures() {
        let (scene, session) = setup(0);
        let params = ViewParams {
            zoom: 2.0,
            ..view(0, false, false)
        };
        let scale = |zoom_textures| {
            let options = RenderOptions {
                zoom_textures,
                ..Default::default()
            };
            session_uniform(&scene, &session, &params, &options).texture_scale
        };
        assert_eq!(scale(true), 2.0);
        assert_eq!(scale(false), 1.0);
    }

    #[test]
    fn the_level_is_at_whole_pixels_with_either_textures() {
        let (scene, session) = setup(0);
        let params = ViewParams {
            zoom: 1.25,
            pan: Vec2::new(0.013, -0.007),
            ..view(0, false, false)
        };
        for textures in [TextureZoom::Stable, TextureZoom::Game] {
            let options = RenderOptions {
                textures,
                ..Default::default()
            };
            let u = session_uniform(&scene, &session, &params, &options);
            let pixels = |a: f32, b: f32| (a as f64 - b as f64) * u.pixels_per_meter as f64;
            for axis in 0..2 {
                let p = pixels(u.level_corner[axis], u.canvas_origin[axis]);
                assert!((p - p.round()).abs() < 1e-3, "{textures:?}: {p} pixels");
                // The camera itself is between pixels
                let c = pixels(u.corner[axis], u.canvas_origin[axis]);
                assert!((c - c.round()).abs() > 1e-3);
            }
            assert_eq!(u.sky_x, u.sky_x.trunc());
        }
    }
}
