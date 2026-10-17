// Loads a replay and everything it is drawn from out of a game folder: elma.res, lev/,
// lgr/, rec/ and bmp/, matching names case-insensitively like the game does on Windows.
// Across folders hold across.res, lev/ and rec/.
pub mod across;
mod error;

pub use error::{Error, FileKind};
use error::{format, not_found, read};

pub use elma_formats::Graphics;
use elma_formats::across::gfx::AcrossGraphics;
use elma_formats::across::rec::AcrossRec;
pub use elma_formats::across::rec::is_across_rec;
use elma_formats::lev::{Level, internal_level_file};
use elma_formats::lgr::Lgr;
use elma_formats::pic::{Pic8, decode_shirt};
use elma_formats::rec::{Rec, Ride};
use elma_formats::res::{Res, ResFile};
use replay::Settings;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

// Finds `name` in `dir` ignoring case, since game folders mix both
pub fn find_file(dir: &Path, name: &str) -> Option<PathBuf> {
    let exact = dir.join(name);
    if exact.is_file() {
        return Some(exact);
    }
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(name))
        .map(|e| e.path())
}

pub struct GameFolder {
    pub dir: PathBuf,
    game: Game,
}

enum Game {
    Elma(ResFile),
    Across(across::Files, OnceLock<Result<Arc<AcrossGraphics>, Error>>),
}

// A replay of another rider drawn over the main one
#[derive(Debug, Clone)]
pub struct Ghost {
    pub path: String,
    pub nick: String,
}

pub struct Loaded {
    pub path: PathBuf,
    // A level without a replay, see assemble_level
    pub level_only: bool,
    pub rec: Rec,
    pub level: Level,
    pub look: Graphics,
    // The main replay's riders, then the ghosts, with their shirts
    pub riders: Vec<(Arc<Ride>, Option<Pic8>)>,
    // From the level's top ten, in centiseconds
    pub best_time: Option<i64>,
    // Things that were skipped, like an invalid shirt
    pub warnings: Vec<String>,
    // How the session plays the replay
    pub settings: Settings,
}

impl GameFolder {
    // An Elma folder, or an Across one. A folder with both elma.res and across.res opens as
    // Elma, see open_game.
    pub fn open(dir: impl Into<PathBuf>) -> Result<GameFolder, Error> {
        let dir = dir.into();
        match (find_file(&dir, "elma.res"), find_file(&dir, "across.res")) {
            (None, None) => Err(not_found(
                FileKind::ElmaRes,
                format!("elma.res or across.res in {}", dir.display()),
            )),
            (elma, _) => GameFolder::open_game(dir, elma.is_none()),
        }
    }

    // The folder as one game's, so the two can share a folder
    pub fn open_game(dir: impl Into<PathBuf>, across: bool) -> Result<GameFolder, Error> {
        let dir = dir.into();
        if across {
            let files = across::Files::find(&dir)?;
            return Ok(GameFolder {
                dir,
                game: Game::Across(files, OnceLock::new()),
            });
        }
        let path = find_file(&dir, "elma.res").ok_or_else(|| {
            not_found(FileKind::ElmaRes, format!("elma.res in {}", dir.display()))
        })?;
        let res = ResFile::parse(read(&path)?).map_err(format(FileKind::ElmaRes, &path))?;
        Ok(GameFolder {
            dir,
            game: Game::Elma(res),
        })
    }

    // The folder as the game of the replay `input`, a path or a name in rec/
    pub fn open_for(dir: impl Into<PathBuf>, input: &str) -> Result<GameFolder, Error> {
        let dir = dir.into();
        let path = Some(PathBuf::from(input))
            .filter(|p| p.is_file())
            .or_else(|| find_file(&dir.join("rec"), input));
        match path.and_then(|p| std::fs::read(p).ok()) {
            Some(data) => GameFolder::open_game(dir, is_across_rec(&data)),
            None => GameFolder::open(dir),
        }
    }

    pub fn is_across(&self) -> bool {
        matches!(self.game, Game::Across(..))
    }

    // A file of the game: from elma.res, or Across's files
    pub fn file(&self, name: &str) -> Option<Vec<u8>> {
        match &self.game {
            Game::Elma(res) => res.res().get(name).map(<[u8]>::to_vec),
            Game::Across(files, _) => files.get(name),
        }
    }

    fn res(&self) -> Result<Res<'_>, Error> {
        match &self.game {
            Game::Elma(res) => Ok(res.res()),
            Game::Across(..) => Err(Error::WrongGame {
                folder: "Across",
                wanted: "Elma",
            }),
        }
    }

    fn across(&self) -> Result<(&across::Files, Arc<AcrossGraphics>), Error> {
        match &self.game {
            Game::Across(files, graphics) => {
                let graphics = graphics.get_or_init(|| files.graphics().map(Arc::new));
                Ok((files, graphics.clone()?))
            }
            Game::Elma(_) => Err(Error::WrongGame {
                folder: "Elma",
                wanted: "Across",
            }),
        }
    }

    // The levels in lev/ and the .lev and .txt files next to `replay`, for finding the one an
    // Across replay was recorded on
    fn across_levels(
        &self,
        replay: &Path,
        kinds: &[elma_formats::lev::ObjectType],
    ) -> Vec<(PathBuf, Level)> {
        let lev = self.dir.join("lev");
        let mut dirs = vec![lev.clone()];
        if let Some(dir) = replay.parent()
            && canonical(dir) != canonical(&lev)
        {
            dirs.push(dir.to_path_buf());
        }
        let is_level = |p: &Path| {
            p.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("lev") || e.eq_ignore_ascii_case("txt"))
        };
        dirs.iter()
            .enumerate()
            .flat_map(|(i, dir)| {
                let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| i == 0 || (p.is_file() && is_level(p)))
                    .collect();
                paths.sort();
                paths
            })
            .filter_map(|path| {
                let data = std::fs::read(&path).ok()?;
                // Elma's levels, when the games share the folder
                if data.starts_with(b"POT") && !data.starts_with(b"POT06") {
                    return None;
                }
                let name = path.display().to_string();
                Some((path, across::parse_level(&name, &data, kinds).ok()?))
            })
            .collect()
    }

    fn load_across(
        &self,
        path: PathBuf,
        data: &[u8],
        ghosts: &[Ghost],
        level_file: Option<&Path>,
    ) -> Result<Loaded, Error> {
        let (files, graphics) = self.across()?;
        let kinds = graphics.kinds();
        let rec = AcrossRec::parse(data).map_err(format(FileKind::Rec, &path))?;
        let (level, descriptor) = match rec.level {
            Some(number) => across::builtin_level(files, number, &kinds)?,
            None => {
                let levels = match level_file {
                    Some(file) => {
                        let data = read(file)?;
                        let level = across::parse_level(file, &data, &kinds)?;
                        vec![(file.to_path_buf(), level)]
                    }
                    None => self.across_levels(&path, &kinds),
                };
                let matches = across::match_level(&rec, &levels);
                let Some(&found) = matches.first() else {
                    return Err(match level_file {
                        Some(file) => Error::WrongLevel {
                            replay: path.clone(),
                            level: file.to_path_buf(),
                        },
                        None => Error::MissingLevel(None),
                    });
                };
                let level = levels.iter().find(|(p, _)| p == found).unwrap().1.clone();
                (level, across::EXTERNAL)
            }
        };
        let mut others = Vec::new();
        for ghost in ghosts {
            let ghost_path = self
                .resolve_replay(&ghost.path)
                .ok_or_else(|| not_found(FileKind::Rec, format!("replay {}", ghost.path)))?;
            let other = AcrossRec::parse(&read(&ghost_path)?)
                .map_err(format(FileKind::Rec, &ghost_path))?;
            others.push((ghost_path.display().to_string(), other));
        }
        across::assemble(path, rec, level, descriptor, graphics, others)
    }

    fn load_across_level(&self, input: &str) -> Result<Loaded, Error> {
        let (files, graphics) = self.across()?;
        let kinds = graphics.kinds();
        let given = PathBuf::from(input);
        let name = given
            .file_name()
            .map_or(input.into(), |n| n.to_string_lossy().into_owned());
        // A built-in level by its file name, like paly07.txt
        if !given.is_file()
            && let Some(number) = (0..elma_formats::across::table::LEVEL_COUNT as i32).find(|&n| {
                elma_formats::across::table::builtin(n)
                    .is_some_and(|e| e.file.eq_ignore_ascii_case(&name))
            })
        {
            let (level, descriptor) = across::builtin_level(files, number, &kinds)?;
            return Ok(across::assemble_level(given, level, descriptor, graphics));
        }
        let path = if given.is_file() {
            given
        } else {
            find_file(&self.dir.join("lev"), input)
                .ok_or_else(|| not_found(FileKind::Lev, format!("level {input}")))?
        };
        let level = across::parse_level(&path, &read(&path)?, &kinds)?;
        Ok(across::assemble_level(
            path,
            level,
            across::EXTERNAL,
            graphics,
        ))
    }

    // Accepts both a path as given and a name relative to the rec/ folder
    pub fn resolve_replay(&self, input: &str) -> Option<PathBuf> {
        let path = PathBuf::from(input);
        if path.is_file() {
            return Some(path);
        }
        find_file(&self.dir.join("rec"), input)
    }

    fn shirt(&self, nick: &str, warnings: &mut Vec<String>) -> Option<Pic8> {
        if nick.is_empty() {
            return None;
        }
        let Some(path) = find_file(&self.dir.join("bmp"), &format!("{nick}.bmp")) else {
            warnings.push(format!("bmp/{nick}.bmp not found, using the default body"));
            return None;
        };
        let shirt = std::fs::read(path).ok().and_then(|d| decode_shirt(&d));
        if shirt.is_none() {
            warnings.push(format!(
                "bmp/{nick}.bmp is not a valid shirt, using the default body"
            ));
        }
        shirt
    }

    // The LGR the level asks for, or default.lgr
    fn lgr(&self, level: &Level, warnings: &mut Vec<String>) -> Result<Lgr, Error> {
        let lgr_dir = self.dir.join("lgr");
        [level.lgr.as_str(), "default"]
            .iter()
            .find_map(|name| {
                let path = find_file(&lgr_dir, &format!("{name}.lgr"))?;
                match Lgr::parse(&std::fs::read(&path).ok()?) {
                    Ok(lgr) => Some(lgr),
                    Err(e) => {
                        warnings.push(format!("{}: {e}", path.display()));
                        None
                    }
                }
            })
            .ok_or_else(|| {
                not_found(
                    FileKind::Lgr,
                    format!("{}.lgr or default.lgr in {}", level.lgr, lgr_dir.display()),
                )
            })
    }

    // A level to look around without a replay. Accepts both a path as given and a name
    // relative to the lev/ folder, and the internal levels by their name, like QWQUU001.LEV.
    pub fn load_level(&self, input: &str) -> Result<Loaded, Error> {
        if self.is_across() {
            return self.load_across_level(input);
        }
        let given = PathBuf::from(input);
        let name = given
            .file_name()
            .map_or(input.into(), |n| n.to_string_lossy().into_owned());
        let (path, level) = if !given.is_file()
            && let Some(internal) = internal_level_file(&name)
        {
            (given, internal_level(&self.res()?, internal)?)
        } else {
            let path = if given.is_file() {
                given
            } else {
                find_file(&self.dir.join("lev"), input)
                    .ok_or_else(|| not_found(FileKind::Lev, format!("level {input}")))?
            };
            let level = Level::parse(&read(&path)?, false).map_err(format(FileKind::Lev, &path))?;
            (path, level)
        };
        let mut warnings = Vec::new();
        let lgr = self.lgr(&level, &mut warnings)?;
        let mut loaded = assemble_level(path, level, lgr)?;
        loaded.warnings = warnings;
        Ok(loaded)
    }

    // The level of the Elma replay at `replay`: in lev/ or next to the replay, preferring the
    // one it was recorded on when both have a level of that name
    fn find_level(&self, replay: &Path, rec: &Rec) -> Result<Level, Error> {
        let dirs = [
            Some(self.dir.join("lev")),
            replay.parent().map(Path::to_path_buf),
        ];
        let mut found: Vec<(PathBuf, Level)> = Vec::new();
        for dir in dirs.iter().flatten() {
            if let Some(lev) = find_file(dir, &rec.level_filename)
                && !found.iter().any(|(p, _)| canonical(p) == canonical(&lev))
            {
                let level =
                    Level::parse(&read(&lev)?, false).map_err(format(FileKind::Lev, &lev))?;
                found.push((lev, level));
            }
        }
        if found.is_empty() {
            return Err(Error::MissingLevel(Some(rec.level_filename.clone())));
        }
        let index = found
            .iter()
            .position(|(_, l)| l.level_id == rec.level_id)
            .unwrap_or(0);
        Ok(found.swap_remove(index).1)
    }

    // `level_file` is the level to play the replay on, in place of the one in lev/
    pub fn load(
        &self,
        input: &str,
        nick: &str,
        ghosts: &[Ghost],
        level_file: Option<&Path>,
    ) -> Result<Loaded, Error> {
        let path = self
            .resolve_replay(input)
            .ok_or_else(|| not_found(FileKind::Rec, format!("replay {input}")))?;
        let data = read(&path)?;
        if is_across_rec(&data) {
            return self.load_across(path, &data, ghosts, level_file);
        }
        let rec = Rec::parse(&data).map_err(format(FileKind::Rec, &path))?;
        let level = if let Some(internal) = internal_level_file(&rec.level_filename) {
            internal_level(&self.res()?, internal)?
        } else {
            match level_file {
                Some(lev) => {
                    Level::parse(&read(lev)?, false).map_err(format(FileKind::Lev, lev))?
                }
                None => self.find_level(&path, &rec)?,
            }
        };

        let mut warnings = Vec::new();
        let lgr = self.lgr(&level, &mut warnings)?;

        let shirt = self.shirt(nick, &mut warnings);
        let mut others = Vec::new();
        for ghost in ghosts {
            let path = self
                .resolve_replay(&ghost.path)
                .ok_or_else(|| not_found(FileKind::Rec, format!("replay {}", ghost.path)))?;
            let other = read_rec(&path)?;
            let shirt = self.shirt(&ghost.nick, &mut warnings);
            others.push((path.display().to_string(), other, shirt));
        }
        let mut loaded = assemble(path, rec, level, lgr, shirt, others)?;
        loaded.warnings = warnings;
        Ok(loaded)
    }
}

// An Elma replay, or an Across one in Elma's terms
pub fn read_rec(path: &Path) -> Result<Rec, Error> {
    parse_rec(&read(path)?).map_err(format(FileKind::Rec, path))
}

pub fn parse_rec(data: &[u8]) -> Result<Rec, elma_formats::Error> {
    if is_across_rec(data) {
        AcrossRec::parse(data).map(|r| r.rec)
    } else {
        Rec::parse(data)
    }
}

// The path with links and relative parts resolved, to tell whether a replay is loaded
pub fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

// An internal level, packed in elma.res
pub fn internal_level(res: &Res, name: &str) -> Result<Level, Error> {
    let data = res
        .get(name)
        .ok_or_else(|| not_found(FileKind::ElmaRes, format!("{name} in elma.res")))?;
    Level::parse(data, true).map_err(format(FileKind::ElmaRes, name))
}

// The .rec files in `dir`, sorted by name, that are replays of level `level_id`
pub fn same_level_replays(dir: &Path, level_id: i32) -> Result<Vec<(PathBuf, Rec)>, Error> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| Error::Io {
            path: dir.to_path_buf(),
            source: Arc::new(e),
        })?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("rec")))
        .collect();
    paths.sort();
    Ok(paths
        .into_iter()
        .filter_map(|path| {
            let rec = read_rec(&path).ok()?;
            (rec.level_id == level_id).then_some((path, rec))
        })
        .collect())
}

// Puts a replay together with its level, LGR and ghosts, checking that they belong together.
// Each ghost is named for errors and comes with its shirt.
pub fn assemble(
    path: PathBuf,
    rec: Rec,
    level: Level,
    lgr: Lgr,
    shirt: Option<Pic8>,
    ghosts: Vec<(String, Rec, Option<Pic8>)>,
) -> Result<Loaded, Error> {
    if level.level_id != rec.level_id {
        return Err(Error::LevelChanged {
            level: rec.level_filename,
            replay: path.display().to_string(),
        });
    }
    if level.topology_errors {
        return Err(Error::TopologyErrors {
            level: rec.level_filename,
        });
    }
    let mut riders: Vec<(Arc<Ride>, Option<Pic8>)> = rec
        .rides
        .iter()
        .map(|r| (Arc::new(r.clone()), None))
        .collect();
    riders[0].1 = shirt;
    for (name, ghost, shirt) in ghosts {
        if ghost.level_id != rec.level_id {
            return Err(Error::NotSameLevel {
                ghost: name,
                level: rec.level_filename,
            });
        }
        add_rides(&mut riders, &ghost, shirt);
    }
    Ok(Loaded {
        level_only: false,
        best_time: level.best_time(rec.multiplayer),
        path,
        rec,
        level,
        look: Graphics::Elma(Box::new(lgr)),
        riders,
        warnings: Vec::new(),
        settings: Settings::default(),
    })
}

// Every ride of a merged replay, so both bikes of a multiplayer one are drawn. The shirt is
// for the first, the replay's own player, and the other keeps the default body.
fn add_rides(riders: &mut Vec<(Arc<Ride>, Option<Pic8>)>, rec: &Rec, shirt: Option<Pic8>) {
    let mut shirt = Some(shirt);
    for ride in &rec.rides {
        riders.push((Arc::new(ride.clone()), shirt.take().flatten()));
    }
}

// A level on its own, with the bike waiting at the start
pub fn assemble_level(path: PathBuf, level: Level, lgr: Lgr) -> Result<Loaded, Error> {
    if level.topology_errors {
        return Err(Error::TopologyErrors {
            level: path.display().to_string(),
        });
    }
    let rec = replay::start_rec(&level);
    let riders = vec![(Arc::new(rec.rides[0].clone()), None)];
    Ok(Loaded {
        level_only: true,
        best_time: level.best_time(false),
        path,
        rec,
        level,
        look: Graphics::Elma(Box::new(lgr)),
        riders,
        warnings: Vec::new(),
        settings: Settings::default(),
    })
}

impl Loaded {
    // The riders that are not the main replay's
    pub fn ghosts(&self) -> Vec<Arc<Ride>> {
        self.riders[self.rec.rides.len()..]
            .iter()
            .map(|(ride, _)| ride.clone())
            .collect()
    }

    pub fn shirts(&self) -> Vec<Option<Pic8>> {
        self.riders.iter().map(|(_, shirt)| shirt.clone()).collect()
    }

    pub fn add_ghost(&mut self, rec: &Rec) {
        add_rides(&mut self.riders, rec, None);
    }

    // When each rider reaches the exit with every apple, in game time, in the order of `riders`
    // The main replay's riders finish together, so a two player replay's apples all count
    pub fn finish_times(&self) -> Vec<Option<f64>> {
        let main = self.rec.rides.len();
        let mut times = replay::finish_times(&self.level, &self.rec, self.settings.clone());
        times.extend(self.riders.iter().skip(main).map(|(ride, _)| {
            replay::finish_time(&self.level, &self.rec, ride, self.settings.clone())
        }));
        times
    }

    // Leaves out the ghosts that don't finish, and returns how many
    pub fn retain_finishers(&mut self) -> usize {
        let main = self.rec.rides.len();
        let mut finishes = self.finish_times().into_iter().enumerate();
        let before = self.riders.len();
        self.riders
            .retain(|_| matches!(finishes.next(), Some((i, t)) if i < main || t.is_some()));
        before - self.riders.len()
    }

    // Makes the fastest finish the main replay, and the other riders ghosts. Returns its
    // time, or None if nothing finishes. With `only_finishes` a main replay that doesn't
    // finish is left out once it becomes a ghost.
    pub fn follow_fastest(&mut self, only_finishes: bool) -> Option<f64> {
        let times = self.finish_times();
        let (i, time) = times
            .iter()
            .enumerate()
            .filter_map(|(i, t)| t.map(|t| (i, t)))
            .min_by(|a, b| a.1.total_cmp(&b.1))?;
        if i != 0 || self.rec.rides.len() > 1 {
            let followed = self.riders.remove(i);
            if only_finishes {
                let mut finished = times.iter().enumerate().filter(|(j, _)| *j != i);
                self.riders
                    .retain(|_| finished.next().is_some_and(|(_, t)| t.is_some()));
            }
            self.rec.multiplayer = false;
            self.rec.flagtag = false;
            self.rec.rides = vec![(*followed.0).clone()];
            self.riders.insert(0, followed);
        }
        Some(time)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use elma_formats::rec::Frame;

    fn rec(rides: usize, x: f32) -> Rec {
        let ride = Ride {
            frames: vec![Frame {
                bike_x: x,
                ..Default::default()
            }],
            events: vec![],
            frame_rate: elma_formats::rec::FRAME_RATE,
        };
        Rec {
            multiplayer: rides > 1,
            flagtag: false,
            level_id: 1,
            level_filename: String::new(),
            rides: vec![ride; rides],
        }
    }

    fn shirt(color: u8) -> Option<Pic8> {
        Some(Pic8 {
            width: 1,
            height: 1,
            pixels: vec![color],
        })
    }

    #[test]
    fn merged_replays_keep_their_order_and_shirts() {
        let mut riders = Vec::new();
        add_rides(&mut riders, &rec(1, 1.0), shirt(1));
        add_rides(&mut riders, &rec(2, 2.0), shirt(2));
        add_rides(&mut riders, &rec(1, 3.0), None);
        add_rides(&mut riders, &rec(1, 4.0), shirt(4));
        let xs: Vec<f32> = riders.iter().map(|(r, _)| r.frames[0].bike_x).collect();
        assert_eq!(xs, [1.0, 2.0, 2.0, 3.0, 4.0]);
        let shirts: Vec<Option<u8>> = riders
            .iter()
            .map(|(_, s)| s.as_ref().map(|s| s.pixels[0]))
            .collect();
        assert_eq!(shirts, [Some(1), Some(2), None, None, Some(4)]);
    }
}
