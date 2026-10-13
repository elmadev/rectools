// The text levels most built-in Across levels are stored as (paly*.txt, cs*.txt; SZAKASZ.CPP,
// TARGY.CPP): a block of edges, then a block of objects, each an edge followed by how it moves.

use super::ground::{self, Segment};
use crate::lev::{Level, Motion, Object, ObjectType, TopTens, Vec2};
use crate::{Error, invalid};

struct Edge {
    r: (f64, f64),
    v: (f64, f64),
}

struct Movement {
    moving: bool,
    kind: usize,
    speed: f64,
    phase: f64,
    back_and_forth: bool,
}

struct Lines<'a> {
    lines: std::str::Lines<'a>,
}

impl Lines<'_> {
    fn next(&mut self) -> Result<&str, Error> {
        self.lines
            .next()
            .ok_or_else(|| Error::invalid("the level ends early"))
    }

    // The integers at the start of the next line, like sscanf
    fn ints(&mut self, n: usize) -> Result<Vec<i64>, Error> {
        let line = self.next()?;
        let values: Vec<i64> = line
            .split_whitespace()
            .take(n)
            .map(|word| word.parse::<i64>())
            .collect::<Result<_, _>>()
            .or_else(|_| invalid(format!("level line can't be read: {line}")))?;
        if values.len() < n {
            return invalid(format!("level line can't be read: {line}"));
        }
        Ok(values)
    }

    fn int(&mut self) -> Result<i64, Error> {
        Ok(self.ints(1)?[0])
    }
}

fn read_block(lines: &mut Lines, objects: bool) -> Result<Vec<(Edge, Option<Movement>)>, Error> {
    let header = lines.next()?;
    let mut words = header.split_whitespace();
    let version: i64 = match (words.next(), words.next().map(str::parse)) {
        (Some("Verzio:"), Some(Ok(version))) => version,
        _ => return invalid("not an Across text level"),
    };
    if !(3..=5).contains(&version) {
        return invalid("Across text level version isn't supported");
    }
    let count = lines.int()?;
    if !(0..5130).contains(&count) {
        return invalid("Across text level has too many edges");
    }
    let scale = 1.0 / 100000.0;
    let mut out = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let (ground_on_left, values) = if version < 5 {
            let v = lines.ints(5)?;
            (v[0] != 0, v[1..].to_vec())
        } else {
            (true, lines.ints(4)?)
        };
        let r = (values[0] as f64 * scale, values[1] as f64 * scale);
        let mut v = (values[2] as f64 * scale, values[3] as f64 * scale);
        // Version 4 on stores both ends
        if version > 3 {
            v = (v.0 - r.0, v.1 - r.1);
        }
        let edge = if ground_on_left {
            Edge { r, v }
        } else {
            Edge {
                r: (r.0 + v.0, r.1 + v.1),
                v: (-v.0, -v.1),
            }
        };
        let movement = if objects {
            let moving = lines.int()? != 0;
            let kind = lines.int()?;
            let speed = lines.int()? as f64 / 1000.0;
            let phase = lines.int()? as f64 / 1000.0;
            let back_and_forth = lines.int()? != 0;
            Some(Movement {
                moving,
                kind: usize::try_from(kind).or_else(|_| invalid("object has an invalid type"))?,
                speed,
                phase,
                back_and_forth,
            })
        } else {
            None
        };
        out.push((edge, movement));
    }
    Ok(out)
}

// `kinds` gives the object type of each entry of the object file
pub fn parse(data: &[u8], kinds: &[ObjectType]) -> Result<Level, Error> {
    let text = String::from_utf8_lossy(data);
    let mut lines = Lines {
        lines: text.lines(),
    };
    let edges = read_block(&mut lines, false)?;
    let things = read_block(&mut lines, true)?;

    let segments: Vec<Segment> = edges
        .iter()
        .map(|(e, _)| Segment {
            a: e.r,
            b: (e.r.0 + e.v.0, e.r.1 + e.v.1),
        })
        .collect();
    let polygons = ground::sky_polygons(&segments);
    if polygons.is_empty() {
        return invalid("level has no ground");
    }

    // File coordinates point down
    let file = |(x, y): (f64, f64)| Vec2 { x, y: -y };
    let mut objects = Vec::with_capacity(things.len());
    for (edge, movement) in &things {
        let movement = movement.as_ref().unwrap();
        let Some(&kind) = kinds.get(movement.kind) else {
            return invalid("object has an invalid type");
        };
        let end = (edge.r.0 + edge.v.0, edge.r.1 + edge.v.1);
        // The bike starts at the start of the edge, other objects sit at its end or move
        // from the end to the start
        let (r, motion) = if kind == ObjectType::Start {
            (file(edge.r), None)
        } else if movement.moving {
            let motion = Motion {
                from: file(end),
                to: file(edge.r),
                speed: movement.speed,
                phase: movement.phase,
                back_and_forth: movement.back_and_forth,
            };
            (motion.position(0.0), Some(motion))
        } else {
            (file(end), None)
        };
        objects.push(Object {
            r,
            kind,
            gravity: crate::lev::Gravity::None,
            animation: 0,
            motion,
        });
    }
    if !objects.iter().any(|o| o.kind == ObjectType::Start) {
        return invalid("level has no start");
    }

    Ok(Level {
        level_id: 0,
        name: String::new(),
        lgr: String::new(),
        foreground: String::new(),
        background: String::new(),
        polygons,
        objects,
        sprites: Vec::new(),
        topology_errors: false,
        top_tens: TopTens::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::across::DEFAULT_OBJECT_KINDS;

    const LEVEL: &str = "Verzio: 5
3
0 0 0 500000
800000 500000 800000 0
800000 0 0 0
Verzio: 5
3
100000 100000 100000 120000
0
3
0
0
0
200000 100000 300000 100000
1
1
500
250
1
400000 100000 400000 200000
0
0
0
0
0
";

    #[test]
    fn reads_edges_and_moving_objects() {
        let level = parse(LEVEL.as_bytes(), &DEFAULT_OBJECT_KINDS).unwrap();
        assert!(!level.polygons.is_empty());
        let start = &level.objects[0];
        assert_eq!(start.kind, ObjectType::Start);
        assert_eq!((start.r.x, start.r.y), (1.0, -1.0));

        let apple = &level.objects[1];
        assert_eq!(apple.kind, ObjectType::Food);
        let motion = apple.motion.unwrap();
        // From the end of the edge to its start, half a trip per unit of time, a quarter in
        let x = |t: f64| motion.position(t).x;
        assert!((x(0.0) - 2.75).abs() < 1e-9);
        assert!((x(1.0) - 2.25).abs() < 1e-9);
        // and back
        assert!((x(1.5) - 2.0).abs() < 1e-9);
        assert!((x(2.0) - 2.25).abs() < 1e-9);
        assert!((apple.r.x - 2.75).abs() < 1e-9 && apple.r.y == -1.0);

        let exit = &level.objects[2];
        assert_eq!(exit.kind, ObjectType::Exit);
        assert_eq!((exit.r.x, exit.r.y), (4.0, -2.0));
    }
}
