use std::ops::{Add, Mul, Neg, Sub};

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Vec2 {
    pub x: f64,
    pub y: f64,
}

impl Vec2 {
    pub const fn new(x: f64, y: f64) -> Vec2 {
        Vec2 { x, y }
    }

    pub fn dot(self, o: Vec2) -> f64 {
        self.x * o.x + self.y * o.y
    }

    // The game refines sqrt with one Newton step; keep it so results stay identical
    pub fn length(self) -> f64 {
        square_root(self.x * self.x + self.y * self.y)
    }

    pub fn unit(self) -> Vec2 {
        self * (1.0 / self.length())
    }

    pub fn rotate_90(self) -> Vec2 {
        Vec2::new(-self.y, self.x)
    }

    pub fn rotate_minus_90(self) -> Vec2 {
        Vec2::new(self.y, -self.x)
    }

    pub fn rotated(self, rotation: f64) -> Vec2 {
        let a = rotation.sin();
        let b = rotation.cos();
        Vec2::new(b * self.x - a * self.y, a * self.x + b * self.y)
    }
}

fn square_root(a: f64) -> f64 {
    let a = a.max(0.0);
    let x1 = a.sqrt();
    if x1 == 0.0 {
        return 0.0;
    }
    0.5 * (x1 + a / x1)
}

impl Add for Vec2 {
    type Output = Vec2;
    fn add(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x + o.x, self.y + o.y)
    }
}

impl Sub for Vec2 {
    type Output = Vec2;
    fn sub(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x - o.x, self.y - o.y)
    }
}

impl Mul<f64> for Vec2 {
    type Output = Vec2;
    fn mul(self, k: f64) -> Vec2 {
        Vec2::new(self.x * k, self.y * k)
    }
}

impl Neg for Vec2 {
    type Output = Vec2;
    fn neg(self) -> Vec2 {
        Vec2::new(-self.x, -self.y)
    }
}

// Where two circles meet, used to bend the knees and elbows
pub fn circles_intersection(r1: Vec2, r2: Vec2, l1: f64, l2: f64) -> Vec2 {
    let v = r2 - r1;
    let mut l = v.length();
    let mut l1 = l1;
    let mut l2 = l2;
    if l >= l1 + l2 {
        l = l1 + l2 - 0.000001;
    }
    if l1 >= l + l2 {
        l1 = l + l2 - 0.00001;
    }
    if l2 >= l + l1 {
        l2 = l + l1 - 0.00001;
    }
    let v_unit = v * (1.0 / l);
    let normal = v_unit.rotate_90();
    let x = (l1 * l1 - l2 * l2 + l * l) / (2.0 * l);
    let m = square_root(l1 * l1 - x * x);
    r1 + v_unit * x + normal * m
}
