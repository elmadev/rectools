use crate::plan::RenderError;
// Across's look (KIRAJ320.CPP, KORNY.CPP): a big sky picture that moves a little with the
// camera, clouds, one ground texture, objects of any size and its own bike. Across draws at
// 100 pixels per meter at 640x480, which the sizes here follow.

use crate::atlas::{AtlasBuilder, AtlasRect};
use crate::bike::{Drawer, Sprite, Stretch};
use crate::instance::{BIKE_DISTANCE, Instance, M2P, snap};
use crate::scene::{Look, Scene, TextureRef};
use elma_formats::across::gfx::{AcrossGraphics, Picture};
use elma_formats::lev::{Level, ObjectType};
use elma_formats::pic::Pic8;
use replay::{Animation, LevelObject, Pose, Vec2, circles_intersection};
use std::f64::consts::PI;

// Across pixels per meter, at 640x480
const PIXELS_PER_METER: f64 = 100.0;
// The room the game leaves around the level, in its pixels
const MARGIN: (f64, f64) = (400.0, 300.0);
// The sky never moves faster than this against the camera
const MAX_SKY_SPEED: f64 = 0.1;
const CLOUD_SPEED: f64 = 0.5;
// The height the sky and clouds are made for
const SCREEN_HEIGHT: f64 = 480.0;

const WHEEL_RADIUS: f64 = 0.4;
const HEAD_RADIUS: f64 = 0.238;
const HEAD_ANGLE: f64 = 0.2;
const FRAME_X: f64 = 390.0;
const FRAME_Y: f64 = 420.0;
const FRAME_ROTATION: f64 = 0.62;
const FRAME_WIDTH: f64 = 0.0045;
const FORARM_LENGTH: f64 = 0.385;
const UP_ARM_LENGTH: f64 = 0.41;
// The body parts, as rectangles of the drawing the bike is laid out in
const PARTS: [[f64; 4]; 5] = [
    [272.0, 275.0, 347.0, 334.0],
    [287.0, 334.0, 404.0, 558.0],
    [404.0, 369.0, 451.0, 530.0],
    [451.0, 289.0, 568.0, 538.0],
    [568.0, 474.0, 626.0, 529.0],
];

#[derive(Debug, Clone, Copy)]
pub struct BikeSprites {
    pub wheel: Sprite,
    pub parts: [Sprite; 5],
    pub fork: Sprite,
    pub rear_arm: Sprite,
    pub forearm: Sprite,
    pub upper_arm: Sprite,
    pub head: Sprite,
}

#[derive(Debug, Clone)]
pub struct ObjectSprites {
    pub frames: Vec<AtlasRect>,
    pub transparency: u8,
    pub radius: f64,
    pub frame_speed: f64,
}

#[derive(Debug, Clone)]
pub struct AcrossSprites {
    pub bike: BikeSprites,
    // By object type
    pub objects: Vec<(ObjectType, ObjectSprites)>,
    pub sky: AtlasRect,
    pub clouds: Option<Sprite>,
    // The corner of the area the sky spans, and its size in Across pixels
    pub origin: Vec2,
    pub extent: Vec2,
}

fn sprite(atlas: &mut AtlasBuilder, picture: &Picture) -> Sprite {
    Sprite {
        rect: atlas.add(&picture.pic),
        transparency: picture.transparency,
    }
}

// A level with Across's graphics. `descriptor` picks the ground, and the clouds for 0.
pub fn scene(
    gfx: &AcrossGraphics,
    level: &Level,
    descriptor: u8,
    max_atlas_size: usize,
) -> Result<Scene, RenderError> {
    let largest = gfx.sky.width.max(gfx.sky.height);
    let atlas_size = crate::scene::atlas_size(largest, max_atlas_size)?;
    let mut atlas = AtlasBuilder::new(atlas_size);

    let bike = &gfx.bike;
    let bike = BikeSprites {
        wheel: sprite(&mut atlas, &bike.wheel),
        parts: bike.parts.each_ref().map(|p| sprite(&mut atlas, p)),
        fork: sprite(&mut atlas, &bike.fork),
        rear_arm: sprite(&mut atlas, &bike.rear_arm),
        forearm: sprite(&mut atlas, &bike.forearm),
        upper_arm: sprite(&mut atlas, &bike.upper_arm),
        head: sprite(&mut atlas, &bike.head),
    };
    let objects = gfx
        .objects
        .iter()
        .map(|o| {
            let sprites = ObjectSprites {
                frames: o.frames.iter().map(|f| atlas.add(&f.pic)).collect(),
                transparency: o.frames[0].transparency,
                radius: o.radius,
                frame_speed: o.frame_speed,
            };
            (o.kind, sprites)
        })
        .collect();
    let sky = atlas.add(&gfx.sky);
    let clouds = (descriptor == 0).then(|| sprite(&mut atlas, &gfx.clouds));
    let ground = &gfx.grounds[(descriptor as usize).min(2)];
    let foreground = TextureRef {
        rect: atlas.add(&ground.pic),
        size: (ground.size.0 * M2P, ground.size.1 * M2P),
    };
    let qframe = TextureRef {
        rect: atlas.add(&Pic8::filled(1, 1, gfx.frame_color)),
        size: (1.0, 1.0),
    };

    let (min, max) = crate::scene::ground_bounds(level);
    let margin = Vec2::new(MARGIN.0, MARGIN.1) * (1.0 / PIXELS_PER_METER);
    let origin = min - margin;
    let extent = (max + margin - origin) * PIXELS_PER_METER;

    Ok(Scene {
        atlas_size: atlas.layer_size(),
        atlas_layers: atlas.into_layers(),
        // Across has no shirts to find room for
        packer: Default::default(),
        palette: gfx.palette,
        // One color, unlike Elma's timer that stands out from what is behind it (DIGIT.CPP)
        timer_map: [gfx.timer_color; 256],
        // Textures start at the corner of the area the game draws
        canvas_origin: origin,
        polygon_vertices: crate::scene::polygon_fans(level),
        foreground,
        qframe,
        minimap: crate::minimap::Minimap::across(gfx, descriptor),
        look: Look::Across(Box::new(AcrossSprites {
            bike,
            objects,
            sky,
            clouds,
            origin,
            extent,
        })),
    })
}

// The sky and clouds of a view whose bottom left corner is `corner`, `size` pixels at `ppm`
// pixels per meter. Across pins them to the screen and scrolls them slower than the level, so
// that the sky's edges show when the camera is at the level's.
pub fn sky_instances(
    sprites: &AcrossSprites,
    corner: Vec2,
    size: (f64, f64),
    ppm: f64,
    out: &mut Vec<Instance>,
) {
    let sky = (sprites.sky.width as f64, sprites.sky.height as f64);
    // Screen pixels per sky pixel, so the sky fills the view like at 640x480
    let mut scale = (size.1 / SCREEN_HEIGHT).max(1.0);
    scale = scale
        .max(size.0 / (sky.0 - 4.0))
        .max(size.1 / (sky.1 - 4.0));
    let display = (size.0 / scale, size.1 / scale);

    // Port of hatterszamitas
    let extent = sprites.extent;
    let speed = MAX_SKY_SPEED
        .min((sky.0 - display.0 - 4.0) / extent.x.max(100.0))
        .min((sky.1 - display.1 - 4.0) / extent.y.max(100.0))
        .max(0.0);
    let base = (
        sky.0 / 2.0 - (speed * extent.x / 2.0 + display.0 / 2.0),
        sky.1 / 2.0 - (speed * extent.y / 2.0 + display.1 / 2.0),
    );
    let camera = (corner - sprites.origin) * PIXELS_PER_METER;
    let offset = (
        (base.0 + speed * camera.x).clamp(0.0, sky.0 - display.0),
        (base.1 + speed * camera.y).clamp(0.0, sky.1 - display.1),
    );

    // Screen pixels to world meters, for an image whose first row is at the bottom like the
    // game's frame buffer
    let texel = scale / ppm;
    let image = |rect: AtlasRect, x: f64, y: f64, transparency: Option<u8>| {
        let (w, h) = (rect.width as f64 * texel, rect.height as f64 * texel);
        let origin = [corner.x + x * texel, corner.y + y * texel];
        Instance::new(origin, [w, 0.0], [0.0, h])
            .image(rect, transparency)
            .snap(snap::EXACT)
            .distance(2000.0)
    };
    out.push(image(sprites.sky, -offset.0, -offset.1, None));

    // Port of felhokfel
    if let Some(clouds) = sprites.clouds {
        let (w, h) = (clouds.rect.width as f64, clouds.rect.height as f64);
        let start = (
            (-camera.x * CLOUD_SPEED).rem_euclid(w) - w,
            (-camera.y * CLOUD_SPEED).rem_euclid(h) - h,
        );
        let mut x = start.0;
        while x < display.0 {
            let mut y = start.1;
            while y < display.1 {
                out.push(image(clouds.rect, x, y, Some(clouds.transparency)));
                y += h;
            }
            x += w;
        }
    }
}

// The objects that are still there, where they have moved to by `time` and turning through
// their frames
pub fn object_instances(
    sprites: &AcrossSprites,
    objects: &[LevelObject],
    active: impl Fn(usize, &LevelObject) -> bool,
    time: f64,
    animated: bool,
    out: &mut Vec<Instance>,
) {
    for (i, object) in objects.iter().enumerate() {
        if object.kind == ObjectType::Start || !active(i, object) {
            continue;
        }
        let Some((_, s)) = sprites
            .objects
            .iter()
            .find(|(kind, _)| *kind == object.kind)
        else {
            continue;
        };
        let frame = if animated {
            ((time * s.frame_speed) as i64).rem_euclid(s.frames.len() as i64) as usize
        } else {
            0
        };
        let r = object.position(time);
        let size = 2.0 * s.radius;
        out.push(
            Instance::new([r.x - s.radius, r.y + s.radius], [size, 0.0], [0.0, -size])
                .image(s.frames[frame], Some(s.transparency))
                .snap(snap::EXACT)
                .distance(BIKE_DISTANCE),
        );
    }
}

// Port of the bike part of kirajzol320
pub fn draw_bike(
    pose: &Pose,
    animation: &Animation,
    sprites: &BikeSprites,
    out: &mut Vec<Instance>,
) {
    let mut d = Drawer { out, stretch: None };

    let mut turn_phase = animation.bike_turning.turn_phase;
    let turning = turn_phase < 0.999;
    if turning {
        turn_phase = -(turn_phase * PI).cos();
    }
    let flipped = pose.flipped;
    let (mut front, mut back) = (pose.left_wheel_r, pose.right_wheel_r);

    // While turning, one wheel is drawn over the bike
    let (mut front_behind, mut back_behind) = (true, true);
    if turning {
        if (turn_phase > 0.0 && !flipped) || (turn_phase <= 0.0 && flipped) {
            front_behind = false;
        } else {
            back_behind = false;
        }
    }
    let wheel = |d: &mut Drawer, r: Vec2, rotation: f64| {
        d.rigidbody(r, WHEEL_RADIUS, rotation, sprites.wheel, false);
    };
    if front_behind {
        wheel(&mut d, front, pose.left_wheel_rotation);
    }
    if back_behind {
        wheel(&mut d, back, pose.right_wheel_rotation);
    }

    let center = pose.bike_r;
    let mut right = Vec2::new(pose.bike_rotation.cos(), pose.bike_rotation.sin());
    let up = right.rotate_90();
    if turning {
        d.stretch = Some(Stretch {
            center,
            axis: right.unit(),
            factor: turn_phase,
        });
    }
    if flipped {
        right = -right;
        std::mem::swap(&mut front, &mut back);
    }

    let frame_i =
        right * (FRAME_WIDTH * FRAME_ROTATION.cos()) + up * (FRAME_WIDTH * FRAME_ROTATION.sin());
    let mut frame_j = frame_i.rotate_90();
    if flipped {
        frame_j = -frame_j;
    }
    let frame = |x: f64, y: f64| frame_i * (x - FRAME_X) + frame_j * (FRAME_Y - y) + center;

    // The handlebar, a little further out along the fork
    let handlebar = front + (frame(336.0, 335.0) - front) * 1.235;
    d.limb(front, handlebar, 0.06, sprites.fork, 0.05, 0.03, false);
    d.limb(
        frame(370.0, 520.0),
        back,
        0.06,
        sprites.rear_arm,
        0.0,
        0.1,
        false,
    );

    for (part, &[x1, y1, x2, y2]) in sprites.parts.iter().zip(&PARTS) {
        let r = frame(x1, y1);
        d.affine(*part, frame_i * (x2 - x1), frame_j * (y1 - y2), r);
    }

    let head_angle = if flipped { -HEAD_ANGLE } else { HEAD_ANGLE };
    d.rigidbody(
        pose.head_r,
        HEAD_RADIUS,
        pose.bike_rotation + head_angle,
        sprites.head,
        flipped,
    );

    // The hand is on the handlebar, unless volting
    let shoulder = frame(532.0, 320.0);
    let mut hand = handlebar;
    let mut arm_position = animation.arm_position;
    if arm_position > 0.0001 {
        arm_position = 1.0 - arm_position;
        let right_volt = animation.volt_is_right;
        let goes_up = !((right_volt && !flipped) || (!right_volt && flipped));
        let apex = if goes_up { 0.25 } else { 0.2 };
        let max_rotation = if goes_up { 2.7 } else { -1.6 };
        let max_stretch = if goes_up { -0.3 } else { 0.15 };
        let amount = if arm_position < apex {
            arm_position / apex
        } else {
            1.0 - (arm_position - apex) / (1.0 - apex)
        };
        let arm = hand - shoulder;
        let rotation = max_rotation * amount;
        let arm = if flipped {
            arm.rotated(rotation)
        } else {
            arm.rotated(-rotation)
        };
        hand = shoulder + arm * (max_stretch * amount + 1.0);
    }
    let elbow = if flipped {
        circles_intersection(hand, shoulder, FORARM_LENGTH, UP_ARM_LENGTH)
    } else {
        circles_intersection(shoulder, hand, UP_ARM_LENGTH, FORARM_LENGTH)
    };
    d.limb(
        elbow,
        shoulder,
        0.11,
        sprites.upper_arm,
        0.08,
        0.1,
        !flipped,
    );
    d.limb(hand, elbow, 0.076, sprites.forearm, 0.08, 0.1, flipped);

    d.stretch = None;
    if !front_behind || !back_behind {
        if flipped {
            std::mem::swap(&mut front, &mut back);
        }
        if !front_behind {
            wheel(&mut d, front, pose.left_wheel_rotation);
        }
        if !back_behind {
            wheel(&mut d, back, pose.right_wheel_rotation);
        }
    }
}
