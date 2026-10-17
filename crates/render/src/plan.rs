// The CPU half of drawing a frame: where each view looks, and every instance, timer digit and
// minimap it draws. It needs no GPU, so it can be tested, and gpu.rs only uploads the result
// and encodes the passes.

use crate::camera::{self, view_corner};
use crate::hud::{PixelRect, last_apple_rects, timer_rects};
use crate::instance::{Instance, M2P, mode, snap_to_pixel_center};
use crate::options::{CameraMode, GhostStyle, RenderOptions, TextureZoom, ViewParams};
use crate::scene::{Look, Scene, TextureRef, dynamic_instances};
use bytemuck::{Pod, Zeroable};
use replay::{Session, TIME_TO_CENTISECONDS, Vec2};
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
    pixels_per_meter: f32,
    texture_scale: f32,
    sky_x: f32,
    front_grass_rows: f32,
    // 1 to tile textures from the canvas origin like the game does at each zoom, 0 to keep
    // each tile where it is at zoom 1
    game_textures: u32,
    // In the background slot, which the present pass reads: 1 when the frame draws translucent
    // ghosts into their own layer, which is otherwise left out
    translucent_ghosts: u32,
}

// A rectangle of the timer or the info panel's text, in view pixels from the bottom left: the
// text in a color of the palette, the timer as TIMER_COLOR
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub(crate) struct HudRect {
    pub rect: [f32; 4],
    pub color: u32,
}

// Each pixel mapped to the brightest or darkest color by the LGR's timer palette map
pub(crate) const TIMER_COLOR: u32 = u32::MAX;

impl HudRect {
    fn new(r: PixelRect, color: u32) -> HudRect {
        HudRect {
            rect: [r.x as f32, r.y as f32, r.width as f32, r.height as f32],
            color,
        }
    }
}

// What one view draws, as ranges of the per-frame buffers
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ViewDraw {
    // What covers the ground and the sky
    pub(crate) ground_fill: Range<u32>,
    pub(crate) sky_fill: Range<u32>,
    pub(crate) dynamic: Range<u32>,
    pub(crate) ghosts: Range<u32>,
    pub(crate) timer: Range<u32>,
    pub(crate) minimap: Option<MinimapDraw>,
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
}

// What a frame draws. Kept between frames so its buffers are reused.
#[derive(Debug, Default)]
pub struct FramePlan {
    // A ViewUniform per slot, UNIFORM_STRIDE bytes apart
    pub(crate) uniforms: Vec<u8>,
    pub(crate) instances: Vec<Instance>,
    pub(crate) timer: Vec<HudRect>,
    // The part of the target the timer's pixels are worked out from, as x, y, width and
    // height from the top left: only it is copied for the timer pass to read
    pub(crate) timer_copy: Option<[u32; 4]>,
    pub(crate) views: Vec<ViewDraw>,
    // The instance of the frame behind the views, None when they cover the target
    pub(crate) background: Option<u32>,
    pub(crate) translucent_ghosts: bool,
}

fn view_uniform(
    scene: &Scene,
    session: &Session,
    params: &ViewParams,
    target_width: u32,
    textures: TextureZoom,
) -> ViewUniform {
    let [vx, vy, vw, vh] = params.viewport;
    let ppm = M2P * params.zoom;
    let corner = view_corner(session, params, target_width);
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
        pixels_per_meter: ppm as f32,
        texture_scale: params.zoom as f32,
        sky_x: sky_x as f32,
        front_grass_rows: params.zoom.round().max(1.0) as f32,
        game_textures: (textures == TextureZoom::Game) as u32,
        translucent_ghosts: 0,
    }
}

// The bounding box in the target of the rects drawn in TIMER_COLOR, the only ones that read
// what is under them
fn timer_copy_bounds(
    rects: &[HudRect],
    views: &[ViewParams],
    draws: &[ViewDraw],
    (width, height): (u32, u32),
) -> Option<[u32; 4]> {
    let mut bounds: Option<[f32; 4]> = None;
    for (params, draw) in views.iter().zip(draws) {
        let [vx, vy, _, vh] = params.viewport.map(|v| v as f32);
        for r in &rects[draw.timer.start as usize..draw.timer.end as usize] {
            if r.color != TIMER_COLOR {
                continue;
            }
            let [x, y, w, h] = r.rect;
            // From the view's bottom left to the target's top left
            let (left, top) = (vx + x, vy + vh - (y + h));
            let b = bounds.get_or_insert([left, top, left + w, top + h]);
            *b = [
                b[0].min(left),
                b[1].min(top),
                b[2].max(left + w),
                b[3].max(top + h),
            ];
        }
    }
    let [left, top, right, bottom] = bounds?;
    let clamp = |v: f32, max: u32| (v.max(0.0) as u32).min(max);
    let (x0, y0) = (clamp(left.floor(), width), clamp(top.floor(), height));
    let (x1, y1) = (clamp(right.ceil(), width), clamp(bottom.ceil(), height));
    (x1 > x0 && y1 > y0).then_some([x0, y0, x1 - x0, y1 - y0])
}

// Whether `views` cover every pixel of the target. Their edges cut it into cells that are
// each inside a view or outside them all.
fn covers(views: &[ViewParams], (width, height): (u32, u32)) -> bool {
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
    session: &Session,
    views: &[ViewParams],
    options: &RenderOptions,
    (width, height): (u32, u32),
) -> Result<(), RenderError> {
    if views.len() > MAX_VIEWS {
        return Err(RenderError::TooManyViews(views.len()));
    }
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
    for (i, params) in views.iter().enumerate() {
        let u = view_uniform(scene, session, params, width, options.textures);
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
        // Following a ghost draws the main bikes as when following the first, and the
        // apples and timer are the ghost's
        let count = camera::bike_count(session);
        let ghost_index = (params.follow.min(count - 1)).checked_sub(session.riders.len());
        let followed_ghost = ghost_index.map(|i| &session.ghosts[i]);
        let rider = if ghost_index.is_some() {
            0
        } else {
            params.follow
        };
        let ghosts = dynamic_instances(
            scene,
            session,
            rider,
            ghost_index,
            &options.dynamic,
            scratch,
        );
        // The timer stops when the followed replay ends, even if other bikes go on, and then
        // shows its time
        let shown = match followed_ghost {
            Some(ghost) => ghost.timer_centiseconds(session.time),
            None => session.main_timer_centiseconds(session.time),
        };

        let timer_start = timer_data.len() as u32;
        if params.timer {
            let centiseconds = |t: f64| (t * TIME_TO_CENTISECONDS) as i64;
            let flag_time = if rider == 0 {
                session.flag_time_a
            } else {
                session.flag_time_b
            };
            let flagtag =
                (session.flagtag && session.riders.len() > 1).then(|| centiseconds(flag_time));
            let rects = timer_rects(
                params.viewport[2] as i32,
                params.viewport[3] as i32,
                params.best_time,
                flagtag,
                shown,
                matches!(scene.look, Look::Across(_)),
                hud_scale,
            );
            timer_data.extend(rects.iter().map(|&r| HudRect::new(r, TIMER_COLOR)));
        }
        if params.last_apple_time && !matches!(scene.look, Look::Across(_)) {
            let (apples, last) = session.apples_taken(camera::followed(session, params.follow));
            if let (true, Some(last)) = (apples > 0, last) {
                let rects = last_apple_rects(
                    params.viewport[2] as i32,
                    apples,
                    (last * TIME_TO_CENTISECONDS) as i64,
                    hud_scale,
                );
                timer_data.extend(rects.iter().map(|r| HudRect::new(r.rect, r.color as u32)));
            }
        }
        let dynamic_end = scratch.len() as u32;
        let minimap = if params.minimap {
            let rider = camera::followed(session, params.follow);
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
                        front_grass_rows: 1.0,
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
                crate::minimap::icons(
                    m,
                    &p,
                    session,
                    rider.pose.bike_r,
                    &|i| session.objects[i].active,
                    session.time,
                    scratch,
                );
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
        draws.push(ViewDraw {
            minimap,
            ground_fill: ground_start..sky_start,
            sky_fill: sky_start..dynamic_start,
            dynamic: dynamic_start..dynamic_end,
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
            front_grass_rows: 1.0,
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
    frame.timer_copy = timer_copy_bounds(&frame.timer, views, &frame.views, (width, height));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::SceneOptions;
    use elma_formats::Graphics;
    use elma_formats::lev::{
        Gravity, Level, Object, ObjectType, Polygon, TopTens, Vec2 as LevVec2,
    };
    use elma_formats::lgr::Lgr;
    use replay::Settings;
    use std::sync::Arc;

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
        plan(
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
    fn only_the_timers_are_copied_for_the_timer_pass() {
        let (scene, session) = setup(0);
        assert_eq!(
            planned(&scene, &session, &[view(0, false, false)]).timer_copy,
            None
        );
        let mut top = view(0, true, false);
        top.viewport = [0, 0, 640, 240];
        let mut bottom = top;
        bottom.viewport = [0, 240, 640, 240];
        let frame = planned(&scene, &session, &[top, bottom]);
        let [x, y, w, h] = frame.timer_copy.unwrap();
        assert!(w * h < 640 * 480 / 4, "{w}x{h}");
        // Every timer pixel of both views, in the target from its top left
        for (params, draw) in [top, bottom].iter().zip(&frame.views) {
            let [vx, vy, _, vh] = params.viewport.map(|v| v as f32);
            for r in &frame.timer[draw.timer.start as usize..draw.timer.end as usize] {
                let [rx, ry, rw, rh] = r.rect;
                let (left, top) = (vx + rx, vy + vh - (ry + rh));
                assert!(left >= x as f32 && left + rw <= (x + w) as f32, "{r:?}");
                assert!(top >= y as f32 && top + rh <= (y + h) as f32, "{r:?}");
            }
        }
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
            plan(&mut frame, &scene, &session, &views, &options, (640, 480)).unwrap();
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
        assert!(pictures.instances.iter().any(|i| i.layer == layer::FRONT));
        assert!(!pictures.front_grass);
    }

    #[test]
    fn too_many_views_is_an_error() {
        let (scene, session) = setup(0);
        let views = vec![view(0, false, false); MAX_VIEWS + 1];
        let mut frame = FramePlan::default();
        let options = RenderOptions::default();
        assert_eq!(
            plan(&mut frame, &scene, &session, &views, &options, (640, 480)),
            Err(RenderError::TooManyViews(MAX_VIEWS + 1))
        );
    }

    #[test]
    fn planning_again_reuses_the_buffers() {
        let (scene, session) = setup(1);
        let mut frame = FramePlan::default();
        let views = [view(0, true, true)];
        let options = RenderOptions::default();
        plan(&mut frame, &scene, &session, &views, &options, (640, 480)).unwrap();
        let (instances, capacity) = (frame.instances.len(), frame.instances.capacity());
        plan(&mut frame, &scene, &session, &views, &options, (640, 480)).unwrap();
        assert_eq!(frame.instances.len(), instances);
        assert_eq!(frame.instances.capacity(), capacity);
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
            let u = view_uniform(&scene, &session, &params, 640, textures);
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
