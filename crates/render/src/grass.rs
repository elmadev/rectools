use crate::instance::M2P;
use elma_formats::lev::Polygon;
use elma_formats::pic::Pic8;
use replay::Vec2;

pub const QUPDOWN_MARGIN: i32 = 20;
pub const QGRASS_MARGIN: i32 = QUPDOWN_MARGIN + 20;
pub const GRASS_DISTANCE: f64 = 600.0;
const HEIGHTMAP_LENGTH: usize = 10000;

// A QUP/QDOWN piece as the game uses it at zoom 1
pub struct Piece {
    pub pic: Pic8,
    pub is_up: bool,
    pub slope: i32,
    // Solid above the grass edge, where the qgrass texture fills in. None if the
    // piece is too large to get one.
    pub mask: Option<Pic8>,
}

impl Piece {
    pub fn new(source: &Pic8, is_up: bool, target_height: i32) -> Piece {
        let pic = source.resized(target_height);
        let slope = (target_height - (2 * QUPDOWN_MARGIN + 1)).max(0);
        let slope = if is_up { slope } else { -slope };
        let mask = grass_mask(&pic, target_height);
        Piece {
            pic,
            is_up,
            slope,
            mask,
        }
    }
}

// Port of generate_grass_mask at zoom 1: each column is solid down to its first
// opaque pixel, transparent columns are solid all the way.
fn grass_mask(pic: &Pic8, target_height: i32) -> Option<Pic8> {
    let mask_height = target_height.max(1) as usize;
    let scale = mask_height as f64 / pic.height as f64;
    let mask_width = ((pic.width as f64 * scale) as usize).max(1);
    if mask_height as f64 > 640.0 || mask_width as f64 > 480.0 {
        return None;
    }
    let transparency = pic.get(0, 0);
    let heightmap: Vec<usize> = (0..mask_width)
        .map(|j| {
            let src_j = ((j as f64 / scale) as usize).min(pic.width - 1);
            (0..pic.height)
                .find(|&i| pic.get(src_j, i) != transparency)
                .map(|i| (i as f64 * scale).ceil() as usize)
                .unwrap_or(mask_height)
        })
        .collect();
    let height = heightmap.iter().copied().max().unwrap_or(0);
    if height == 0 {
        return None;
    }
    let mut mask = Pic8::new(mask_width, height);
    for (j, &h) in heightmap.iter().enumerate() {
        for i in 0..h {
            mask.set(j, i, 1);
        }
    }
    Some(mask)
}

// Heightmap of the top edge of a grass polygon, in canvas pixels (y up) at zoom 1.
// Returns the first x and the heights from there on.
pub fn heightmap(poly: &Polygon, origin: Vec2) -> Option<(i32, Vec<i32>)> {
    let vertices: Vec<Vec2> = poly.vertices.iter().map(|v| Vec2::new(v.x, v.y)).collect();
    let n = vertices.len();
    let mut longest = 0.0;
    let mut v1 = 0usize;
    for i in 0..n {
        let j = (i + 1) % n;
        let length = (vertices[i].x - vertices[j].x).abs();
        if length > longest {
            v1 = i;
            longest = length;
        }
    }
    if longest < 0.0001 {
        return None;
    }
    let mut v2 = (v1 + 1) % n;
    let counterclockwise = vertices[v1].x >= vertices[v2].x;

    let mut heights = vec![0i32; HEIGHTMAP_LENGTH];
    let mut x0: i32 = -1;
    let mut cur: i32 = -1;
    for _ in 0..n - 1 {
        if counterclockwise {
            v1 = (v1 + 1) % n;
            v2 = (v2 + 1) % n;
        } else {
            v1 = (v1 + n - 1) % n;
            v2 = (v2 + n - 1) % n;
        }
        let (left, right) = if counterclockwise { (v1, v2) } else { (v2, v1) };
        cur = line_heightmap(
            vertices[left],
            vertices[right],
            &mut x0,
            cur,
            &mut heights,
            origin,
        );
    }
    if x0 < 0 {
        return None;
    }
    let length = (cur - x0 + 1).max(0) as usize;
    heights.truncate(length.min(HEIGHTMAP_LENGTH));
    if heights.is_empty() {
        return None;
    }
    Some((x0, heights))
}

fn line_heightmap(
    r1: Vec2,
    r2: Vec2,
    x0: &mut i32,
    mut cur: i32,
    heights: &mut [i32],
    origin: Vec2,
) -> i32 {
    // Only left-to-right lines are part of the top edge
    if r1.x > r2.x {
        return cur;
    }
    let x1 = ((r1.x - origin.x) * M2P) as i32;
    let y1 = (-r1.y - origin.y) * M2P;
    let x2 = ((r2.x - origin.x) * M2P) as i32;
    let y2 = (-r2.y - origin.y) * M2P;
    if x1 < 0 || x2 < 0 {
        return cur;
    }
    if cur < 0 {
        cur = x1;
        *x0 = x1;
        heights[0] = y1 as i32;
    }
    if x1 >= x2 {
        return cur;
    }
    let max = heights.len() as i32;
    if x1 - *x0 >= max || cur < x1 - 1 {
        return cur;
    }
    for x in x1..=x2 {
        if x < cur {
            continue;
        }
        if x - *x0 >= max {
            return cur;
        }
        let y = y1 + (y2 - y1) * (x as f64 - x1 as f64) / (x2 - x1) as f64;
        heights[(x - *x0) as usize] = y as i32;
        cur = x;
    }
    cur
}

// A piece placed along a grass polygon: bottom left corner in canvas pixels
pub struct Placement {
    pub piece: usize,
    pub x: i32,
    pub y: i32,
}

// Pick the pieces that follow the heightmap best, like draw_grass_polygon
pub fn place(pieces: &[Piece], x0: i32, heights: &[i32]) -> Vec<Placement> {
    let length = heights.len() as i32;
    let mut out = Vec::new();
    let mut x = x0;
    let mut y = heights[0];
    while x < x0 + length {
        let mut best: Option<(i32, usize)> = None;
        for (index, piece) in pieces.iter().enumerate() {
            let target_x = x + piece.pic.width as i32;
            let target_y = if target_x >= x0 + length {
                heights[heights.len() - 1]
            } else {
                heights[(target_x - x0) as usize]
            };
            let score = (y + piece.slope - target_y).abs();
            if best.is_none_or(|(s, _)| score < s) {
                best = Some((score, index));
            }
        }
        let Some((_, index)) = best else {
            break;
        };
        let piece = &pieces[index];
        let mut py = y - QUPDOWN_MARGIN;
        if !piece.is_up {
            py += piece.slope;
        }
        out.push(Placement {
            piece: index,
            x,
            y: py,
        });
        x += piece.pic.width.max(1) as i32;
        y += piece.slope;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use elma_formats::lev::Vec2 as LevVec2;

    #[test]
    fn mask_is_solid_above_the_grass() {
        // Transparent (0) on top, grass (5) starting at row 2 in column 0, row 1 in column 1
        let pic = Pic8 {
            width: 3,
            height: 4,
            pixels: vec![0, 0, 0, 0, 5, 0, 5, 5, 0, 5, 5, 0],
        };
        let mask = grass_mask(&pic, 4).unwrap();
        assert_eq!(mask.height, 4);
        assert_eq!(
            (0..3)
                .map(|x| (0..4).filter(|&y| mask.get(x, y) == 1).count())
                .collect::<Vec<_>>(),
            vec![2, 1, 4]
        );
    }

    #[test]
    fn heightmap_follows_the_top_edge() {
        // A flat strip from x=1 to x=3, top at y=-1 (file coordinates, y down)
        let poly = Polygon {
            is_grass: true,
            vertices: vec![
                LevVec2 { x: 1.0, y: -1.0 },
                LevVec2 { x: 3.0, y: -1.0 },
                LevVec2 { x: 3.0, y: 0.0 },
                LevVec2 { x: 1.0, y: 0.0 },
            ],
        };
        let (x0, heights) = heightmap(&poly, Vec2::new(0.0, 0.0)).unwrap();
        assert_eq!(x0, 48);
        assert_eq!(heights.len(), 97);
        assert!(heights.iter().all(|&h| h == 48 || h == 0));
    }
}
