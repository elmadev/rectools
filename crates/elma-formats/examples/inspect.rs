// Parses every file given on the command line and reports failures
use elma_formats::{lev::Level, lgr::Lgr, rec::Rec, res::Res};

fn main() {
    let mut failures = 0;
    let mut total = 0;
    for path in std::env::args().skip(1) {
        let Ok(data) = std::fs::read(&path) else {
            continue;
        };
        let lower = path.to_ascii_lowercase();
        total += 1;
        let result = if lower.ends_with(".lgr") {
            Lgr::parse(&data).map(|l| {
                format!(
                    "{} pictures, {} masks, {} textures, {} grass",
                    l.pictures.len(),
                    l.masks.len(),
                    l.textures.len(),
                    l.grass.len()
                )
            })
        } else if lower.ends_with(".lev") {
            Level::parse(&data, false).map(|l| {
                format!(
                    "{} polygons, {} objects, {} sprites, lgr {}",
                    l.polygons.len(),
                    l.objects.len(),
                    l.sprites.len(),
                    l.lgr
                )
            })
        } else if lower.ends_with(".rec") {
            Rec::parse(&data).map(|r| {
                format!(
                    "{} {} frames, {} events",
                    r.level_filename,
                    r.rides[0].frames.len(),
                    r.rides[0].events.len()
                )
            })
        } else if lower.ends_with(".res") {
            Res::parse(&data).map(|r| r.names().collect::<Vec<_>>().join(" "))
        } else {
            continue;
        };
        match result {
            Ok(summary) => {
                if std::env::var("VERBOSE").is_ok() {
                    println!("ok   {path}: {summary}");
                }
            }
            Err(e) => {
                failures += 1;
                println!("FAIL {path}: {e}");
            }
        }
    }
    println!("{failures} of {total} files failed");
}
