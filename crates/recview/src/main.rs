// A window that plays a replay
// No console window next to it on Windows. Errors are shown in dialogs.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
#[cfg(target_os = "macos")]
mod macos;
mod sound;

use audio::Sounds;
use game_data::{GameFolder, Ghost, find_file, is_across_rec};
use player::{Content, Player, Settings, ZOOM_STEP};
use render::{
    GpuScene, MAX_ZOOM, MIN_ZOOM, Output, Renderer, Scene, SceneOptions, Targets, acquire,
    surface_format,
};
use sound::Speaker;
use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowId};

const USAGE: &str = "Usage: recview [input.rec | input.lev] [options]

Plays a replay in a window. Without a replay, open one with O or drop .rec files
on the window: the first is played, the others are drawn as ghosts. Replays of the
level that is playing are added to it as ghosts; others replace it.

A .lev opens the level on its own to look around: drag to move, the wheel to zoom
and R to see all of it again. Dropping a replay of it plays the replay. A replay
finds its level in lev/ or next to it; when it doesn't, the level opened next is
the one it plays on, as is a level opened together with it.

Options:
  --data <dir>        game folder with elma.res, lev/, lgr/, rec/ and bmp/. By default
                      the folder the replay is in, or the one chosen last time.
  --merge <rec>       draw another replay as a ghost (repeatable)
  --nick <nick>       shirt (bmp/<nick>.bmp) for the replay named before it
  --zoom <z>          starting zoom (default: the zoom used last time, or 1)
  --no-audio          play without sound (M turns the sound on and off)
  -h, --help          show this help

The settings, the sound and the zoom are remembered for the next launch.

Keys, the same as the web player's:
  space               play / pause (a click too)
  - / + or =          slower / faster
  backspace           normal speed
  R                   reverse
  left, right         back / forward 2.5 seconds
  , / .               back / forward one frame
  W / E, wheel        zoom out / in
  0                   zoom 1.00x, the game's
  F                   show the whole level
  C                   back onto the bike, after dragging it away
  N, shift N          follow the next / previous bike
  P                   pictures, and the level's own ground and sky
  G, shift G          grass / grass, pictures and the level's own sky on
  A                   gravity arrows
  T                   timer
  V                   minimap
  X                   last apple time

Keys only recview has:
  K                   centered camera, or the game's classic one
  S                   split screen for two player replays
  B                   ghosts blue and see-through, or like any bike
  Z                   auto zoom: zoom out to keep every ghost in view
  Y                   zoom textures: the ground, sky and qgrass grow with the zoom, or
                      keep their size at zoom 1
  M                   sound on / off
  L                   loop on / off
  O                   open replays or a level
  shift left, right   the previous / next replay or level in its folder
  shift O             choose the game folder, and play again from it
  escape, Q           quit";

struct Args {
    input: Option<String>,
    data: Option<PathBuf>,
    nick: String,
    ghosts: Vec<Ghost>,
    zoom: Option<f64>,
    audio: bool,
}

fn parse_arguments() -> Result<Args, String> {
    let mut args = Args {
        input: None,
        data: None,
        nick: String::new(),
        ghosts: Vec::new(),
        zoom: None,
        audio: true,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut value = |name: &str| it.next().ok_or_else(|| format!("{name} needs a value"));
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            "--data" => args.data = Some(PathBuf::from(value("--data")?)),
            "--no-audio" => args.audio = false,
            "--merge" => args.ghosts.push(Ghost {
                path: value("--merge")?,
                nick: String::new(),
            }),
            "--nick" => {
                let nick = value("--nick")?;
                match args.ghosts.last_mut() {
                    Some(ghost) => ghost.nick = nick,
                    None => args.nick = nick,
                }
            }
            "--zoom" => {
                let zoom = value("--zoom")?;
                args.zoom = Some(
                    zoom.parse()
                        .ok()
                        .filter(|&z: &f64| z > 0.0 && z.is_finite())
                        .ok_or_else(|| format!("invalid zoom: {zoom}"))?,
                );
            }
            // Finder passes this when launching an app bundle on older macOS versions
            _ if arg.starts_with("-psn_") => {}
            _ if arg.starts_with('-') => return Err(format!("unknown option: {arg}")),
            _ if args.input.is_none() => args.input = Some(arg),
            _ => return Err(format!("unexpected argument: {arg}")),
        }
    }
    Ok(args)
}

fn show_error(message: impl std::fmt::Display) {
    eprintln!("recview: {message}");
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title("recview")
        .set_description(message.to_string())
        .show();
}

// Where recview remembers the game folder and the view settings
fn config_dir() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    Some(if cfg!(target_os = "macos") {
        home.join("Library/Application Support/recview")
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"))
            .join("recview")
    })
}

// The Elma folder, or the Across one
fn saved_folder_file(across: bool) -> Option<PathBuf> {
    let name = if across {
        "across-folder"
    } else {
        "game-folder"
    };
    Some(config_dir()?.join(name))
}

// The settings, kept between launches
#[derive(Debug, Clone, Copy, PartialEq)]
struct Prefs {
    settings: Settings,
    muted: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            settings: Settings::default(),
            muted: true,
        }
    }
}

// An on/off setting of the prefs
type Flag = fn(&mut Prefs) -> &mut bool;

impl Prefs {
    // The on/off settings by their key in the file
    const FLAGS: [(&str, Flag); 18] = [
        ("centered_camera", |p| &mut p.settings.centered_camera),
        ("split_screen", |p| &mut p.settings.split_screen),
        ("timer", |p| &mut p.settings.timer),
        ("minimap", |p| &mut p.settings.minimap),
        ("last_apple_time", |p| &mut p.settings.last_apple_time),
        ("muted", |p| &mut p.muted),
        ("loop", |p| &mut p.settings.looping),
        ("translucent_ghosts", |p| &mut p.settings.translucent_ghosts),
        ("auto_zoom", |p| &mut p.settings.auto_zoom),
        ("pictures_in_background", |p| {
            &mut p.settings.pictures_in_background
        }),
        ("zoom_textures", |p| &mut p.settings.zoom_textures),
        ("grass", |p| &mut p.settings.level.grass),
        ("pictures", |p| &mut p.settings.level.pictures),
        ("custom_sky", |p| &mut p.settings.level.custom_sky),
        ("arrows", |p| &mut p.settings.level.arrows),
        ("animated_objects", |p| {
            &mut p.settings.level.animated_objects
        }),
        ("still_objects", |p| &mut p.settings.level.still_objects),
        ("fancyboost", |p| &mut p.settings.level.fancyboost),
    ];

    fn file() -> Option<PathBuf> {
        Some(config_dir()?.join("settings"))
    }

    // One key=value per line; unknown or broken lines keep the default
    fn load() -> Prefs {
        let mut prefs = Prefs::default();
        let Some(text) = Prefs::file().and_then(|f| std::fs::read_to_string(f).ok()) else {
            return prefs;
        };
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            match key.trim() {
                "zoom" => {
                    if let Ok(z) = value.trim().parse::<f64>()
                        && z > 0.0
                        && z.is_finite()
                    {
                        prefs.settings.zoom = z.clamp(MIN_ZOOM, MAX_ZOOM);
                    }
                }
                // Written by older versions, which had a choice of three
                "camera" => prefs.settings.centered_camera = value.trim() != "classic",
                key => {
                    if let Some((_, flag)) = Prefs::FLAGS.iter().find(|(k, _)| *k == key) {
                        *flag(&mut prefs) = value.trim() == "true";
                    }
                }
            }
        }
        prefs
    }

    fn save(&self) {
        let Some(file) = Prefs::file() else {
            return;
        };
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let mut text = format!(
            "zoom={}
",
            self.settings.zoom
        );
        let mut prefs = *self;
        for (key, flag) in Prefs::FLAGS {
            text += &format!(
                "{key}={}
",
                flag(&mut prefs)
            );
        }
        let _ = std::fs::write(file, text);
    }
}

fn saved_folder(across: bool) -> Option<PathBuf> {
    let text = std::fs::read_to_string(saved_folder_file(across)?).ok()?;
    Some(PathBuf::from(text.trim()))
}

fn save_folder(folder: &GameFolder) {
    if let Some(file) = saved_folder_file(folder.is_across()) {
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(file, folder.dir.to_string_lossy().as_bytes());
    }
}

// The replay's file name, the level's name, and how many ghosts it is drawn with
fn title_name(replay: &Path, level: &str, ghosts: usize) -> String {
    let mut name = replay
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let level = level.trim();
    if !level.is_empty() {
        name += &format!(" \"{level}\"");
    }
    if ghosts > 0 {
        name += &format!(" + {ghosts} ghost{}", if ghosts == 1 { "" } else { "s" });
    }
    name
}

fn level_id_of(path: &str) -> Option<i32> {
    Some(game_data::read_rec(Path::new(path)).ok()?.level_id)
}

// Whether `path` is an Across replay, which needs an Across folder
fn is_across_replay(path: &Path) -> bool {
    std::fs::read(path).is_ok_and(|data| is_across_rec(&data))
}

// The game folder a replay is in, such as elma/ for elma/rec/x.rec
fn folder_of(replay: &Path, across: bool) -> Option<PathBuf> {
    replay
        .canonicalize()
        .ok()?
        .ancestors()
        .skip(1)
        .find(|dir| match across {
            false => find_file(dir, "elma.res").is_some(),
            true => find_file(dir, "across.res").is_some(),
        })
        .map(Path::to_path_buf)
}

fn pick_folder(across: bool) -> Option<GameFolder> {
    let title = match across {
        false => "Choose your Elasto Mania folder (the one with elma.res)",
        true => "Choose your Across folder (the one with across.res)",
    };
    loop {
        let dir = rfd::FileDialog::new().set_title(title).pick_folder()?;
        match GameFolder::open_game(&dir, across) {
            Ok(folder) => {
                save_folder(&folder);
                return Some(folder);
            }
            Err(e) => show_error(&e),
        }
    }
}

enum LevelChoice {
    File(PathBuf),
    Folder,
}

// Asks where the level of `replay` is, when lev/ doesn't have it: `name` is the file an
// Elma replay asks for. None when cancelled.
fn ask_for_level(name: Option<&str>, replay: &Path, across: bool) -> Option<LevelChoice> {
    let description = match name {
        Some(name) => format!(
            "The level of this replay, {name}, isn't in the game folder's lev/ or next to \
             the replay."
        ),
        None => "None of the levels in the game folder's lev/ or next to the replay is the one \
                 it was recorded on."
            .to_string(),
    };
    let (file, folder) = ("Choose the level…", "Choose another folder…");
    let answer = rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Warning)
        .set_title("recview")
        .set_description(description)
        .set_buttons(rfd::MessageButtons::YesNoCancelCustom(
            file.into(),
            folder.into(),
            "Cancel".into(),
        ))
        .show();
    // Windows shows Yes and No without common controls 6
    match answer {
        rfd::MessageDialogResult::Yes => {}
        rfd::MessageDialogResult::Custom(c) if c == file => {}
        rfd::MessageDialogResult::No => return Some(LevelChoice::Folder),
        rfd::MessageDialogResult::Custom(c) if c == folder => return Some(LevelChoice::Folder),
        _ => return None,
    }
    let extensions: &[&str] = match across {
        false => &["lev", "LEV"],
        true => &["lev", "LEV", "txt", "TXT"],
    };
    let mut dialog = rfd::FileDialog::new()
        .set_title(match name {
            Some(name) => format!("Find {name}"),
            None => "Find the level of this replay".into(),
        })
        .add_filter("Level", extensions);
    if let Some(dir) = replay.parent().filter(|d| d.is_dir()) {
        dialog = dialog.set_directory(dir);
    }
    dialog.pick_file().map(LevelChoice::File)
}

fn is_level(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("lev"))
}

// Names compared without case and with the numbers in them as numbers, so lev2 comes
// before lev10, and 01b0ne.rec before 01b0ne1.rec
fn natural_order(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.as_bytes(), b.as_bytes());
    loop {
        match (a.first(), b.first()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let digits = |s: &[u8]| s.iter().take_while(|c| c.is_ascii_digit()).count();
                let (da, db) = (digits(a), digits(b));
                let trim = |s: &[u8]| -> Vec<u8> {
                    s.iter().copied().skip_while(|&c| c == b'0').collect()
                };
                let (na, nb) = (trim(&a[..da]), trim(&b[..db]));
                let order = na.len().cmp(&nb.len()).then_with(|| na.cmp(&nb));
                if order != Ordering::Equal {
                    return order;
                }
                (a, b) = (&a[da..], &b[db..]);
            }
            (Some(x), Some(y)) => {
                let order = x.to_ascii_lowercase().cmp(&y.to_ascii_lowercase());
                if order != Ordering::Equal {
                    return order;
                }
                (a, b) = (&a[1..], &b[1..]);
            }
        }
    }
}

// The file before (-1) or after (1) `path` in its folder, of the same kind: a replay's
// neighbour is a replay, a level's a level. After the last comes the first again.
fn neighbour(path: &Path, step: i32) -> Option<PathBuf> {
    let extension = path.extension()?;
    let dir = match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir,
        _ => Path::new("."),
    };
    let name = path.file_name()?.to_string_lossy().into_owned();
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|t| t.is_file()))
        .map(|entry| PathBuf::from(entry.file_name()))
        .filter(|file| {
            file.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case(extension))
        })
        .map(|file| file.to_string_lossy().into_owned())
        .collect();
    names.sort_by(|a, b| natural_order(a, b).then_with(|| a.cmp(b)));
    // Where the file is, or would be if it was deleted since
    let at =
        names.binary_search_by(|n| natural_order(n, &name).then_with(|| n.as_str().cmp(&name)));
    let len = names.len();
    if len == 0 {
        return None;
    }
    let index = match (at, step > 0) {
        (Ok(i), true) => (i + 1) % len,
        (Ok(i) | Err(i), false) => (i + len - 1) % len,
        (Err(i), true) => i % len,
    };
    // The only one of its kind has no other to go to
    Some(&names[index])
        .filter(|n| **n != name)
        .map(|n| dir.join(n))
}

// A level to look around, opened like a replay
fn level_only(level: &Path) -> Opened {
    Opened {
        input: level.to_string_lossy().into_owned(),
        nick: String::new(),
        ghosts: Vec::new(),
    }
}

// A replay with its ghosts, as it was opened
#[derive(Clone)]
struct Opened {
    input: String,
    nick: String,
    ghosts: Vec<Ghost>,
}

struct Current {
    // What was opened, so ghosts can be added later
    opened: Opened,
    // The level chosen for the replay when lev/ didn't have it
    level_file: Option<PathBuf>,
    level_id: i32,
    name: String,
    // What the scene is built from, to build it again when the level options change
    loaded: game_data::Loaded,
    scene: GpuScene,
    built_with: SceneOptions,
    speaker: Option<Speaker>,
}

struct Gpu {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
    targets: Targets,
}

struct App {
    args: Args,
    folder: Option<GameFolder>,
    // Whether `folder` was chosen with shift O, which makes it win over the one a replay is in
    folder_chosen: bool,
    // The replay whose level wasn't found, to play on the level opened next
    waiting: Option<Opened>,
    // The sounds and the folder and game they were loaded for
    sounds: Option<((PathBuf, bool), Arc<Sounds>)>,
    gpu: Option<Gpu>,
    current: Option<Current>,
    // Playback, the camera and the settings
    player: Player,
    muted: bool,
    // The settings last written to disk
    saved: Prefs,
    last_save: Instant,
    // Files waiting to be opened, collected until the window is idle
    pending: Vec<PathBuf>,
    // The file shift and an arrow went to, when it didn't open, so the next press goes on
    // from it instead of trying it again
    stepped_to: Option<PathBuf>,
    modifiers: ModifiersState,
    error: Option<String>,
    last_frame: Instant,
    last_title: Instant,
    cursor: (f64, f64),
    // Where the left button went down, and whether it dragged since: a click plays or pauses
    pressed: Option<(f64, f64)>,
    dragged: bool,
}

impl App {
    fn open_window(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("recview")
                        .with_inner_size(LogicalSize::new(1280.0, 800.0)),
                )
                .map_err(|e| e.to_string())?,
        );
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| e.to_string())?;
        let (renderer, adapter) =
            pollster::block_on(Renderer::request(&instance, Some(&surface), "recview"))
                .map_err(|e| e.to_string())?;

        let size = window.inner_size();
        let (width, height) = (size.width.max(1), size.height.max(1));
        let format = surface_format(&surface.get_capabilities(&adapter))
            .ok_or("the window surface is not supported")?;
        let mut config = surface
            .get_default_config(&adapter, width, height)
            .ok_or("the window surface is not supported")?;
        config.format = format;
        config.present_mode = wgpu::PresentMode::AutoVsync;
        surface.configure(renderer.device(), &config);
        let targets = renderer.create_targets(width, height);
        self.player.resize(width, height, window.scale_factor());
        self.gpu = Some(Gpu {
            window,
            surface,
            config,
            renderer,
            targets,
        });
        self.last_frame = Instant::now();
        Ok(())
    }

    // The folder to load `replay` from: one chosen with shift O, --data, the folder it is in,
    // the one chosen last time, or ask
    fn choose_folder(&mut self, replay: &Path) -> Option<()> {
        // A level could be either game's, so it goes with the open folder
        let across = if is_level(replay) {
            self.folder.as_ref().is_some_and(GameFolder::is_across)
        } else {
            is_across_replay(replay)
        };
        let open = self.folder.as_ref().filter(|f| f.is_across() == across);
        let wanted = open
            .filter(|_| self.folder_chosen)
            .map(|f| f.dir.clone())
            .or_else(|| self.args.data.clone())
            .or_else(|| folder_of(replay, across))
            .or_else(|| open.map(|f| f.dir.clone()))
            .or_else(|| saved_folder(across));
        match wanted {
            Some(dir) if open.is_some_and(|f| f.dir == dir) => {}
            Some(dir) => match GameFolder::open_game(&dir, across) {
                Ok(folder) => self.folder = Some(folder),
                Err(e) => {
                    show_error(&e);
                    self.folder = Some(pick_folder(across)?);
                }
            },
            None => self.folder = Some(pick_folder(across)?),
        }
        Some(())
    }

    // Play the first replay, with the others as ghosts, on `level_file` or the level it
    // finds. A merge adds ghosts to the replay playing, or follows another of its replays,
    // carrying on from the same moment.
    fn open(&mut self, opened: Opened, level_file: Option<PathBuf>, merge: bool) {
        if self.gpu.is_none() {
            return;
        }
        let replay = PathBuf::from(&opened.input);
        if self.choose_folder(&replay).is_none() {
            return;
        }
        let mut level_file = level_file;
        let loaded = loop {
            let folder = self.folder.as_ref().unwrap();
            let loaded = if is_level(&replay) {
                folder.load_level(&opened.input)
            } else {
                folder.load(
                    &opened.input,
                    &opened.nick,
                    &opened.ghosts,
                    level_file.as_deref(),
                )
            };
            match loaded {
                Ok(loaded) => break loaded,
                Err(game_data::Error::MissingLevel(name)) => {
                    let across = folder.is_across();
                    match ask_for_level(name.as_deref(), &replay, across) {
                        Some(LevelChoice::File(file)) => level_file = Some(file),
                        Some(LevelChoice::Folder) => {
                            level_file = None;
                            let Some(picked) = pick_folder(across) else {
                                self.waiting = Some(opened);
                                return;
                            };
                            self.folder = Some(picked);
                            self.folder_chosen = true;
                        }
                        None => {
                            self.waiting = Some(opened);
                            return;
                        }
                    }
                }
                Err(e) => return show_error(&e),
            }
        };
        self.show(opened, level_file, loaded, merge);
    }

    // Plays `opened` on `level`, or shows the level on its own when the replay wasn't
    // recorded on it, and the replay keeps waiting for its level
    fn open_on_level(&mut self, opened: Opened, level: PathBuf) {
        if self.gpu.is_none() || self.choose_folder(Path::new(&opened.input)).is_none() {
            return;
        }
        let folder = self.folder.as_ref().unwrap();
        match folder.load(&opened.input, &opened.nick, &opened.ghosts, Some(&level)) {
            Ok(loaded) => self.show(opened, Some(level), loaded, false),
            Err(game_data::Error::LevelChanged { .. } | game_data::Error::WrongLevel { .. }) => {
                self.waiting = Some(opened);
                self.open(level_only(&level), None, false);
            }
            Err(e) => show_error(&e),
        }
    }

    fn show(
        &mut self,
        opened: Opened,
        level_file: Option<PathBuf>,
        loaded: game_data::Loaded,
        merge: bool,
    ) {
        for warning in &loaded.warnings {
            eprintln!("recview: {warning}");
        }
        let was_level = self.player.level_only();
        let built_with = self.player.scene_options();
        let gpu = self.gpu.as_ref().unwrap();
        let scene = match build_scene(&gpu.renderer, &loaded, &built_with) {
            Ok(scene) => scene,
            Err(e) => return show_error(e),
        };
        // Adding a ghost keeps the sound going instead of starting the motors again, unless
        // the sounds are another folder's
        let folder = self.folder.as_ref().unwrap();
        let same_sounds = self
            .sounds
            .as_ref()
            .is_some_and(|((dir, across), _)| *dir == folder.dir && *across == folder.is_across());
        let reused = match merge && same_sounds {
            true => self.current.as_mut().and_then(|c| c.speaker.take()),
            false => None,
        };
        let speaker = match loaded.level_only {
            true => None,
            false => reused.or_else(|| self.make_speaker(loaded.rec.rides.len())),
        };
        self.player.load(Content::new(&loaded), merge);
        // A replay that is opened plays, at the zoom replays had before a level was shown whole
        if !merge {
            self.player.set_playing(true);
        }
        if was_level && !loaded.level_only {
            self.player.set_zoom(self.saved.settings.zoom);
            self.player.recenter();
        }
        if !loaded.level_only {
            self.waiting = None;
        }
        self.stepped_to = None;
        self.current = Some(Current {
            name: title_name(&loaded.path, &loaded.level.name, opened.ghosts.len()),
            opened,
            level_file,
            level_id: loaded.rec.level_id,
            loaded,
            scene,
            built_with,
            speaker,
        });
        self.last_frame = Instant::now();
        self.gpu.as_ref().unwrap().window.focus_window();
    }

    // What is shown again from the folder chosen with shift O: the replay waiting for its
    // level, or the one playing, from the same moment
    fn reopen(&mut self) {
        if let Some(waiting) = self.waiting.take() {
            return self.open(waiting, None, false);
        }
        if let Some(current) = &self.current {
            let (opened, level_file) = (current.opened.clone(), current.level_file.clone());
            self.open(opened, level_file, true);
        }
    }

    // The sounds are loaded once per game folder
    fn make_speaker(&mut self, riders: usize) -> Option<Speaker> {
        if !self.args.audio {
            return None;
        }
        let folder = self.folder.as_ref()?;
        let sounds = match &self.sounds {
            Some(((dir, across), sounds))
                if *dir == folder.dir && *across == folder.is_across() =>
            {
                Ok(sounds.clone())
            }
            _ => Sounds::load_files(&|name| folder.file(name), folder.is_across())
                .map(Arc::new)
                .map_err(|e| e.to_string()),
        };
        if let Ok(sounds) = &sounds {
            self.sounds = Some(((folder.dir.clone(), folder.is_across()), sounds.clone()));
        }
        match sounds.and_then(|s| Speaker::open(s, riders)) {
            Ok(mut speaker) => {
                speaker.muted = self.muted;
                Some(speaker)
            }
            Err(e) => {
                eprintln!("recview: no sound: {e}");
                None
            }
        }
    }

    // Plays the replay before or after the one playing in its folder, on its own, or shows
    // the level before or after the one shown
    fn step_file(&mut self, step: i32) {
        let Some(current) = &self.current else {
            return;
        };
        let from = match &self.stepped_to {
            Some(path) => path.clone(),
            None => PathBuf::from(&current.opened.input),
        };
        let Some(next) = neighbour(&from, step) else {
            return;
        };
        self.stepped_to = Some(next.clone());
        if is_level(&next) {
            return self.open(level_only(&next), None, false);
        }
        // The level chosen for the replay before is the one of the next, if it is the same
        let level_id = current.level_id;
        let level_file = current.level_file.clone().filter(|_| {
            !self.player.level_only() && level_id_of(&next.to_string_lossy()) == Some(level_id)
        });
        let opened = Opened {
            input: next.to_string_lossy().into_owned(),
            nick: String::new(),
            ghosts: Vec::new(),
        };
        self.open(opened, level_file, false);
    }

    fn open_pending(&mut self) {
        if self.pending.is_empty() || self.gpu.is_none() {
            return;
        }
        self.stepped_to = None;
        let paths = std::mem::take(&mut self.pending);
        let level = paths.iter().find(|p| is_level(p)).cloned();
        let recs: Vec<String> = paths
            .iter()
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("rec")))
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        // A level on its own plays the replay waiting for it, if there is one
        if let Some(level) = level.clone().filter(|_| recs.is_empty()) {
            return match self.waiting.take() {
                Some(waiting) => self.open_on_level(waiting, level),
                None => self.open(level_only(&level), None, false),
            };
        }
        if recs.is_empty() {
            return show_error("only .rec and .lev files can be opened");
        }
        let as_ghost = |path: &String| Ghost {
            path: path.clone(),
            nick: String::new(),
        };

        // Replays of the level that is playing join it as ghosts
        if let Some(current) = self.current.as_ref().filter(|_| !self.player.level_only()) {
            let canonical = |p: &String| game_data::canonical(Path::new(p));
            let opened = &current.opened;
            let loaded: Vec<PathBuf> = std::iter::once(&opened.input)
                .chain(opened.ghosts.iter().map(|g| &g.path))
                .map(canonical)
                .collect();
            let new: Vec<&String> = recs
                .iter()
                .filter(|r| !loaded.contains(&canonical(r)))
                .collect();
            if new.is_empty() {
                return;
            }
            if level.is_none() && new.iter().all(|r| level_id_of(r) == Some(current.level_id)) {
                let mut opened = opened.clone();
                opened.ghosts.extend(new.into_iter().map(as_ghost));
                return self.open(opened, current.level_file.clone(), true);
            }
        }

        // Otherwise the first replay starts over, with the ones of the same level as ghosts,
        // on the level opened with it
        let (main, others) = recs.split_first().unwrap();
        let level_id = level_id_of(main);
        let opened = Opened {
            input: main.clone(),
            nick: String::new(),
            ghosts: others
                .iter()
                .filter(|r| level_id_of(r) == level_id)
                .map(as_ghost)
                .collect(),
        };
        match level {
            Some(level) => self.open_on_level(opened, level),
            None => self.open(opened, None, false),
        }
    }

    fn save_prefs(&mut self) {
        let mut settings = *self.player.settings();
        // The zoom that shows a level whole isn't the one to watch replays at
        if self.player.level_only() {
            settings.zoom = self.saved.settings.zoom;
        }
        let prefs = Prefs {
            settings,
            muted: self.muted,
        };
        if prefs != self.saved {
            prefs.save();
            self.saved = prefs;
        }
    }

    fn resize(&mut self, width: u32, height: u32) {
        let Some(gpu) = &mut self.gpu else {
            return;
        };
        let (width, height) = (width.max(1), height.max(1));
        gpu.config.width = width;
        gpu.config.height = height;
        gpu.surface.configure(gpu.renderer.device(), &gpu.config);
        gpu.targets = gpu.renderer.create_targets(width, height);
        self.player.resize(width, height, gpu.window.scale_factor());
    }

    fn update_title(&mut self) {
        let Some(gpu) = &self.gpu else {
            return;
        };
        let Some(current) = &self.current else {
            gpu.window
                .set_title("recview - open a replay with O, or drop .rec files here");
            return;
        };
        let p = &self.player;
        if p.level_only() {
            gpu.window.set_title(&format!(
                "recview - {} - zoom {:.2}",
                current.name,
                p.shown_zoom()
            ));
            return;
        }
        let (t, duration) = (p.shown_time(), p.shown_duration());
        gpu.window.set_title(&format!(
            "recview - {} - {:02}:{:05.2} / {:02}:{:05.2} - {:.2}x - zoom {:.2}{}{}",
            current.name,
            (t / 60.0) as u32,
            t % 60.0,
            (duration / 60.0) as u32,
            duration % 60.0,
            p.speed(),
            p.shown_zoom(),
            if p.is_playing() { "" } else { " - paused" },
            if self.muted { " - muted" } else { "" },
        ));
    }

    fn redraw(&mut self) {
        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f64().min(0.25);
        self.last_frame = now;
        if (now - self.last_title).as_secs_f64() > 0.1 {
            self.last_title = now;
            self.update_title();
        }
        let Some(gpu) = &mut self.gpu else {
            return;
        };

        let tick = self.player.tick(dt);
        if let (Some(current), Some(session)) = (&mut self.current, self.player.session()) {
            if let Some(speaker) = &mut current.speaker {
                // Sounds skipped over by a jump are not played
                if tick.seeked {
                    speaker.seeked(session);
                }
                speaker.update(session, tick.step.as_ref(), self.player.is_playing());
            }
            // The level options changed what the scene is built with
            let options = self.player.scene_options();
            if current.built_with != options {
                match build_scene(&gpu.renderer, &current.loaded, &options) {
                    Ok(scene) => current.scene = scene,
                    Err(e) => eprintln!("recview: {e}"),
                }
                current.built_with = options;
            }
        }

        let Some(frame) = acquire(&gpu.surface, gpu.renderer.device(), &gpu.config) else {
            return;
        };
        let view = frame.texture.create_view(&Default::default());
        let views = self.player.views(dt);
        let options = self.player.render_options();
        let output = Output {
            view: &view,
            format: gpu.config.format,
        };
        match (&self.current, self.player.session()) {
            (Some(current), Some(session)) => {
                let rendered = gpu.renderer.render(
                    &current.scene,
                    &gpu.targets,
                    session,
                    &views,
                    &options,
                    output,
                );
                if let Err(e) = rendered {
                    eprintln!("recview: {e}");
                }
            }
            _ => gpu.renderer.clear(output),
        }
        gpu.window.pre_present_notify();
        gpu.renderer.queue().present(frame);
    }

    // A key pressed: the shared keys go to the player, then come recview's own
    fn key(&mut self, event_loop: &ActiveEventLoop, key: &Key, repeat: bool) {
        let name = match key {
            Key::Named(NamedKey::Space) => " ".to_string(),
            Key::Named(NamedKey::ArrowLeft) => "ArrowLeft".to_string(),
            Key::Named(NamedKey::ArrowRight) => "ArrowRight".to_string(),
            Key::Named(NamedKey::Backspace) => "Backspace".to_string(),
            Key::Named(NamedKey::Escape) => "Escape".to_string(),
            Key::Character(c) => c.to_string(),
            _ => return,
        };
        if self.modifiers.shift_key() && name.starts_with("Arrow") {
            if !repeat {
                self.step_file(if name == "ArrowLeft" { -1 } else { 1 });
            }
            return;
        }
        // recview's own keys are all toggles, which a held key flips only once
        if self.player.key(&name, repeat) || repeat {
            return;
        }
        let p = &mut self.player;
        let s = *p.settings();
        match name.to_lowercase().as_str() {
            "escape" | "q" => {
                self.save_prefs();
                event_loop.exit();
            }
            "k" => p.set_centered_camera(!s.centered_camera),
            "s" => p.set_split_screen(!s.split_screen),
            "b" => p.set_translucent_ghosts(!s.translucent_ghosts),
            "z" => p.set_auto_zoom(!s.auto_zoom),
            "y" => p.set_zoom_textures(!s.zoom_textures),
            "l" => p.set_looping(!s.looping),
            "m" => {
                self.muted = !self.muted;
                if let Some(speaker) = self.current.as_mut().and_then(|c| c.speaker.as_mut()) {
                    speaker.muted = self.muted;
                }
            }
            // Shift gives a capital, which a modifier state left over from a dialog can't spoil
            "o" if name == "O" => {
                let across = match &self.waiting {
                    Some(waiting) => is_across_replay(Path::new(&waiting.input)),
                    None => self.folder.as_ref().is_some_and(GameFolder::is_across),
                };
                if let Some(folder) = pick_folder(across) {
                    self.folder = Some(folder);
                    self.folder_chosen = true;
                    self.reopen();
                }
            }
            "o" => {
                let mut dialog = rfd::FileDialog::new()
                    .set_title("Open replays (the first plays, the others are ghosts) or a level")
                    .add_filter("Replay or level", &["rec", "REC", "lev", "LEV"]);
                if let Some(folder) = &self.folder {
                    dialog = dialog.set_directory(folder.dir.join("rec"));
                }
                if let Some(files) = dialog.pick_files() {
                    self.pending.extend(files);
                }
            }
            _ => {}
        }
    }
}

// The scene of what was loaded, uploaded
fn build_scene(
    renderer: &Renderer,
    loaded: &game_data::Loaded,
    options: &SceneOptions,
) -> Result<GpuScene, render::RenderError> {
    let scene = Scene::new(
        &loaded.look,
        &loaded.level,
        &loaded.shirts(),
        options,
        renderer.atlas_limit(),
    )?;
    Ok(renderer.upload_scene(scene))
}

impl ApplicationHandler<Vec<PathBuf>> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gpu.is_some() {
            return;
        }
        if let Err(e) = self.open_window(event_loop) {
            self.error = Some(e);
            event_loop.exit();
            return;
        }
        if let Some(input) = self.args.input.take() {
            let opened = Opened {
                input,
                nick: std::mem::take(&mut self.args.nick),
                ghosts: std::mem::take(&mut self.args.ghosts),
            };
            self.open(opened, None, false);
        }
    }

    // Files opened from Finder
    fn user_event(&mut self, _: &ActiveEventLoop, paths: Vec<PathBuf>) {
        self.pending.extend(paths);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                self.save_prefs();
                event_loop.exit();
            }
            WindowEvent::Resized(size) => self.resize(size.width, size.height),
            WindowEvent::RedrawRequested => self.redraw(),
            WindowEvent::DroppedFile(path) => self.pending.push(path),
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            // Shift let go of in another window doesn't stay held here
            WindowEvent::Focused(false) => self.modifiers = ModifiersState::empty(),
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed {
                    self.key(event_loop, &event.logical_key, event.repeat);
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y as f64,
                    MouseScrollDelta::PixelDelta(p) => p.y / 40.0,
                };
                // A notch zooms as much as the zoom keys
                self.player.zoom_by(ZOOM_STEP.powf(lines));
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if button == MouseButton::Left {
                    match state {
                        ElementState::Pressed => {
                            self.pressed = Some(self.cursor);
                            self.dragged = false;
                        }
                        // A click without dragging plays or pauses, like a tap on the web
                        ElementState::Released => {
                            if self.pressed.take().is_some() && !self.dragged {
                                self.player.toggle_play();
                            }
                        }
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let (x, y) = (position.x, position.y);
                if let Some((px, py)) = self.pressed {
                    // A few pixels of wobble is still a click
                    if !self.dragged && ((x - px).powi(2) + (y - py).powi(2)).sqrt() > 4.0 {
                        self.dragged = true;
                    }
                    if self.dragged {
                        self.player.pan_by(x - self.cursor.0, y - self.cursor.1);
                    }
                }
                self.cursor = (x, y);
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _: &ActiveEventLoop) {
        self.open_pending();
        // Zooming with the wheel changes the settings many times a second
        if self.last_save.elapsed().as_secs_f64() > 1.0 {
            self.last_save = Instant::now();
            self.save_prefs();
        }
        if let Some(gpu) = &self.gpu {
            gpu.window.request_redraw();
        }
    }
}

fn main() -> ExitCode {
    let args = match parse_arguments() {
        Ok(args) => args,
        Err(e) => {
            eprintln!("recview: {e}");
            return ExitCode::FAILURE;
        }
    };
    let event_loop = match EventLoop::<Vec<PathBuf>>::with_user_event().build() {
        Ok(event_loop) => event_loop,
        Err(e) => {
            eprintln!("recview: {e}");
            return ExitCode::FAILURE;
        }
    };
    #[cfg(target_os = "macos")]
    macos::handle_open_files(event_loop.create_proxy());

    let saved = Prefs::load();
    let player = Player::new(Settings {
        zoom: args.zoom.unwrap_or(saved.settings.zoom),
        ..saved.settings
    });
    let mut app = App {
        player,
        muted: saved.muted,
        saved,
        last_save: Instant::now(),
        args,
        folder: None,
        folder_chosen: false,
        waiting: None,
        sounds: None,
        gpu: None,
        current: None,
        pending: Vec::new(),
        stepped_to: None,
        modifiers: ModifiersState::empty(),
        error: None,
        last_frame: Instant::now(),
        last_title: Instant::now(),
        cursor: (0.0, 0.0),
        pressed: None,
        dragged: false,
    };
    if let Err(e) = event_loop.run_app(&mut app) {
        show_error(&e);
        return ExitCode::FAILURE;
    }
    match app.error {
        Some(e) => {
            show_error(&e);
            ExitCode::FAILURE
        }
        None => ExitCode::SUCCESS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order_sorts_numbers_as_numbers() {
        let mut names = vec![
            "lev10.lev",
            "LEV2.lev",
            "01b0ne1.rec",
            "01b0ne.rec",
            "lev1.lev",
        ];
        names.sort_by(|a, b| natural_order(a, b));
        assert_eq!(
            names,
            [
                "01b0ne.rec",
                "01b0ne1.rec",
                "lev1.lev",
                "LEV2.lev",
                "lev10.lev"
            ]
        );
        assert_eq!(natural_order("qwquu001", "qwquu002"), Ordering::Less);
    }

    #[test]
    fn neighbour_is_a_file_of_the_same_kind() {
        let dir = std::env::temp_dir().join(format!("recview-neighbour-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for name in [
            "01b0ne.rec",
            "01b0ne1.rec",
            "02.REC",
            "qwquu001.lev",
            "qwquu002.lev",
        ] {
            std::fs::write(dir.join(name), b"").unwrap();
        }
        let next = |name: &str, step| {
            neighbour(&dir.join(name), step).map(|p| p.file_name().unwrap().to_owned())
        };
        assert_eq!(next("01b0ne.rec", 1), Some("01b0ne1.rec".into()));
        assert_eq!(next("01b0ne1.rec", 1), Some("02.REC".into()));
        assert_eq!(next("02.REC", 1), Some("01b0ne.rec".into()));
        assert_eq!(next("01b0ne.rec", -1), Some("02.REC".into()));
        assert_eq!(next("qwquu002.lev", 1), Some("qwquu001.lev".into()));
        // The last file is the first after it, even when it is gone
        assert_eq!(next("03.rec", 1), Some("01b0ne.rec".into()));
        assert_eq!(next("qwquu001.lev", 1), Some("qwquu002.lev".into()));
        assert_eq!(next("qwquu002.lev", -1), Some("qwquu001.lev".into()));
        // A level alone in its folder has none, nor has a replay whose folder is empty
        std::fs::remove_file(dir.join("qwquu001.lev")).unwrap();
        assert_eq!(next("qwquu002.lev", 1), None);
        std::fs::remove_file(dir.join("qwquu002.lev")).unwrap();
        assert_eq!(next("qwquu002.lev", 1), None);
        // A file that is gone still has neighbours
        assert_eq!(next("01b0ne0.rec", 1), Some("01b0ne1.rec".into()));
        assert_eq!(next("01b0ne0.rec", -1), Some("01b0ne.rec".into()));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn title_names_the_level() {
        let title = |level, ghosts| title_name(Path::new("lev/neolb6.lev"), level, ghosts);
        assert_eq!(title("Top Heavy", 0), "neolb6.lev \"Top Heavy\"");
        assert_eq!(title("w", 2), "neolb6.lev \"w\" + 2 ghosts");
        // Internal levels have no name in the file
        assert_eq!(title("", 1), "neolb6.lev + 1 ghost");
    }
}
