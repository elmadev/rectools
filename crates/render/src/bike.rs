use crate::atlas::AtlasRect;
use crate::instance::{BIKE_DISTANCE, Instance, snap};
use replay::{Animation, Pose, Vec2, circles_intersection};
use std::f64::consts::PI;

// Atlas locations of one bike's pictures, with their size and transparent index
#[derive(Debug, Clone, Copy)]
pub struct Sprite {
    pub rect: AtlasRect,
    pub transparency: u8,
}

#[derive(Debug, Clone, Copy)]
pub struct BikeSprites {
    pub parts: [Sprite; 4],
    pub body: Sprite,
    pub thigh: Sprite,
    pub leg: Sprite,
    pub wheel: Sprite,
    pub susp1: Sprite,
    pub susp2: Sprite,
    pub forarm: Sprite,
    pub up_arm: Sprite,
    pub head: Sprite,
}

const WHEEL_RENDER_RADIUS: f64 = 0.395;
const HEAD_RADIUS: f64 = 0.238;
const BIKE_FRAME_X: f64 = 390.0;
const BIKE_FRAME_Y: f64 = 420.0;
const BIKE_FRAME_ROTATION: f64 = 0.62;
const BIKE_FRAME_WIDTH: f64 = 0.0045;
const THIGH_LENGTH: f64 = 0.51;
const LEG_LENGTH: f64 = 0.51;
const FORARM_LENGTH: f64 = 0.308 * 1.05;
const UP_ARM_LENGTH: f64 = 0.328 * 1.05;

pub(crate) struct Stretch {
    pub center: Vec2,
    pub axis: Vec2,
    pub factor: f64,
}

impl Stretch {
    fn point(&self, r: Vec2) -> Vec2 {
        r - self.axis * ((r - self.center).dot(self.axis) * (1.0 - self.factor))
    }

    fn vector(&self, u: Vec2) -> Vec2 {
        u - self.axis * (u.dot(self.axis) * (1.0 - self.factor))
    }
}

pub(crate) struct Drawer<'a> {
    pub out: &'a mut Vec<Instance>,
    pub stretch: Option<Stretch>,
}

impl Drawer<'_> {
    // The game spans u and v from the center of the first texel to the center of the
    // last one, so the picture reaches half a texel further on each side.
    pub fn affine(&mut self, sprite: Sprite, u: Vec2, v: Vec2, r: Vec2) {
        let (mut r, mut u, mut v) = (r, u, v);
        if let Some(stretch) = &self.stretch {
            r = stretch.point(r);
            u = stretch.vector(u);
            v = stretch.vector(v);
        }
        let w = sprite.rect.width.max(2) as f64;
        let h = sprite.rect.height.max(2) as f64;
        let du = u * (1.0 / (w - 1.0));
        let dv = v * (1.0 / (h - 1.0));
        let origin = r - du * 0.5 - dv * 0.5;
        let instance = Instance::new(
            [origin.x, origin.y],
            [du.x * w, du.y * w],
            [dv.x * h, dv.y * h],
        )
        .image(sprite.rect, Some(sprite.transparency))
        .snap(snap::EXACT)
        .distance(BIKE_DISTANCE);
        self.out.push(instance);
    }

    // A limb from `a` to `b` (pictures are stored sideways in the LGR)
    #[allow(clippy::too_many_arguments)]
    pub fn limb(
        &mut self,
        a: Vec2,
        b: Vec2,
        height: f64,
        sprite: Sprite,
        a_stretch: f64,
        b_stretch: f64,
        flip: bool,
    ) {
        let i = (b - a).unit();
        let b = b + i * b_stretch;
        let a = a - i * a_stretch;
        let u = b - a;
        let v = if flip {
            i.rotate_90() * height
        } else {
            i.rotate_minus_90() * height
        };
        self.affine(sprite, u, v * 2.0, a - v);
    }

    pub fn rigidbody(&mut self, r: Vec2, radius: f64, rotation: f64, sprite: Sprite, flip: bool) {
        let direction = Vec2::new(rotation.cos() * radius, rotation.sin() * radius);
        if flip {
            self.limb(r + direction, r - direction, radius, sprite, 0.0, 0.0, flip);
        } else {
            self.limb(r - direction, r + direction, radius, sprite, 0.0, 0.0, flip);
        }
    }
}

pub struct BikeDraw<'a> {
    pub pose: &'a Pose,
    pub animation: &'a Animation,
    pub sprites: &'a BikeSprites,
    pub shirt: Option<Sprite>,
    pub flag: Option<Sprite>,
}

// Port of render_bike: the same parts, positions and order
pub fn draw_bike(bike: &BikeDraw, out: &mut Vec<Instance>) {
    let mot = bike.pose;
    let sprites = bike.sprites;
    let mut d = Drawer { out, stretch: None };

    let mut turn_phase = bike.animation.bike_turning.turn_phase;
    let mut is_turning = false;
    if turn_phase < 0.999 {
        is_turning = true;
        turn_phase = -(turn_phase * PI).cos();
    }

    let mut left_wheel_r = mot.left_wheel_r;
    let mut right_wheel_r = mot.right_wheel_r;

    // While turning, one wheel is drawn in front of the bike
    let mut left_wheel_in_back = true;
    let mut right_wheel_in_back = true;
    if is_turning {
        if (turn_phase > 0.0 && !mot.flipped) || (turn_phase <= 0.0 && mot.flipped) {
            left_wheel_in_back = false;
        } else {
            right_wheel_in_back = false;
        }
    }

    if left_wheel_in_back {
        d.rigidbody(
            left_wheel_r,
            WHEEL_RENDER_RADIUS,
            mot.left_wheel_rotation,
            sprites.wheel,
            false,
        );
    }
    if right_wheel_in_back {
        d.rigidbody(
            right_wheel_r,
            WHEEL_RENDER_RADIUS,
            mot.right_wheel_rotation,
            sprites.wheel,
            false,
        );
    }

    let bike_r = mot.bike_r;
    let mut bike_i = Vec2::new(mot.bike_rotation.cos(), mot.bike_rotation.sin());
    let bike_j = bike_i.rotate_90();

    if is_turning {
        d.stretch = Some(Stretch {
            center: bike_r,
            axis: bike_i.unit(),
            factor: turn_phase,
        });
    }

    // Swap the wheels for the suspension when flipped
    if mot.flipped {
        bike_i = -bike_i;
        std::mem::swap(&mut left_wheel_r, &mut right_wheel_r);
    }

    let frame_i = bike_i * (BIKE_FRAME_WIDTH * BIKE_FRAME_ROTATION.cos())
        + bike_j * (BIKE_FRAME_WIDTH * BIKE_FRAME_ROTATION.sin());
    let mut frame_j = frame_i.rotate_90();
    if mot.flipped {
        frame_j = -frame_j;
    }
    let frame =
        |px: f64, py: f64| frame_i * (px - BIKE_FRAME_X) + frame_j * (BIKE_FRAME_Y - py) + bike_r;

    let susp1_r = frame(365.0, 292.0);
    d.limb(
        left_wheel_r,
        susp1_r,
        0.06,
        sprites.susp1,
        0.05,
        0.03,
        false,
    );
    let susp2_r = frame(370.0, 520.0);
    d.limb(susp2_r, right_wheel_r, 0.06, sprites.susp2, 0.0, 0.1, false);

    if let Some(flag) = bike.flag {
        let flag_base_r = frame_i * (500.0 + 107.0 - BIKE_FRAME_X)
            + frame_j * (BIKE_FRAME_Y + 114.0 - 600.0)
            + bike_r;
        let flag_tip_r = flag_base_r + (frame_i * 356.0 + frame_j * 500.0) * 0.2;
        d.limb(flag_base_r, flag_tip_r, 0.2, flag, 0.0, 0.0, mot.flipped);
    }

    let transparency = sprites.parts[0].transparency;
    for (part, &[x1, y1, x2, y2]) in sprites.parts.iter().zip(&elma_formats::lgr::BIKE_BOXES) {
        let (x1, y1, x2, y2) = (x1 as f64, y1 as f64, x2 as f64, y2 as f64);
        let r = frame_i * (x1 + 260.0 - BIKE_FRAME_X)
            + frame_j * (BIKE_FRAME_Y - (y1 + 260.0))
            + bike_r;
        let u = frame_i * (x2 - x1);
        let v = frame_j * (y1 - y2);
        d.affine(
            Sprite {
                rect: part.rect,
                transparency,
            },
            u,
            v,
            r,
        );
    }

    let body_r = mot.body_r;
    let hip_r = body_r + frame_i * 75.0 + frame_j * -47.0;
    let shoulder_r = body_r + frame_i * 47.0 + frame_j * 65.0;
    let neck_r = body_r + frame_i * 41.0 + frame_j * 70.0;
    let foot_r = frame(346.0, 514.0);

    let knee_r = if mot.flipped {
        circles_intersection(hip_r, foot_r, THIGH_LENGTH, LEG_LENGTH)
    } else {
        circles_intersection(foot_r, hip_r, LEG_LENGTH, THIGH_LENGTH)
    };

    d.rigidbody(
        mot.head_r,
        HEAD_RADIUS,
        mot.bike_rotation,
        sprites.head,
        mot.flipped,
    );

    // The hand is on the handlebars, unless volting
    let mut hand_r = susp1_r;
    let mut arm_position = bike.animation.arm_position;
    if arm_position > 0.0001 {
        arm_position = 1.0 - arm_position;
        let volt_is_right = bike.animation.volt_is_right;
        let arm_goes_up = !((volt_is_right && !mot.flipped) || (!volt_is_right && mot.flipped));
        let apex = if arm_goes_up { 0.25 } else { 0.2 };
        let max_rotation = if arm_goes_up { 2.7 } else { -1.6 };
        let max_stretch = if arm_goes_up { -0.3 } else { 0.15 };
        let interpolation = if arm_position < apex {
            arm_position / apex
        } else {
            1.0 - (arm_position - apex) / (1.0 - apex)
        };
        let rotation = max_rotation * interpolation;
        let stretch = max_stretch * interpolation + 1.0;
        let mut arm = hand_r - shoulder_r;
        arm = if mot.flipped {
            arm.rotated(rotation)
        } else {
            arm.rotated(-rotation)
        };
        hand_r = shoulder_r + arm * stretch;
    }

    let elbow_r = if mot.flipped {
        circles_intersection(hand_r, shoulder_r, FORARM_LENGTH, UP_ARM_LENGTH)
    } else {
        circles_intersection(shoulder_r, hand_r, UP_ARM_LENGTH, FORARM_LENGTH)
    };

    d.limb(knee_r, hip_r, 0.14, sprites.thigh, 0.03, 0.1, mot.flipped);
    d.limb(foot_r, knee_r, 0.21, sprites.leg, 0.03, 0.03, mot.flipped);
    let body = bike.shirt.unwrap_or(sprites.body);
    d.limb(hip_r, neck_r, 0.2, body, 0.1, 0.05, mot.flipped);
    d.limb(
        elbow_r,
        shoulder_r,
        0.11,
        sprites.up_arm,
        0.08,
        0.1,
        !mot.flipped,
    );
    d.limb(
        hand_r,
        elbow_r,
        0.076,
        sprites.forarm,
        0.08,
        0.1,
        mot.flipped,
    );

    d.stretch = None;
    if !left_wheel_in_back || !right_wheel_in_back {
        if mot.flipped {
            std::mem::swap(&mut left_wheel_r, &mut right_wheel_r);
        }
        if !left_wheel_in_back {
            d.rigidbody(
                left_wheel_r,
                WHEEL_RENDER_RADIUS,
                mot.left_wheel_rotation,
                sprites.wheel,
                false,
            );
        }
        if !right_wheel_in_back {
            d.rigidbody(
                right_wheel_r,
                WHEEL_RENDER_RADIUS,
                mot.right_wheel_rotation,
                sprites.wheel,
                false,
            );
        }
    }
}
