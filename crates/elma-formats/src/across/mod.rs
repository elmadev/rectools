// Action SuperCross (Across), the game Elasto Mania grew out of. Its replays and levels are
// read into the Elma types where they mean the same.

pub mod gfx;
pub mod ground;
pub mod rec;
pub mod table;
pub mod txt;

use crate::lev::{Level, ObjectType};
use crate::{Error, invalid};

// The object types of the original object file (T4.TTT), which levels refer to by index
pub const DEFAULT_OBJECT_KINDS: [ObjectType; 4] = [
    ObjectType::Exit,
    ObjectType::Food,
    ObjectType::Killer,
    ObjectType::Start,
];

// A POT06 level the way Across reads it: the sky is worked out from the polygons' edges
pub fn level_from_lev(data: &[u8]) -> Result<Level, Error> {
    let mut level = Level::parse(data, false)?;
    let segments: Vec<ground::Segment> = level
        .polygons
        .iter()
        .flat_map(|p| {
            let n = p.vertices.len();
            (0..n).map(move |i| {
                let (a, b) = (p.vertices[i], p.vertices[(i + 1) % n]);
                // Upside down, like the game
                ground::Segment {
                    a: (a.x, -a.y),
                    b: (b.x, -b.y),
                }
            })
        })
        .collect();
    if segments.is_empty() {
        return invalid("level has no polygons");
    }
    level.polygons = ground::sky_polygons(&segments);
    Ok(level)
}
