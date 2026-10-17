use elma_formats::abc::{Font, decode_abc};
use std::sync::OnceLock;

// Rectangle in view pixels, measured from the bottom left corner
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PixelRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Clone, Copy)]
enum Segment {
    BottomLeft,
    BottomRight,
    TopLeft,
    TopRight,
    Bottom,
    Middle,
    Top,
}

struct Metrics {
    // Of every line, around where the game's 1 pixel line is
    thickness: i32,
    line_width: i32,
    line_height: i32,
    digit_spacing: i32,
    digit_and_colon_spacing: i32,
    colon_x: i32,
    colon_y1: i32,
    colon_y2: i32,
}

fn segment(m: &Metrics, segment: Segment, x: i32, y: i32, out: &mut Vec<PixelRect>) {
    let grow = (m.thickness - 1) / 2;
    let vertical = |x, y| PixelRect {
        x: x - grow,
        y,
        width: m.thickness,
        height: m.line_height,
    };
    let horizontal = |x, y| PixelRect {
        x,
        y: y - grow,
        width: m.line_width,
        height: m.thickness,
    };
    out.push(match segment {
        Segment::BottomLeft => vertical(x, y + 1),
        Segment::BottomRight => vertical(x + m.line_width + 1, y + 1),
        Segment::TopLeft => vertical(x, y + m.line_height + 2),
        Segment::TopRight => vertical(x + m.line_width + 1, y + m.line_height + 2),
        Segment::Bottom => horizontal(x + 1, y),
        Segment::Middle => horizontal(x + 1, y + m.line_height + 1),
        Segment::Top => horizontal(x + 1, y + 2 * m.line_height + 2),
    });
}

fn digit(m: &Metrics, c: u8, x: i32, y: i32, out: &mut Vec<PixelRect>) {
    use Segment::*;
    let segments: &[Segment] = match c {
        b'0' => &[Bottom, BottomLeft, BottomRight, TopLeft, TopRight, Top],
        b'1' => &[BottomRight, TopRight],
        b'2' => &[Bottom, BottomLeft, Middle, TopRight, Top],
        b'3' => &[Bottom, BottomRight, Middle, TopRight, Top],
        b'4' => &[BottomRight, Middle, TopLeft, TopRight],
        b'5' => &[Bottom, BottomRight, Middle, TopLeft, Top],
        b'6' => &[Bottom, BottomLeft, BottomRight, Middle, TopLeft, Top],
        b'7' => &[BottomRight, TopRight, Top],
        b'8' => &[
            Bottom,
            BottomLeft,
            BottomRight,
            Middle,
            TopLeft,
            TopRight,
            Top,
        ],
        _ => &[Bottom, BottomRight, Middle, TopLeft, TopRight, Top],
    };
    for &s in segments {
        segment(m, s, x, y, out);
    }
}

fn colon(m: &Metrics, x: i32, y: i32, out: &mut Vec<PixelRect>) {
    for dy in [m.colon_y1, m.colon_y2] {
        out.push(PixelRect {
            x: x + m.colon_x + 1 - m.thickness,
            y: y + dy + 1 - m.thickness,
            width: 2 * m.thickness,
            height: 2 * m.thickness,
        });
    }
}

fn timer(m: &Metrics, text: &[u8; 8], mut x: i32, y: i32, out: &mut Vec<PixelRect>) {
    digit(m, text[0], x, y, out);
    x += m.digit_spacing;
    digit(m, text[1], x, y, out);
    colon(m, x, y, out);
    x += m.digit_and_colon_spacing;
    digit(m, text[3], x, y, out);
    x += m.digit_spacing;
    digit(m, text[4], x, y, out);
    colon(m, x, y, out);
    x += m.digit_and_colon_spacing;
    digit(m, text[6], x, y, out);
    x += m.digit_spacing;
    digit(m, text[7], x, y, out);
}

// MM:SS:CC, capped at 59:59:99 like the game's timer
pub fn format_centiseconds(time: i64) -> [u8; 8] {
    let time = time.max(0);
    let (mut minutes, mut seconds, mut centiseconds) =
        ((time / 6000) % 60, (time / 100) % 60, time % 100);
    if time / 360000 > 0 {
        (minutes, seconds, centiseconds) = (59, 59, 99);
    }
    let s = format!("{minutes:02}:{seconds:02}:{centiseconds:02}");
    s.as_bytes().try_into().unwrap()
}

// The pixels the game's seven segment timers cover. `best` and `flagtag` are shown
// when set; times are in centiseconds. Across draws them a little to the left (DIGIT.CPP).
pub(crate) fn timer_rects(
    view_width: i32,
    view_height: i32,
    best: Option<i64>,
    flagtag: Option<i64>,
    current: i64,
    across: bool,
    thickness: i32,
) -> Vec<PixelRect> {
    const LINE_HEIGHT: f64 = 16.0;
    const LINE_WIDTH: f64 = 10.0;
    const DIGIT_SPACING: f64 = 1.0 + LINE_WIDTH + 1.0 + 4.0;
    const COLON_SPACING: f64 = 4.0 + 4.0;
    let width_ratio = view_width as f64 / 640.0;
    let height_ratio = view_height as f64 / 480.0;
    let m = Metrics {
        thickness,
        line_width: (LINE_WIDTH * height_ratio) as i32,
        line_height: (LINE_HEIGHT * height_ratio) as i32,
        digit_spacing: (DIGIT_SPACING * height_ratio) as i32,
        digit_and_colon_spacing: ((DIGIT_SPACING + COLON_SPACING) * height_ratio) as i32,
        colon_x: ((DIGIT_SPACING + 1.0) * height_ratio) as i32,
        colon_y1: ((LINE_HEIGHT * 0.5 + 1.0) * height_ratio) as i32,
        colon_y2: ((LINE_HEIGHT * 1.5 - 1.0) * height_ratio) as i32,
    };
    let digit_width = 1 + m.line_width + 1;
    let timer_width = 3 * m.digit_spacing + 2 * m.digit_and_colon_spacing + digit_width;
    // At 640x480 Across's best time starts at x 28 and the current one at 488
    let (left_edge, right_edge) = if across {
        (28.0, 640.0 - 488.0 - 108.0)
    } else {
        (34.0, 34.0)
    };
    let edge_width = (left_edge * width_ratio) as i32;
    let y = (420.0 * height_ratio) as i32;

    let mut out = Vec::new();
    let best_x = edge_width;
    if let Some(best) = best {
        timer(&m, &format_centiseconds(best), best_x, y, &mut out);
    }
    let current_x = view_width - (right_edge * width_ratio) as i32 - timer_width;
    timer(&m, &format_centiseconds(current), current_x, y, &mut out);
    if let Some(flagtag) = flagtag {
        timer(
            &m,
            &format_centiseconds(flagtag),
            (best_x + current_x) / 2,
            y,
            &mut out,
        );
    }
    out
}

// The game's small font, which the info panel in the bottom right is written in: elma-classic's
// resources/small.abc, without the copy of medium.abc that file has after it
const SMALL_ABC: &[u8] = include_bytes!("../assets/small.abc");
// Pixels between characters, and the width of a space where a font has no picture for it
const SMALL_SPACING: i32 = 1;
const SPACE_WIDTH: i32 = 5;

fn small_font() -> &'static Font {
    static FONT: OnceLock<Font> = OnceLock::new();
    FONT.get_or_init(|| decode_abc(SMALL_ABC).expect("the built in font reads"))
}

// A pixel run of text, in a color of the palette
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TextRect {
    pub rect: PixelRect,
    pub color: u8,
}

// In game pixels, as abc8::len
fn text_width(font: &Font, text: &str) -> i32 {
    text.bytes()
        .map(|c| match font.glyph(c) {
            Some(g) => g.pic.width as i32 + SMALL_SPACING,
            None if c == b' ' => SPACE_WIDTH,
            None => 0,
        })
        .sum()
}

// `text` from `x`, on the line at `y`, as abc8::write. Positions are in game pixels from the
// bottom left, and a game pixel is `scale` view pixels square.
fn write(font: &Font, text: &str, mut x: i32, y: i32, scale: i32, out: &mut Vec<TextRect>) {
    for c in text.bytes() {
        let Some(g) = font.glyph(c) else {
            if c == b' ' {
                x += SPACE_WIDTH;
            }
            continue;
        };
        let w = g.pic.width;
        for row in 0..g.pic.height {
            let line = row * w..(row + 1) * w;
            let (solid, pixels) = (&g.solid[line.clone()], &g.pic.pixels[line]);
            let mut start = 0;
            while start < w {
                if !solid[start] {
                    start += 1;
                    continue;
                }
                let color = pixels[start];
                let end = (start..w)
                    .find(|&i| !solid[i] || pixels[i] != color)
                    .unwrap_or(w);
                out.push(TextRect {
                    rect: PixelRect {
                        x: (x + start as i32) * scale,
                        y: (y + g.y_offset + row as i32) * scale,
                        width: (end - start) as i32 * scale,
                        height: scale,
                    },
                    color,
                });
                start = end;
            }
        }
        x += w as i32 + SMALL_SPACING;
    }
}

// Seconds:hundredths, with minutes and hours only once there are any, as the game's
// compact times
fn compact_time(centiseconds: i64) -> String {
    let t = centiseconds.max(0);
    let (h, m, s, c) = (t / 360000, t / 6000 % 60, t / 100 % 60, t % 100);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}:{c:02}")
    } else if m > 0 {
        format!("{m}:{s:02}:{c:02}")
    } else {
        format!("{s}:{c:02}")
    }
}

// The last apple row of the game's info panel (render_info_panel): the label 180 game pixels
// from the right edge, the time right aligned 10 from it, 10 up from the bottom
pub(crate) fn last_apple_rects(
    view_width: i32,
    apples: i32,
    last_centiseconds: i64,
    scale: i32,
) -> Vec<TextRect> {
    let font = small_font();
    let width = view_width / scale;
    let value = compact_time(last_centiseconds);
    let label_offset = 180 + (value.len() as i32 - 10).max(0) * 6;
    let y = 10;
    let mut out = Vec::new();
    write(
        font,
        &format!("last apple ({apples})"),
        width - label_offset,
        y,
        scale,
        &mut out,
    );
    let value_x = width - 10 - text_width(font, &value);
    write(font, &value, value_x, y, scale, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_times_like_the_game() {
        assert_eq!(compact_time(2297), "22:97");
        assert_eq!(compact_time(6523), "1:05:23");
        assert_eq!(compact_time(5), "0:05");
        assert_eq!(compact_time(360000 + 6100), "1:01:01:00");
    }

    #[test]
    fn the_last_apple_row_sits_bottom_right() {
        let rects = last_apple_rects(640, 3, 2297, 1);
        assert!(!rects.is_empty());
        assert!(rects.iter().all(|r| r.color == 25));
        let left = rects.iter().map(|r| r.rect.x).min().unwrap();
        let right = rects.iter().map(|r| r.rect.x + r.rect.width).max().unwrap();
        // Glyphs can start and end with an empty column
        assert!((640 - 180..640 - 177).contains(&left), "{left}");
        assert!(
            (640 - 14..=640 - 10 - SMALL_SPACING).contains(&right),
            "{right}"
        );
        assert!(rects.iter().all(|r| r.rect.y >= 10 && r.rect.y < 10 + 14));
        // Twice the scale is the same row on twice the pixels
        let big = last_apple_rects(1280, 3, 2297, 2);
        assert_eq!(big.len(), rects.len());
        assert_eq!(big[0].rect.x, 2 * rects[0].rect.x);
    }

    #[test]
    fn formats_like_the_game() {
        assert_eq!(&format_centiseconds(0), b"00:00:00");
        assert_eq!(&format_centiseconds(123456), b"20:34:56");
        assert_eq!(&format_centiseconds(400000), b"59:59:99");
    }

    #[test]
    fn segments_do_not_overlap() {
        let rects = timer_rects(800, 600, None, None, 888888 % 360000, false, 1);
        for (i, a) in rects.iter().enumerate() {
            for b in &rects[i + 1..] {
                let overlap = a.x < b.x + b.width
                    && b.x < a.x + a.width
                    && a.y < b.y + b.height
                    && b.y < a.y + a.height;
                assert!(!overlap, "{a:?} {b:?}");
            }
        }
    }

    #[test]
    fn thick_lines_cover_the_games() {
        let thin = timer_rects(1200, 900, None, None, 888888 % 360000, false, 1);
        let thick = timer_rects(1200, 900, None, None, 888888 % 360000, false, 3);
        assert_eq!(thin.len(), thick.len());
        for (a, b) in thin.iter().zip(&thick) {
            assert!(b.x <= a.x && b.y <= a.y, "{a:?} {b:?}");
            assert!(b.x + b.width >= a.x + a.width && b.y + b.height >= a.y + a.height);
            assert_eq!(
                a.width.min(a.height) * 3,
                b.width.min(b.height),
                "{a:?} {b:?}"
            );
        }
    }
}
