use crate::Error;
use crate::reader::{Reader, xor_decrypt};

const LEVEL_NAME_LENGTH: usize = 50;
const LEVEL_NAME_LENGTH_OLD: usize = 14;
const MAX_POLYGONS: usize = 1200;
const MAX_OBJECTS: usize = 252;
const MAX_SPRITES: usize = 5000;
const MAX_VERTICES: usize = 20000;
const CHECKSUM_MULTIPLIER: f64 = 3247.764325643;
const TOP_TEN_HEADER: i32 = 6754362;
const TOP_TEN_FOOTER: i32 = 8674642;
const TOP_TEN_SIZE: usize = 344;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec2 {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Polygon {
    pub is_grass: bool,
    pub vertices: Vec<Vec2>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ObjectType {
    Exit,
    Food,
    Killer,
    Start,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gravity {
    None,
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Object {
    pub r: Vec2,
    pub kind: ObjectType,
    pub gravity: Gravity,
    pub animation: i32,
    // Across objects can move along a line
    pub motion: Option<Motion>,
}

// An Across object moving from `from` to `to` and starting over, or turning back when
// `back_and_forth`. File coordinates, like the object's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Motion {
    pub from: Vec2,
    pub to: Vec2,
    // Trips per unit of game time
    pub speed: f64,
    pub phase: f64,
    pub back_and_forth: bool,
}

impl Motion {
    // Port of spritehely
    pub fn position(&self, time: f64) -> Vec2 {
        let phase = time * self.speed + self.phase;
        let trip = phase as i64;
        let mut fraction = phase - trip as f64;
        if self.back_and_forth && trip % 2 != 0 {
            fraction = 1.0 - fraction;
        }
        Vec2 {
            x: self.from.x + (self.to.x - self.from.x) * fraction,
            y: self.from.y + (self.to.y - self.from.y) * fraction,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clipping {
    Unclipped,
    Ground,
    Sky,
}

impl Clipping {
    pub fn from_i32(value: i32) -> Option<Clipping> {
        match value {
            0 => Some(Clipping::Unclipped),
            1 => Some(Clipping::Ground),
            2 => Some(Clipping::Sky),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sprite {
    pub picture: String,
    pub texture: String,
    pub mask: String,
    pub r: Vec2,
    pub distance: i32,
    pub clipping: Option<Clipping>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TopTime {
    pub centiseconds: i32,
    pub name1: String,
    pub name2: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TopTens {
    pub single: Vec<TopTime>,
    pub multi: Vec<TopTime>,
}

// Coordinates are as stored in the file, with y pointing down
#[derive(Debug, Clone, PartialEq)]
pub struct Level {
    pub level_id: i32,
    pub name: String,
    pub lgr: String,
    pub foreground: String,
    pub background: String,
    pub polygons: Vec<Polygon>,
    pub objects: Vec<Object>,
    pub sprites: Vec<Sprite>,
    pub topology_errors: bool,
    pub top_tens: TopTens,
}

impl Level {
    // `internal` levels are the .leb files packed in elma.res
    pub fn parse(data: &[u8], internal: bool) -> Result<Level, Error> {
        let mut r = Reader::new(data);
        let magic = r.array::<5>()?;
        let version = if internal {
            if &magic != b"@@^!@" {
                return r.invalid("internal level header is invalid");
            }
            14
        } else {
            if &magic[..3] != b"POT" {
                return r.invalid("not a level file");
            }
            let version =
                (magic[4].wrapping_sub(b'0') as i32) + 10 * (magic[3].wrapping_sub(b'0') as i32);
            if version > 14 {
                return r.invalid("level version is too new");
            }
            if version != 6 && version != 14 {
                return r.invalid("corrupt level file");
            }
            version
        };

        if version == 14 {
            let _level_id_checksum = r.u16()?;
        }
        let level_id = r.i32()?;
        let integrity_checksum = r.f64()?;
        let integrity_shareware = r.f64()?;
        let in_range = |value: f64, low: f64, high: f64| value >= low && value <= high;
        if !in_range(integrity_shareware + integrity_checksum, 9786.0, 36546.0) {
            return r.invalid("corrupt level file (integrity)");
        }
        let integrity_topology = r.f64()?;
        if !in_range(integrity_topology + integrity_checksum, 9786.0, 36546.0) {
            return r.invalid("corrupt level file (integrity)");
        }
        let topology_errors = integrity_topology + integrity_checksum > 20000.0;
        let integrity_locked = r.f64()?;
        if !in_range(integrity_locked + integrity_checksum, 9875.0, 32345.0) {
            return r.invalid("corrupt level file (integrity)");
        }

        let name_length = if version == 6 {
            LEVEL_NAME_LENGTH_OLD
        } else {
            LEVEL_NAME_LENGTH
        };
        let name = r.cstr(name_length + 1)?;
        let (lgr, foreground, background) = if version == 14 {
            (r.cstr(16)?, r.cstr(10)?, r.cstr(10)?)
        } else {
            ("default".into(), "ground".into(), "sky".into())
        };
        if version == 6 {
            r.seek(100)?;
        }

        let polygon_count = r.f64()? as i64;
        // The object count is moved out of order in .leb files
        let mut object_count = if internal {
            Some(r.f64()? as i64)
        } else {
            None
        };
        if polygon_count <= 0 || polygon_count as usize > MAX_POLYGONS {
            return r.invalid("level has an invalid polygon count");
        }
        let mut polygons = Vec::with_capacity(polygon_count as usize);
        for _ in 0..polygon_count {
            let is_grass = if version == 14 { r.i32()? != 0 } else { false };
            let vertex_count = r.i32()?;
            if vertex_count < 3 || vertex_count as usize > MAX_VERTICES {
                return r.invalid("polygon has an invalid vertex count");
            }
            let mut vertices = Vec::with_capacity(vertex_count as usize);
            for _ in 0..vertex_count {
                vertices.push(Vec2 {
                    x: r.f64()?,
                    y: r.f64()?,
                });
            }
            polygons.push(Polygon { is_grass, vertices });
        }

        if object_count.is_none() {
            object_count = Some(r.f64()? as i64);
        }
        let object_count = object_count.unwrap();
        if object_count <= 0 || object_count as usize > MAX_OBJECTS {
            return r.invalid("level has an invalid object count");
        }
        let mut objects = Vec::with_capacity(object_count as usize);
        for _ in 0..object_count {
            let r_ = Vec2 {
                x: r.f64()?,
                y: r.f64()?,
            };
            let kind = match r.i32()? {
                1 => ObjectType::Exit,
                2 => ObjectType::Food,
                3 => ObjectType::Killer,
                4 => ObjectType::Start,
                _ => return r.invalid("object has an invalid type"),
            };
            let (gravity, animation) = if version == 14 {
                // The game ignores properties it doesn't know
                let gravity = match r.i32()? {
                    1 => Gravity::Up,
                    2 => Gravity::Down,
                    3 => Gravity::Left,
                    4 => Gravity::Right,
                    _ => Gravity::None,
                };
                (gravity, r.i32()?)
            } else {
                (Gravity::None, 0)
            };
            if !(0..=8).contains(&animation) {
                return r.invalid("object has an invalid animation");
            }
            objects.push(Object {
                r: r_,
                kind,
                gravity,
                animation,
                motion: None,
            });
        }

        let mut sprites = Vec::new();
        if version == 14 {
            let sprite_count = r.f64()? as i64;
            if sprite_count < 0 || sprite_count as usize > MAX_SPRITES {
                return r.invalid("level has an invalid picture count");
            }
            for _ in 0..sprite_count {
                let picture = r.cstr(10)?;
                let texture = r.cstr(10)?;
                let mask = r.cstr(10)?;
                let r_ = Vec2 {
                    x: r.f64()?,
                    y: r.f64()?,
                };
                let distance = r.i32()?;
                let clipping = Clipping::from_i32(r.i32()?);
                sprites.push(Sprite {
                    picture,
                    texture,
                    mask,
                    r: r_,
                    distance,
                    clipping,
                });
            }
        }

        let level = Level {
            level_id,
            name,
            lgr,
            foreground,
            background,
            polygons,
            objects,
            sprites,
            topology_errors,
            top_tens: TopTens::default(),
        };
        if (level.checksum() - integrity_checksum).abs() > 0.01 {
            return r.invalid("corrupt level file (checksum)");
        }
        let mut level = level;
        if !internal {
            level.top_tens = read_top_tens(&mut r).unwrap_or_default();
        }
        Ok(level)
    }

    // The best time of the level's top ten, in centiseconds
    pub fn best_time(&self, multiplayer: bool) -> Option<i64> {
        let top_ten = if multiplayer {
            &self.top_tens.multi
        } else {
            &self.top_tens.single
        };
        top_ten.first().map(|t| t.centiseconds as i64)
    }

    pub fn checksum(&self) -> f64 {
        let mut sum = 0.0;
        for polygon in &self.polygons {
            for v in &polygon.vertices {
                sum += v.x;
                sum += v.y;
            }
        }
        for object in &self.objects {
            let kind = match object.kind {
                ObjectType::Exit => 1,
                ObjectType::Food => 2,
                ObjectType::Killer => 3,
                ObjectType::Start => 4,
            };
            sum += object.r.x;
            sum += object.r.y;
            sum += kind as f64;
        }
        for sprite in &self.sprites {
            sum += sprite.r.x;
            sum += sprite.r.y;
        }
        CHECKSUM_MULTIPLIER * sum
    }

    // Objects in the order the game keeps them in during play: killers, apples, exit, start.
    // Replay events refer to objects by their index in this order.
    pub fn sorted_objects(&self) -> Vec<Object> {
        let mut objects = self.objects.clone();
        let order = |kind: ObjectType| match kind {
            ObjectType::Killer => 1,
            ObjectType::Food => 2,
            ObjectType::Exit => 3,
            ObjectType::Start => 10,
        };
        objects.sort_by_key(|o| order(o.kind));
        objects
    }
}

// The optional, encrypted best times stored after the level data
fn read_top_tens(r: &mut Reader) -> Option<TopTens> {
    if r.i32().ok()? != TOP_TEN_HEADER {
        return None;
    }
    let mut data = r.bytes(2 * TOP_TEN_SIZE).ok()?.to_vec();
    if r.i32().ok()? != TOP_TEN_FOOTER {
        return None;
    }
    xor_decrypt(&mut data, 21, 9783, 3389);
    let table = |t: &[u8]| -> Vec<TopTime> {
        let count = i32::from_le_bytes(t[0..4].try_into().unwrap()).clamp(0, 10) as usize;
        (0..count)
            .map(|i| TopTime {
                centiseconds: i32::from_le_bytes(t[4 + i * 4..8 + i * 4].try_into().unwrap()),
                name1: crate::reader::cstr(&t[44 + i * 15..59 + i * 15]),
                name2: crate::reader::cstr(&t[194 + i * 15..209 + i * 15]),
            })
            .collect()
    };
    Some(TopTens {
        single: table(&data[..TOP_TEN_SIZE]),
        multi: table(&data[TOP_TEN_SIZE..]),
    })
}

// Maps a QWQUUnnn.LEV name to the .leb file inside elma.res
pub fn internal_level_file(filename: &str) -> Option<&'static str> {
    // QWQUU001 to QWQUU055, index 0 is unused
    const INTERNAL_FILE_PATHS: [&str; 56] = [
        "nulla.leb",
        "a01.leb",
        "a02.leb",
        "a03.leb",
        "a04.leb",
        "a05.leb",
        "a06.leb",
        "a07.leb",
        "ujtag.leb",
        "a08.leb",
        "a09.leb",
        "ujgrav.leb",
        "a10.leb",
        "a11.leb",
        "a12.leb",
        "a13.leb",
        "a14.leb",
        "a15.leb",
        "a16.leb",
        "a17.leb",
        "ujupdown.leb",
        "a18.leb",
        "a19.leb",
        "a20.leb",
        "a21.leb",
        "a22.leb",
        "a23.leb",
        "a24.leb",
        "a25.leb",
        "a26.leb",
        "ujkomb.leb",
        "a27.leb",
        "ujtolcs.leb",
        "a28.leb",
        "ujzuhan.leb",
        "a29.leb",
        "a30.leb",
        "a31.leb",
        "a32.leb",
        "ujvissza.leb",
        "a33.leb",
        "a34.leb",
        "a35.leb",
        "a36.leb",
        "a37.leb",
        "ujcsab.leb",
        "Mate.leb",
        "a38.leb",
        "ujdownhi.leb",
        "ujcsomo.leb",
        "a39.leb",
        "a40.leb",
        "a41.leb",
        "ujhook.leb",
        "a42.leb",
        "visit.leb",
    ];
    let mut name = filename;
    if name.len() == 12 && name[8..].eq_ignore_ascii_case(".lev") {
        name = &name[..8];
    }
    if name.len() != 8 || !name[..5].eq_ignore_ascii_case("QWQUU") {
        return None;
    }
    let index: usize = name[5..].parse().ok()?;
    if index == 0 {
        return None;
    }
    INTERNAL_FILE_PATHS.get(index).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_internal_level_names() {
        assert_eq!(internal_level_file("QWQUU001.LEV"), Some("a01.leb"));
        assert_eq!(internal_level_file("qwquu053.lev"), Some("ujhook.leb"));
        assert_eq!(internal_level_file("QWQUU055.LEV"), Some("visit.leb"));
        assert_eq!(internal_level_file("QWQUU056.LEV"), None);
        assert_eq!(internal_level_file("QWQUU000.LEV"), None);
        assert_eq!(internal_level_file("mylevel.lev"), None);
    }

    #[test]
    fn sorts_objects_like_the_game() {
        let object = |kind| Object {
            r: Vec2 { x: 0.0, y: 0.0 },
            kind,
            gravity: Gravity::None,
            animation: 0,
            motion: None,
        };
        let level = Level {
            level_id: 0,
            name: String::new(),
            lgr: String::new(),
            foreground: String::new(),
            background: String::new(),
            polygons: vec![],
            objects: vec![
                object(ObjectType::Start),
                object(ObjectType::Exit),
                object(ObjectType::Food),
                object(ObjectType::Killer),
            ],
            sprites: vec![],
            topology_errors: false,
            top_tens: TopTens::default(),
        };
        let kinds: Vec<_> = level.sorted_objects().iter().map(|o| o.kind).collect();
        assert_eq!(
            kinds,
            [
                ObjectType::Killer,
                ObjectType::Food,
                ObjectType::Exit,
                ObjectType::Start
            ]
        );
    }
}
