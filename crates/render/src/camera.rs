use crate::instance::M2P;
use crate::options::{CameraMode, MAX_ZOOM, MIN_ZOOM, ViewParams};
use elma_formats::lev::Level;
use replay::{Rider, Session, Vec2};

// Bottom left corner of a view in world meters, following the game's camera unless the
// view has a fixed center. `screen_width` is the width of the whole output.
pub fn view_corner(session: &Session, params: &ViewParams, screen_width: u32) -> Vec2 {
    let (vw, vh) = (params.viewport[2] as f64, params.viewport[3] as f64);
    let ppm = pixels_per_meter(params.zoom);
    if let Some(center) = params.center {
        return center - Vec2::new(vw / ppm / 2.0, vh / ppm / 2.0);
    }
    let camera_x = match params.camera {
        CameraMode::Classic => match session.settings().game.camera_edge() {
            Some(edge) => (vw / ppm) * edge,
            None => (screen_width as f64 / ppm) * 0.15 * params.zoom,
        },
        CameraMode::Centered => (vw / ppm) * 0.5,
    };
    let camera_dx = vw / ppm - 2.0 * camera_x;
    let camera_y = vh / ppm / 2.0;
    let rider = followed(session, params.follow);
    let phase = rider.animation.camera_turning.turn_phase;
    let center = rider.pose.bike_r + params.pan;
    center - Vec2::new(camera_x + phase * camera_dx, camera_y)
}

pub(crate) fn view_center(session: &Session, params: &ViewParams, screen_width: u32) -> Vec2 {
    let ppm = pixels_per_meter(params.zoom);
    let size = Vec2::new(
        params.viewport[2] as f64 / ppm,
        params.viewport[3] as f64 / ppm,
    );
    view_corner(session, params, screen_width) + size * 0.5
}

// The bike `follow` points at: the main replay's riders, then the ghosts
pub fn followed(session: &Session, follow: usize) -> &Rider {
    let count = bike_count(session);
    let i = follow.min(count - 1);
    session
        .riders
        .get(i)
        .unwrap_or_else(|| &session.ghosts[i - session.riders.len()])
}

pub fn bike_count(session: &Session) -> usize {
    session.riders.len() + session.ghosts.len()
}

// Smallest and largest coordinates of `points`, None when there are none
pub(crate) fn bounds(points: impl Iterator<Item = Vec2>) -> Option<(Vec2, Vec2)> {
    points.fold(None, |acc, p| {
        let (min, max) = acc.unwrap_or((p, p));
        Some((
            Vec2::new(min.x.min(p.x), min.y.min(p.y)),
            Vec2::new(max.x.max(p.x), max.y.max(p.y)),
        ))
    })
}

// Every bike of the session, ghosts included
fn bikes(session: &Session) -> impl Iterator<Item = Vec2> + '_ {
    session
        .riders
        .iter()
        .chain(&session.ghosts)
        .map(|r| r.pose.bike_r)
}

// When a bike would be outside the view (closer than `margin` meters to its edge), the zoom
// and center that show them all. None when they are all in view already.
pub(crate) fn fit_bikes(
    session: &Session,
    params: &ViewParams,
    screen_width: u32,
    margin: f64,
) -> Option<(f64, Vec2)> {
    let ppm = pixels_per_meter(params.zoom);
    let (vw, vh) = (params.viewport[2] as f64, params.viewport[3] as f64);
    let corner = view_corner(session, params, screen_width);
    let far = corner + Vec2::new(vw / ppm, vh / ppm);
    let inside = |p: &Vec2| {
        p.x >= corner.x + margin
            && p.x <= far.x - margin
            && p.y >= corner.y + margin
            && p.y <= far.y - margin
    };
    if bikes(session).all(|p| inside(&p)) {
        return None;
    }
    let (min, max) = bounds(bikes(session))?;
    let width = max.x - min.x + 2.0 * margin;
    let height = max.y - min.y + 2.0 * margin;
    let zoom = (vw / (M2P * width))
        .min(vh / (M2P * height))
        .min(params.zoom);
    Some((zoom, (min + max) * 0.5))
}

// The largest zoom at which a view centered on `center` shows every bike with `margin`
// meters to spare
pub(crate) fn zoom_to_show(
    session: &Session,
    center: Vec2,
    viewport: [u32; 4],
    margin: f64,
) -> f64 {
    let (mut dx, mut dy) = (0.0f64, 0.0f64);
    for p in bikes(session) {
        dx = dx.max((p.x - center.x).abs());
        dy = dy.max((p.y - center.y).abs());
    }
    let (vw, vh) = (viewport[2] as f64, viewport[3] as f64);
    (vw / (2.0 * M2P * (dx + margin))).min(vh / (2.0 * M2P * (dy + margin)))
}

// Zooms out and moves the camera so every bike stays in view. Zooming out is immediate,
// so nothing is ever outside; moving the camera and zooming back in are eased.
#[derive(Debug, Default)]
pub struct AutoFrame {
    zoom: Option<f64>,
    // 0 follows the bike like normal, 1 looks at the middle of all bikes
    blend: f64,
}

impl AutoFrame {
    // Meters kept between a bike and the edge of the view
    const MARGIN: f64 = 2.5;

    // The zoom it showed last
    pub fn zoom(&self) -> Option<f64> {
        self.zoom
    }

    // `base` is the view the user asked for, `dt` the seconds since the last frame
    pub fn update(
        &mut self,
        session: &Session,
        base: ViewParams,
        screen_width: u32,
        dt: f64,
    ) -> ViewParams {
        let user_zoom = base.zoom;
        let ease = |seconds: f64| 1.0 - (-dt / seconds).exp();
        let engaged = fit_bikes(session, &base, screen_width, Self::MARGIN).is_some();
        self.blend += (if engaged { 1.0 } else { 0.0 } - self.blend) * ease(0.5);
        if !engaged && self.blend < 0.001 {
            self.blend = 0.0;
        }

        let (min, max) = bounds(bikes(session)).unwrap_or_default();
        let middle = (min + max) * 0.5;
        let center_at = |zoom: f64| {
            let normal = view_center(session, &ViewParams { zoom, ..base }, screen_width);
            normal + (middle - normal) * self.blend
        };
        let needed = |zoom: f64| {
            if self.blend == 0.0 {
                user_zoom
            } else {
                zoom_to_show(session, center_at(zoom), base.viewport, Self::MARGIN).min(user_zoom)
            }
        };

        let mut zoom = self.zoom.unwrap_or(user_zoom);
        let target = needed(zoom);
        zoom = if target <= zoom || self.blend == 0.0 {
            target
        } else {
            (zoom.ln() + (target.ln() - zoom.ln()) * ease(0.6)).exp()
        };
        // The center depends on the zoom, so check once more at the final one
        zoom = zoom.min(needed(zoom));
        self.zoom = Some(zoom);
        ViewParams {
            zoom,
            center: Some(center_at(zoom)),
            ..base
        }
    }
}

// The views of one frame: two for a split screen two player replay, otherwise one following
// `base.follow`, zoomed out by `auto` when it is given and there are several bikes. `base`'s
// viewport is replaced.
pub fn layout_views(
    session: &Session,
    base: ViewParams,
    (width, height): (u32, u32),
    split_screen: bool,
    auto: Option<(&mut AutoFrame, f64)>,
) -> Vec<ViewParams> {
    let riders = session.riders.len();
    let view = |follow, viewport| ViewParams {
        follow,
        viewport,
        ..base
    };
    if riders > 1 && split_screen {
        let half = (height / 2).saturating_sub(6).max(1);
        return vec![
            view(0, [0, 0, width, half]),
            view(1, [0, height - half, width, half]),
        ];
    }
    let single = view(
        base.follow.min(bike_count(session) - 1),
        [0, 0, width, height],
    );
    match auto {
        Some((frame, dt)) if riders + session.ghosts.len() > 1 => {
            vec![frame.update(session, single, width, dt)]
        }
        Some((frame, _)) => {
            *frame = AutoFrame::default();
            vec![single]
        }
        None => vec![single],
    }
}

// Worked out as the game does, which at some zooms is a bit off M2P * zoom, and then puts a
// picture or the canvas origin a pixel off where it is exactly on one
pub fn pixels_per_meter(zoom: f64) -> f64 {
    100.0 * (0.48 * zoom)
}

// Smallest and largest coordinates of a level's ground, y pointing up like the bikes
pub fn level_bounds(level: &Level) -> Option<(Vec2, Vec2)> {
    let points = level
        .polygons
        .iter()
        .filter(|p| !p.is_grass)
        .flat_map(|p| &p.vertices)
        .map(|v| Vec2::new(v.x, -v.y));
    bounds(points)
}

// The zoom and middle that show everything in `bounds` in `viewport`, with a little room
pub fn fit_bounds((min, max): (Vec2, Vec2), viewport: [u32; 4]) -> (f64, Vec2) {
    let size = (max - min) * 1.05;
    let (vw, vh) = (viewport[2] as f64, viewport[3] as f64);
    let zoom = (vw / (M2P * size.x.max(1.0))).min(vh / (M2P * size.y.max(1.0)));
    (zoom.clamp(MIN_ZOOM, MAX_ZOOM), (min + max) * 0.5)
}

// The zoom and middle that show the whole level
pub fn fit_level(level: &Level, viewport: [u32; 4]) -> Option<(f64, Vec2)> {
    Some(fit_bounds(level_bounds(level)?, viewport))
}

#[cfg(test)]
mod tests {
    use super::*;
    use elma_formats::lev::{Gravity, Object, ObjectType, Polygon, TopTens, Vec2 as LevVec2};
    use replay::Settings;

    fn level() -> Level {
        let v = |x, y| LevVec2 { x, y };
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
            objects: vec![Object {
                r: v(5.0, 8.0),
                kind: ObjectType::Start,
                gravity: Gravity::None,
                animation: 0,
                motion: None,
            }],
            sprites: vec![],
            topology_errors: false,
            top_tens: TopTens::default(),
        }
    }

    #[test]
    fn fitting_shows_the_whole_level_in_the_middle() {
        let level = level();
        let (zoom, center) = fit_level(&level, [0, 0, 800, 600]).unwrap();
        assert!((center.x - 20.0).abs() < 1e-9);
        assert!((center.y + 5.0).abs() < 1e-9);
        assert!(40.0 * 1.05 * M2P * zoom <= 800.0 + 1e-9);
    }

    #[test]
    fn follow_counts_the_ghosts_after_the_riders() {
        let level = level();
        let rec = replay::start_rec(&level);
        let mut ghost = rec.rides[0].clone();
        ghost.frames[0].bike_x += 10.0;
        let session = Session::new(
            &level,
            &rec,
            &[std::sync::Arc::new(ghost)],
            Settings::default(),
        );
        assert_eq!(bike_count(&session), 2);
        assert_eq!(
            followed(&session, 1).pose.bike_r,
            session.ghosts[0].pose.bike_r
        );
        assert_eq!(
            followed(&session, 9).pose.bike_r,
            session.ghosts[0].pose.bike_r
        );
    }
}
