// Across replays and levels, from a game folder (across.res, lev/, rec/)
use crate::error::{format, not_found, read};
use crate::{Error, FileKind, Graphics, Loaded, find_file};
use elma_formats::across::gfx::AcrossGraphics;
use elma_formats::across::rec::AcrossRec;
use elma_formats::across::table::{self, EXTERNAL_DESCRIPTOR};
use elma_formats::across::{level_from_lev, txt};
use elma_formats::lev::{Level, ObjectType};
use elma_formats::res::ResFile;
use replay::{Game, Settings};
use std::path::{Path, PathBuf};
use std::sync::Arc;

// The game's own files: an across.res, the original game's or the pack the web player is given
pub struct Files(ResFile);

impl Files {
    pub fn res(data: Vec<u8>) -> Result<Files, Error> {
        let res = ResFile::parse_across(data).map_err(format(FileKind::AcrossRes, "across.res"))?;
        Ok(Files(res))
    }

    // The across.res of the game folder `dir`, which also holds rec/ and lev/
    pub fn find(dir: &Path) -> Result<Files, Error> {
        let path = find_file(dir, "across.res").ok_or_else(|| {
            not_found(
                FileKind::AcrossRes,
                format!("across.res in {}", dir.display()),
            )
        })?;
        let res =
            ResFile::parse_across(read(&path)?).map_err(format(FileKind::AcrossRes, &path))?;
        Ok(Files(res))
    }

    pub fn get(&self, name: &str) -> Option<Vec<u8>> {
        self.0.res().get(name).map(<[u8]>::to_vec)
    }

    pub fn graphics(&self) -> Result<AcrossGraphics, Error> {
        AcrossGraphics::load(&|name| self.get(name))
            .map_err(format(FileKind::AcrossRes, "across.res"))
    }
}

// A level's file read the way Across does: the text levels and POT06 levels
// `path` is where it is, or what it is, for errors
pub fn parse_level(
    path: impl Into<PathBuf>,
    data: &[u8],
    kinds: &[ObjectType],
) -> Result<Level, Error> {
    let level = if data.starts_with(b"POT") {
        level_from_lev(data)
    } else {
        txt::parse(data, kinds)
    };
    level.map_err(format(FileKind::Lev, path))
}

// A built-in level, by its number in a replay. Returns it with its descriptor.
pub fn builtin_level(
    files: &Files,
    number: i32,
    kinds: &[ObjectType],
) -> Result<(Level, u8), Error> {
    let entry = table::builtin(number).ok_or_else(|| {
        not_found(
            FileKind::AcrossRes,
            format!("Across's built-in level {number}"),
        )
    })?;
    let data = files.get(entry.file).ok_or_else(|| {
        not_found(
            FileKind::AcrossRes,
            format!("{} ({}) in across.res", entry.file, entry.name),
        )
    })?;
    let mut level = parse_level(format!("{} in across.res", entry.file), &data, kinds)?;
    level.name = entry.name.to_string();
    Ok((level, entry.descriptor))
}

fn start_of(level: &Level) -> Option<(f64, f64)> {
    let start = level.objects.iter().find(|o| o.kind == ObjectType::Start)?;
    Some((start.r.x, -start.r.y))
}

// Of `levels`, the one a replay of a level from a file was recorded on. The Windows port stores
// a stamp of 1 when the level was in lev/, so then it is the level the bike starts on. The
// bike may have settled a little by the first frame.
pub fn match_level<'a>(rec: &AcrossRec, levels: &'a [(PathBuf, Level)]) -> Vec<&'a PathBuf> {
    if rec.stamp != 1
        && let Some((path, _)) = levels.iter().find(|(_, l)| l.level_id as u32 == rec.stamp)
    {
        return vec![path];
    }
    let (x, y) = rec.start();
    levels
        .iter()
        .filter(|(_, l)| {
            start_of(l).is_some_and(|(sx, sy)| (sx - x).abs() < 0.01 && (sy - y).abs() < 0.1)
        })
        .map(|(path, _)| path)
        .collect()
}

pub fn settings(rec: &AcrossRec) -> Settings {
    Settings::new(Game::Across { old: rec.old })
}

// Puts an Across replay together with its level and ghosts. Each ghost is named for errors.
pub fn assemble(
    path: PathBuf,
    rec: AcrossRec,
    level: Level,
    descriptor: u8,
    graphics: Arc<AcrossGraphics>,
    ghosts: Vec<(String, AcrossRec)>,
) -> Result<Loaded, Error> {
    let mut riders: Vec<_> = rec
        .rec
        .rides
        .iter()
        .map(|r| (Arc::new(r.clone()), None))
        .collect();
    for (name, ghost) in ghosts {
        if ghost.level != rec.level || ghost.old != rec.old {
            return Err(Error::NotSameLevel {
                ghost: name,
                level: "the same level".into(),
            });
        }
        riders.extend(ghost.rec.rides.iter().map(|r| (Arc::new(r.clone()), None)));
    }
    let settings = settings(&rec);
    let mut rec = rec.rec;
    if rec.level_id >= 0 {
        rec.level_id = level.level_id;
    }
    Ok(Loaded {
        path,
        level_only: false,
        rec,
        level,
        look: Graphics::Across(graphics, descriptor),
        riders,
        best_time: None,
        warnings: Vec::new(),
        settings,
    })
}

pub fn assemble_level(
    path: PathBuf,
    level: Level,
    descriptor: u8,
    graphics: Arc<AcrossGraphics>,
) -> Loaded {
    let mut rec = replay::start_rec(&level);
    rec.rides[0].frame_rate = elma_formats::across::rec::FRAME_RATE;
    let riders = vec![(Arc::new(rec.rides[0].clone()), None)];
    Loaded {
        path,
        level_only: true,
        rec,
        level,
        look: Graphics::Across(graphics, descriptor),
        riders,
        best_time: None,
        warnings: Vec::new(),
        settings: Settings::new(Game::Across { old: false }),
    }
}

// The descriptor of a level read from a file
pub const EXTERNAL: u8 = EXTERNAL_DESCRIPTOR;
