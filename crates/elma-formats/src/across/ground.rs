// Across has no polygons, only edges with the ground on their left. A pixel is ground until
// the first edge to its left on the same row, and from there on what that edge leaves on its
// right (MAGASSAG.CPP, RATESZ.CPP: teddra). Built-in levels are open chains with small gaps,
// so this is worked out exactly and handed on as sky polygons, which the even-odd fill draws
// the same way.

use crate::lev::{Polygon, Vec2};

// An edge from `a` to `b`, y pointing up
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    pub a: (f64, f64),
    pub b: (f64, f64),
}

impl Segment {
    // The edge has sky on its right: it points up
    fn sky_on_right(&self) -> bool {
        self.b.1 > self.a.1
    }

    fn low(&self) -> f64 {
        self.a.1.min(self.b.1)
    }

    fn high(&self) -> f64 {
        self.a.1.max(self.b.1)
    }

    fn x_at(&self, y: f64) -> f64 {
        let (a, b) = (self.a, self.b);
        a.0 + (b.0 - a.0) * (y - a.1) / (b.1 - a.1)
    }
}

// Sky as the game draws it from the left
pub fn is_sky(segments: &[Segment], x: f64, y: f64) -> bool {
    segments
        .iter()
        .filter(|s| s.low() <= y && y < s.high())
        .map(|s| (s.x_at(y), s))
        .filter(|(sx, _)| *sx <= x)
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .is_some_and(|(_, s)| s.sky_on_right())
}

// How far the sky reaches right of the last edge on a row
const RIGHT_MARGIN: f64 = 100.0;

// The sky as trapezoids between the edges, file coordinates (y pointing down)
pub fn sky_polygons(segments: &[Segment]) -> Vec<Polygon> {
    let segments: Vec<Segment> = segments
        .iter()
        .copied()
        .filter(|s| s.a.1 != s.b.1)
        .collect();
    let Some(right) = segments
        .iter()
        .flat_map(|s| [s.a.0, s.b.0])
        .max_by(f64::total_cmp)
    else {
        return Vec::new();
    };
    let right = right + RIGHT_MARGIN;

    let mut ys: Vec<f64> = segments.iter().flat_map(|s| [s.a.1, s.b.1]).collect();
    ys.sort_by(f64::total_cmp);
    ys.dedup();

    let mut order: Vec<usize> = (0..segments.len()).collect();
    order.sort_by(|&a, &b| segments[a].low().total_cmp(&segments[b].low()));
    let mut next = 0;
    let mut active: Vec<usize> = Vec::new();

    // Sky between a left edge and a right one (None: the margin), open since `bottom`
    struct Open {
        left: usize,
        right: Option<usize>,
        bottom: f64,
    }
    let mut open: Vec<Open> = Vec::new();
    let mut out = Vec::new();
    let x_at = |edge: Option<usize>, y: f64| edge.map_or(right, |e| segments[e].x_at(y));
    let close = |o: &Open, top: f64, out: &mut Vec<Polygon>| {
        let corners = [
            (x_at(Some(o.left), o.bottom), o.bottom),
            (x_at(o.right, o.bottom), o.bottom),
            (x_at(o.right, top), top),
            (x_at(Some(o.left), top), top),
        ];
        out.push(Polygon {
            is_grass: false,
            vertices: corners.iter().map(|&(x, y)| Vec2 { x, y: -y }).collect(),
        });
    };

    for slab in ys.windows(2) {
        let (y0, y1) = (slab[0], slab[1]);
        let mid = (y0 + y1) / 2.0;
        active.retain(|&s| segments[s].high() > y0);
        while next < order.len() && segments[order[next]].low() <= y0 {
            if segments[order[next]].high() > y0 {
                active.push(order[next]);
            }
            next += 1;
        }
        let mut crossings: Vec<(f64, usize)> =
            active.iter().map(|&s| (segments[s].x_at(mid), s)).collect();
        crossings.sort_by(|a, b| a.0.total_cmp(&b.0));

        let mut pairs: Vec<(usize, Option<usize>)> = Vec::new();
        for (i, &(_, s)) in crossings.iter().enumerate() {
            if segments[s].sky_on_right() {
                pairs.push((s, crossings.get(i + 1).map(|c| c.1)));
            }
        }
        // Sky between the same two edges carries on into this slab
        let mut still_open = Vec::with_capacity(open.len());
        for o in open.drain(..) {
            if let Some(p) = pairs.iter().position(|&(l, r)| l == o.left && r == o.right) {
                pairs.swap_remove(p);
                still_open.push(o);
            } else {
                close(&o, y0, &mut out);
            }
        }
        open = still_open;
        open.extend(pairs.into_iter().map(|(left, right)| Open {
            left,
            right,
            bottom: y0,
        }));
    }
    if let Some(&top) = ys.last() {
        for o in &open {
            close(o, top, &mut out);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inside(polygons: &[Polygon], x: f64, y: f64) -> bool {
        // Even-odd, like the renderer
        let mut odd = false;
        for p in polygons {
            let n = p.vertices.len();
            for i in 0..n {
                let (a, b) = (p.vertices[i], p.vertices[(i + 1) % n]);
                let (ay, by) = (-a.y, -b.y);
                if (ay <= y) != (by <= y) {
                    let cx = a.x + (b.x - a.x) * (y - ay) / (by - ay);
                    if cx > x {
                        odd = !odd;
                    }
                }
            }
        }
        odd
    }

    fn check(segments: &[Segment]) {
        let polygons = sky_polygons(segments);
        for i in 0..80 {
            for j in 0..80 {
                let (x, y) = (-2.0 + i as f64 * 0.137, -2.0 + j as f64 * 0.131);
                assert_eq!(
                    inside(&polygons, x, y),
                    is_sky(segments, x, y),
                    "at {x} {y}"
                );
            }
        }
    }

    fn chain(points: &[(f64, f64)]) -> Vec<Segment> {
        points
            .windows(2)
            .map(|w| Segment { a: w[0], b: w[1] })
            .collect()
    }

    #[test]
    fn closed_room_is_sky_inside() {
        // Ground on the left of each edge: clockwise around the sky
        let room = chain(&[(0.0, 0.0), (0.0, 5.0), (8.0, 5.0), (8.0, 0.0), (0.0, 0.0)]);
        let polygons = sky_polygons(&room);
        assert!(inside(&polygons, 4.0, 2.5));
        assert!(!inside(&polygons, -1.0, 2.5));
        assert!(!inside(&polygons, 9.0, 2.5));
        check(&room);
    }

    #[test]
    fn open_valley_and_gaps_match_the_game() {
        // A valley open at the top, with a gap in its floor
        let mut valley = chain(&[(0.0, 9.0), (1.0, 1.0), (3.0, 0.5)]);
        valley.extend(chain(&[(3.1, 0.55), (6.0, 2.0), (8.0, 9.0)]));
        check(&valley);
    }
}
