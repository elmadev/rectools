use crate::text::Font;

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

// A pixel run of text, in a color of the palette
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TextRect {
    pub rect: PixelRect,
    pub color: u8,
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

// The font of the info panel's text: the game's, or its medium one for elma-classic's Large Font
fn info_font(large_font: bool) -> &'static Font {
    if large_font {
        Font::medium()
    } else {
        Font::small()
    }
}

// The game's info panel in the bottom right corner (render_info_panel), and rows of a label and
// a value under it. The last apple row has its label 180 game pixels from the right edge and
// the time right aligned 10 from it, 10 up from the bottom, like the game. Rows under it put it
// higher, each a line with a little room, and their labels start where its label does, further
// left for a row that needs the room, the values right aligned with the time.
pub(crate) fn info_panel_rects(
    view_width: i32,
    last_apple: Option<(i32, i64)>,
    rows: &[(String, String)],
    scale: i32,
    large_font: bool,
) -> Vec<TextRect> {
    // elma-classic's render_info_panel
    let (label_offset, per_char) = if large_font { (280, 12) } else { (180, 6) };
    let font = info_font(large_font);
    let width = view_width / scale;
    let right = width - 10;
    let text_width = |text: &str| font.width(text.as_bytes());
    let gap = 2 * text_width("  ");
    let last_apple = last_apple.map(|(apples, last)| {
        let value = compact_time(last);
        let offset = label_offset + (value.len() as i32 - 10).max(0) * per_char;
        (format!("last apple ({apples})"), value, width - offset)
    });
    let widest = rows
        .iter()
        .map(|(label, value)| right - text_width(label) - gap - text_width(value))
        .min();
    let game_x = last_apple
        .as_ref()
        .map_or(width - label_offset, |row| row.2);
    let label_x = widest.map_or(game_x, |x| x.min(game_x)).max(0);
    let step = font.line_height() + 2;
    let mut out = Vec::new();
    let mut row = |label: &str, value: &str, label_x: i32, line: usize| {
        let y = 10 + line as i32 * step;
        font.write(label.as_bytes(), label_x, y, scale, &mut out);
        font.write(
            value.as_bytes(),
            right - text_width(value),
            y,
            scale,
            &mut out,
        );
    };
    for (line, (label, value)) in rows.iter().rev().enumerate() {
        row(label, value, label_x, line);
    }
    if let Some((label, value, game_x)) = &last_apple {
        let x = if rows.is_empty() { *game_x } else { label_x };
        row(label, value, x, rows.len());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(rows: &[(&str, &str)]) -> Vec<(String, String)> {
        rows.iter().map(|&(l, v)| (l.into(), v.into())).collect()
    }

    fn bottom(rects: &[TextRect]) -> i32 {
        rects.iter().map(|r| r.rect.y).min().unwrap()
    }

    #[test]
    fn info_rows_go_under_the_last_apple_row_in_its_columns() {
        let step = Font::small().line_height() + 2;
        let alone = info_panel_rects(640, Some((3, 2297)), &[], 1, false);
        let info = rows(&[("lgr", "default"), ("level id", "123")]);
        let panel = info_panel_rects(640, Some((3, 2297)), &info, 1, false);
        // The rows alone are the same rows, under the last apple row moved two up
        let below = info_panel_rects(640, None, &info, 1, false);
        let row = &panel[below.len()..];
        assert_eq!(row.len(), alone.len());
        assert_eq!(bottom(row), bottom(&alone) + 2 * step);
        // Short rows leave the last apple row where the game has it
        assert_eq!(row[0].rect.x, alone[0].rect.x);
        // Its label and the rows' start in one column, and their values end in one
        let left = |line: usize| {
            let y = 10 + line as i32 * step;
            let on = panel
                .iter()
                .filter(|r| (y - 2..y + step - 2).contains(&r.rect.y));
            on.map(|r| r.rect.x).min().unwrap()
        };
        assert!((left(0) - left(2)).abs() <= 1 && (left(1) - left(2)).abs() <= 1);
    }

    #[test]
    fn a_long_row_moves_the_labels_left() {
        let short = info_panel_rects(640, Some((3, 2297)), &rows(&[("lgr", "x")]), 1, false);
        let long = rows(&[("level", "A long level name that needs some room")]);
        let wide = info_panel_rects(640, Some((3, 2297)), &long, 1, false);
        let left = |rects: &[TextRect]| rects.iter().map(|r| r.rect.x).min().unwrap();
        assert!(left(&wide) < left(&short) - 50);
        assert!(left(&wide) >= 0);
    }

    #[test]
    fn compact_times_like_the_game() {
        assert_eq!(compact_time(2297), "22:97");
        assert_eq!(compact_time(6523), "1:05:23");
        assert_eq!(compact_time(5), "0:05");
        assert_eq!(compact_time(360000 + 6100), "1:01:01:00");
    }

    #[test]
    fn the_last_apple_row_sits_bottom_right() {
        let rects = info_panel_rects(640, Some((3, 2297)), &[], 1, false);
        assert!(!rects.is_empty());
        assert!(rects.iter().all(|r| r.color == 25));
        let left = rects.iter().map(|r| r.rect.x).min().unwrap();
        let right = rects.iter().map(|r| r.rect.x + r.rect.width).max().unwrap();
        // Glyphs can start and end with an empty column
        assert!((640 - 180..640 - 177).contains(&left), "{left}");
        assert!((640 - 14..=640 - 10 - 1).contains(&right), "{right}");
        assert!(rects.iter().all(|r| r.rect.y >= 10 && r.rect.y < 10 + 14));
        // Twice the scale is the same row on twice the pixels
        let big = info_panel_rects(1280, Some((3, 2297)), &[], 2, false);
        assert_eq!(big.len(), rects.len());
        assert_eq!(big[0].rect.x, 2 * rects[0].rect.x);
    }

    #[test]
    fn the_large_font_row_is_further_from_the_edge_and_taller() {
        let small = info_panel_rects(640, Some((3, 2297)), &[], 1, false);
        let large = info_panel_rects(640, Some((3, 2297)), &[], 1, true);
        let left = |rects: &[TextRect]| rects.iter().map(|r| r.rect.x).min().unwrap();
        let top = |rects: &[TextRect]| {
            rects
                .iter()
                .map(|r| r.rect.y + r.rect.height)
                .max()
                .unwrap()
        };
        // Glyphs can start with an empty column
        assert!(
            (640 - 280..640 - 277).contains(&left(&large)),
            "{}",
            left(&large)
        );
        assert!(top(&large) > top(&small));
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
