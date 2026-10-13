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
    line_width: i32,
    line_height: i32,
    digit_spacing: i32,
    digit_and_colon_spacing: i32,
    colon_x: i32,
    colon_y1: i32,
    colon_y2: i32,
}

fn segment(m: &Metrics, segment: Segment, x: i32, y: i32, out: &mut Vec<PixelRect>) {
    let vertical = |x, y| PixelRect {
        x,
        y,
        width: 1,
        height: m.line_height,
    };
    let horizontal = |x, y| PixelRect {
        x,
        y,
        width: m.line_width,
        height: 1,
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
            x: x + m.colon_x,
            y: y + dy,
            width: 2,
            height: 2,
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
) -> Vec<PixelRect> {
    const LINE_HEIGHT: f64 = 16.0;
    const LINE_WIDTH: f64 = 10.0;
    const DIGIT_SPACING: f64 = 1.0 + LINE_WIDTH + 1.0 + 4.0;
    const COLON_SPACING: f64 = 4.0 + 4.0;
    let width_ratio = view_width as f64 / 640.0;
    let height_ratio = view_height as f64 / 480.0;
    let m = Metrics {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_like_the_game() {
        assert_eq!(&format_centiseconds(0), b"00:00:00");
        assert_eq!(&format_centiseconds(123456), b"20:34:56");
        assert_eq!(&format_centiseconds(400000), b"59:59:99");
    }

    #[test]
    fn segments_do_not_overlap() {
        let rects = timer_rects(800, 600, None, None, 888888 % 360000, false);
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
}
