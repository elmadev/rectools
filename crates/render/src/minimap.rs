// The minimap in the bottom right corner of the view, drawn as each game draws it: Elma's
// (elma-classic's render_minimap) and Across's preview (kirajzol320). Sizes are in pixels at
// 640x480 and grow with the view like the timer.

use crate::instance::{Instance, mode, snap};
use crate::world::{View, World};
use elma_formats::lev::ObjectType;
use replay::Vec2;

#[derive(Debug, Clone)]
pub struct Minimap {
    pub ground: u8,
    pub sky: u8,
    pub border: u8,
    pub bike: u8,
    pub other_bike: u8,
    // The dot of each kind of object, if it has one
    pub objects: Vec<(ObjectType, u8)>,
    pub size: (f64, f64),
    pub pixels_per_meter: f64,
    // Across keeps the bike where it is in the view, Elma at a fifth of the way across
    pub bike_like_view: bool,
}

impl Minimap {
    pub fn elma(colors: &elma_formats::lgr::MinimapColors) -> Minimap {
        Minimap {
            ground: colors.ground,
            sky: colors.sky,
            border: colors.border,
            bike: colors.bike,
            other_bike: colors.other_bike,
            objects: vec![
                (ObjectType::Food, colors.food),
                (ObjectType::Exit, colors.exit),
                (ObjectType::Killer, colors.killer),
            ],
            size: (140.0, 70.0),
            // 48 pixels per meter over the game's scale factor of 10
            pixels_per_meter: 4.8,
            bike_like_view: false,
        }
    }

    // Across's colors depend on the level's descriptor (LEIRO.CPP: foldbeolv_ratesz)
    pub fn across(gfx: &elma_formats::across::gfx::AcrossGraphics, descriptor: u8) -> Minimap {
        let (ground, bike) = match descriptor {
            0 => (39, 18),
            1 => (24, 7),
            _ => (98, 18),
        };
        Minimap {
            ground,
            sky: 127,
            border: gfx.frame_color,
            bike,
            other_bike: bike,
            objects: gfx
                .objects
                .iter()
                .filter(|o| o.kind != ObjectType::Start)
                .map(|o| (o.kind, o.minimap_color))
                .collect(),
            size: (200.0, 80.0),
            // A view pixel is 14 of the game's at 100 pixels per meter
            pixels_per_meter: 100.0 / 14.0,
            bike_like_view: true,
        }
    }
}

// Where a minimap goes in a view, in target pixels from the top left
pub(crate) struct Placement {
    pub inner: [u32; 4],
    pub border: [u32; 4],
    // Bottom left corner of the minimap in world meters
    pub corner: Vec2,
    pub pixels_per_meter: f64,
    // Target pixels per minimap pixel
    pub scale: f64,
}

// `viewport` and `view_corner` of the view it is drawn in, `anchor` the point it follows and
// `fraction` how far across the minimap that point is
pub(crate) fn place(
    minimap: &Minimap,
    viewport: [u32; 4],
    anchor: Vec2,
    fraction: f64,
) -> Option<Placement> {
    let [vx, vy, vw, vh] = viewport.map(|v| v as f64);
    let scale = (vh / 480.0).max(0.5);
    let (w, h) = (
        (minimap.size.0 * scale).round(),
        (minimap.size.1 * scale).round(),
    );
    // Fixed in the corner, where the games move it across with the camera
    let gap = scale.round().max(1.0);
    let (x, bottom) = (vw - gap - w, gap);
    if x < 1.0 || bottom + h + 1.0 > vh || w < 4.0 || h < 4.0 {
        return None;
    }
    let top = vh - bottom - h;
    let ppm = minimap.pixels_per_meter * scale;
    let corner = anchor - Vec2::new(fraction * w / ppm, 0.5 * h / ppm);
    let rect =
        |x: f64, y: f64, w: f64, h: f64| [(vx + x) as u32, (vy + y) as u32, w as u32, h as u32];
    Some(Placement {
        inner: rect(x, top, w, h),
        border: rect(x - 1.0, top - 1.0, w + 2.0, h + 2.0),
        corner,
        pixels_per_meter: ppm,
        scale,
    })
}

// A solid rectangle of `color` from `bottom_left`, `size` meters
fn solid(bottom_left: Vec2, size: Vec2, color: u8) -> Instance {
    Instance::new(
        [bottom_left.x, bottom_left.y + size.y],
        [size.x, 0.0],
        [0.0, -size.y],
    )
    .mode(mode::SOLID)
    .color(color)
    .distance(0.0)
}

pub(crate) fn fill(placement: &Placement, color: u8, border: bool) -> Instance {
    let p = placement;
    let pixel = 1.0 / p.pixels_per_meter;
    let size = Vec2::new(p.inner[2] as f64, p.inner[3] as f64) * pixel;
    if border {
        solid(
            p.corner - Vec2::new(pixel, pixel),
            size + Vec2::new(2.0 * pixel, 2.0 * pixel),
            color,
        )
    } else {
        solid(p.corner, size, color)
    }
    .snap(snap::PIXEL)
}

// A ring of 3x3 minimap pixels around `r`, without the middle one
fn ring(placement: &Placement, r: Vec2, color: u8, out: &mut Vec<Instance>) {
    let p = placement;
    let cell = p.scale / p.pixels_per_meter;
    let x = ((r.x - p.corner.x) / cell).floor();
    let y = ((r.y - p.corner.y) / cell).floor();
    let at = |dx: f64, dy: f64| p.corner + Vec2::new((x + dx) * cell, (y + dy) * cell);
    for (dx, dy, w, h) in [
        (-1.0, 1.0, 3.0, 1.0),
        (-1.0, -1.0, 3.0, 1.0),
        (-1.0, 0.0, 1.0, 1.0),
        (1.0, 0.0, 1.0, 1.0),
    ] {
        out.push(solid(at(dx, dy), Vec2::new(w * cell, h * cell), color).snap(snap::PIXEL));
    }
}

// The dots of the objects that are still there in `view` and the bikes, the followed one last
pub(crate) fn icons(
    minimap: &Minimap,
    placement: &Placement,
    world: &World,
    view: &View,
    out: &mut Vec<Instance>,
) {
    for (i, object) in world.objects.iter().enumerate() {
        if !view.object_there(i, object) || (object.kind == ObjectType::Exit && world.hide_exit) {
            continue;
        }
        if let Some(&(_, color)) = minimap.objects.iter().find(|(k, _)| *k == object.kind) {
            ring(placement, object.position(world.time), color, out);
        }
    }
    let followed = view.followed(world).map(|b| b.pose.bike_r);
    for bike in world.bikes {
        if Some(bike.pose.bike_r) != followed {
            ring(placement, bike.pose.bike_r, minimap.other_bike, out);
        }
    }
    if let Some(followed) = followed {
        ring(placement, followed, minimap.bike, out);
    }
}
