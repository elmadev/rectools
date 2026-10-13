// Compares two renders of the same replay frame by frame, e.g. elma-classic's
// rec2vid against this one. Both inputs are anything ffmpeg can read: a video or
// an image sequence pattern like frames/%04d.png.
use std::io::Read;
use std::process::{Command, Stdio};

fn usage() -> ! {
    eprintln!(
        "Usage: framediff <reference> <candidate> <width>x<height> [--ignore x,y,w,h]... [--threshold percent]

Prints the share of differing pixels per frame and exits with 1 when the mean
is above the threshold (default: 1%)."
    );
    std::process::exit(2);
}

fn decode(input: &str) -> std::process::ChildStdout {
    Command::new("ffmpeg")
        .args([
            "-v", "error", "-i", input, "-f", "rawvideo", "-pix_fmt", "rgb24", "-",
        ])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("failed to start ffmpeg: {e}"))
        .stdout
        .take()
        .unwrap()
}

fn read_frame(stream: &mut impl Read, buffer: &mut [u8]) -> bool {
    stream.read_exact(buffer).is_ok()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        usage();
    }
    let (w, h) = args[2]
        .split_once('x')
        .and_then(|(w, h)| Some((w.parse::<usize>().ok()?, h.parse::<usize>().ok()?)))
        .unwrap_or_else(|| usage());
    let mut ignore = Vec::new();
    let mut threshold = 1.0;
    let mut rest = args[3..].iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--ignore" => {
                let v: Vec<usize> = rest
                    .next()
                    .unwrap_or_else(|| usage())
                    .split(',')
                    .filter_map(|s| s.parse().ok())
                    .collect();
                if v.len() != 4 {
                    usage();
                }
                ignore.push((v[0], v[1], v[2], v[3]));
            }
            "--threshold" => {
                threshold = rest
                    .next()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or_else(|| usage())
            }
            _ => usage(),
        }
    }
    let ignored = |x: usize, y: usize| {
        ignore
            .iter()
            .any(|&(ix, iy, iw, ih)| x >= ix && x < ix + iw && y >= iy && y < iy + ih)
    };

    let mut reference = decode(&args[0]);
    let mut candidate = decode(&args[1]);
    let mut a = vec![0u8; w * h * 3];
    let mut b = vec![0u8; w * h * 3];
    let mut results = Vec::new();
    while read_frame(&mut reference, &mut a) && read_frame(&mut candidate, &mut b) {
        let mut differing = 0usize;
        let mut counted = 0usize;
        for y in 0..h {
            for x in 0..w {
                if ignored(x, y) {
                    continue;
                }
                counted += 1;
                let i = (y * w + x) * 3;
                if a[i..i + 3] != b[i..i + 3] {
                    differing += 1;
                }
            }
        }
        results.push(differing as f64 * 100.0 / counted.max(1) as f64);
    }
    if results.is_empty() {
        eprintln!("framediff: no frames to compare");
        std::process::exit(2);
    }
    let mean = results.iter().sum::<f64>() / results.len() as f64;
    let mut worst: Vec<(usize, f64)> = results.iter().copied().enumerate().collect();
    worst.sort_by(|x, y| y.1.total_cmp(&x.1));
    println!(
        "{} frames, {mean:.3}% of pixels differ on average",
        results.len()
    );
    for (frame, percent) in worst.iter().take(5) {
        println!("  frame {:5}: {percent:.3}%", frame + 1);
    }
    if mean > threshold {
        std::process::exit(1);
    }
}
