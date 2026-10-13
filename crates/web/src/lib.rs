#![cfg(target_arch = "wasm32")]

use elma_formats::across::rec::{AcrossRec, is_across_rec};
use elma_formats::lev::{Level, internal_level_file};
use elma_formats::lgr::Lgr;
use elma_formats::pic::{Pic8, decode_shirt, shirt_from_rgba};
use elma_formats::rec::Rec;
use elma_formats::res::Res;
use game_data::across::Files;
use player::{BikeId, Content, Player, Settings};
use render::wgpu;
use render::{GpuScene, Output, Renderer, Scene, SceneOptions, Targets};
use std::path::PathBuf;
use std::sync::Arc;
use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;

fn error(e: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&e.to_string())
}

// An Error whose `kind` says what couldn't be used, so the player can tell a broken replay
// from a missing LGR: "rec", "ghost" (a merged replay), "lev", "lgr", "res" (elma.res),
// "across-pack", "level-changed", "topology" or "graphics"
fn failure(kind: &str, e: impl std::fmt::Display) -> JsValue {
    let error = js_sys::Error::new(&e.to_string());
    let _ = js_sys::Reflect::set(&error, &"kind".into(), &kind.into());
    error.into()
}

fn load_failure(e: game_data::Error) -> JsValue {
    use game_data::{Error, FileKind};
    let kind = match &e {
        Error::LevelChanged { .. } => "level-changed",
        Error::NotSameLevel { .. } => "ghost",
        Error::TopologyErrors { .. } => "topology",
        _ => match e.file_kind() {
            Some(FileKind::Rec) => "rec",
            Some(FileKind::Lev) => "lev",
            Some(FileKind::Lgr) => "lgr",
            Some(FileKind::ElmaRes) => "res",
            Some(FileKind::AcrossRes) => "across-pack",
            None => "other",
        },
    };
    failure(kind, e)
}

// What a replay needs before it can be loaded
#[wasm_bindgen]
pub struct RecInfo {
    level_filename: String,
    #[wasm_bindgen(readonly, js_name = levelId)]
    pub level_id: i32,
    #[wasm_bindgen(readonly)]
    pub multiplayer: bool,
    // Is the level packed in elma.res, or one of Across's own
    #[wasm_bindgen(readonly)]
    pub internal: bool,
    // An Across replay, which is drawn from an Across pack instead of an LGR
    #[wasm_bindgen(readonly)]
    pub across: bool,
}

#[wasm_bindgen]
impl RecInfo {
    #[wasm_bindgen(getter, js_name = levelFilename)]
    pub fn level_filename(&self) -> String {
        self.level_filename.clone()
    }
}

#[wasm_bindgen(js_name = recInfo)]
pub fn rec_info(rec: &[u8]) -> Result<RecInfo, JsValue> {
    if is_across_rec(rec) {
        let across = AcrossRec::parse(rec).map_err(|e| failure("rec", e))?;
        return Ok(RecInfo {
            internal: across.level.is_some(),
            level_filename: across.rec.level_filename,
            level_id: across.rec.level_id,
            multiplayer: false,
            across: true,
        });
    }
    let rec = Rec::parse(rec).map_err(|e| failure("rec", e))?;
    Ok(RecInfo {
        internal: internal_level_file(&rec.level_filename).is_some(),
        level_filename: rec.level_filename,
        level_id: rec.level_id,
        multiplayer: rec.multiplayer,
        across: false,
    })
}

// Whether an Across replay of a level from a file was recorded on `lev`: the bike starts on
// its start
#[wasm_bindgen(js_name = acrossRecOnLevel)]
pub fn across_rec_on_level(rec: &[u8], lev: &[u8]) -> bool {
    let Ok(rec) = AcrossRec::parse(rec) else {
        return false;
    };
    let Ok(level) = elma_formats::across::level_from_lev(lev) else {
        return false;
    };
    !game_data::across::match_level(&rec, &[(PathBuf::new(), level)]).is_empty()
}

// The LGR a level asks for, without the .lgr extension. Internal levels use default.lgr.
#[wasm_bindgen(js_name = levelLgr)]
pub fn level_lgr(lev: &[u8]) -> Result<String, JsValue> {
    Ok(Level::parse(lev, false).map_err(|e| failure("lev", e))?.lgr)
}

#[wasm_bindgen]
#[derive(Default, Clone, Copy)]
pub struct AppleInfo {
    #[wasm_bindgen(readonly)]
    pub taken: i32,
    #[wasm_bindgen(readonly)]
    pub total: i32,
    #[wasm_bindgen(readonly, js_name = lastTime)]
    pub last_time: Option<f64>,
}

// Everything a replay is drawn from. Ghosts and shirts are added before `Viewer::load`. The
// ghosts are other replays of the level merged in, drawn in the order they were added.
#[wasm_bindgen]
#[derive(Default)]
pub struct Replay {
    lgr: Vec<u8>,
    level: Vec<u8>,
    res: Vec<u8>,
    across_pack: Vec<u8>,
    rec: Vec<u8>,
    ghosts: Vec<Vec<u8>>,
    shirts: Vec<Option<Shirt>>,
}

// A shirt as a bmp, or as the colors of a picture the browser decoded, like a site's png
enum Shirt {
    Bmp(Vec<u8>),
    Rgba {
        width: usize,
        height: usize,
        pixels: Vec<u8>,
    },
}

impl Shirt {
    fn decode(&self, palette: &[u8; 768]) -> Option<Pic8> {
        match self {
            Shirt::Bmp(data) => decode_shirt(data),
            Shirt::Rgba {
                width,
                height,
                pixels,
            } => shirt_from_rgba(*width, *height, pixels, palette),
        }
    }
}

#[wasm_bindgen]
impl Replay {
    #[wasm_bindgen(constructor)]
    pub fn new(rec: &[u8], lgr: &[u8]) -> Replay {
        Replay {
            rec: rec.to_vec(),
            lgr: lgr.to_vec(),
            shirts: vec![None],
            ..Default::default()
        }
    }

    // A level to look around without a replay
    pub fn level(lev: &[u8], lgr: &[u8]) -> Replay {
        Replay {
            level: lev.to_vec(),
            lgr: lgr.to_vec(),
            shirts: vec![None],
            ..Default::default()
        }
    }

    // An external level (.lev)
    #[wasm_bindgen(js_name = setLevel)]
    pub fn set_level(&mut self, lev: &[u8]) {
        self.level = lev.to_vec();
    }

    // elma.res, which holds the internal levels
    #[wasm_bindgen(js_name = setRes)]
    pub fn set_res(&mut self, res: &[u8]) {
        self.res = res.to_vec();
    }

    // The Across graphics and built-in levels (an across.res), for Across replays and
    // levels. With it and no LGR, a level on its own is an Across one.
    #[wasm_bindgen(js_name = setAcrossPack)]
    pub fn set_across_pack(&mut self, pack: &[u8]) {
        self.across_pack = pack.to_vec();
    }

    fn is_across(&self) -> bool {
        if self.rec.is_empty() {
            self.lgr.is_empty() && !self.across_pack.is_empty()
        } else {
            is_across_rec(&self.rec)
        }
    }

    // Shirt bitmap (bmp/<nick>.bmp) of the main replay
    #[wasm_bindgen(js_name = setShirt)]
    pub fn set_shirt(&mut self, bmp: &[u8]) {
        self.shirts[0] = Some(Shirt::Bmp(bmp.to_vec()));
    }

    #[wasm_bindgen(js_name = addGhost)]
    pub fn add_ghost(&mut self, rec: &[u8], shirt: Option<Vec<u8>>) {
        self.ghosts.push(rec.to_vec());
        self.shirts.push(shirt.map(Shirt::Bmp));
    }

    // The shirt of replay `index` (0 the main one, then the ghosts in order) from the RGBA
    // pixels of a picture, upright like sites show them or on its side like a bmp
    #[wasm_bindgen(js_name = setShirtPixels)]
    pub fn set_shirt_pixels(&mut self, index: usize, width: usize, height: usize, rgba: &[u8]) {
        if let Some(shirt) = self.shirts.get_mut(index) {
            *shirt = Some(Shirt::Rgba {
                width,
                height,
                pixels: rgba.to_vec(),
            });
        }
    }
}

// What is shown, besides the session the player keeps
struct Loaded {
    scene: GpuScene,
    // What the scene was built with, so a change builds it again
    built_with: SceneOptions,
    data: game_data::Loaded,
    // The main replay's file, to tell a merge into it from another replay
    rec: Vec<u8>,
}

// What a key or a button of the page does, see player::Action
#[wasm_bindgen]
#[derive(Clone, Copy)]
pub enum Action {
    TogglePlay,
    Slower,
    Faster,
    NormalSpeed,
    Reverse,
    SeekBack,
    SeekForward,
    StepBack,
    StepForward,
    ZoomOut,
    ZoomIn,
    FitLevel,
    Recenter,
    FollowNext,
    FollowPrevious,
    TogglePictures,
    ToggleGrass,
    ToggleArrows,
    ToggleTimer,
    ToggleMinimap,
    ShowEverything,
}

impl From<Action> for player::Action {
    fn from(action: Action) -> player::Action {
        use player::Action as A;
        match action {
            Action::TogglePlay => A::TogglePlay,
            Action::Slower => A::Slower,
            Action::Faster => A::Faster,
            Action::NormalSpeed => A::NormalSpeed,
            Action::Reverse => A::Reverse,
            Action::SeekBack => A::SeekBack,
            Action::SeekForward => A::SeekForward,
            Action::StepBack => A::StepBack,
            Action::StepForward => A::StepForward,
            Action::ZoomOut => A::ZoomOut,
            Action::ZoomIn => A::ZoomIn,
            Action::FitLevel => A::FitLevel,
            Action::Recenter => A::Recenter,
            Action::FollowNext => A::FollowNext,
            Action::FollowPrevious => A::FollowPrevious,
            Action::TogglePictures => A::TogglePictures,
            Action::ToggleGrass => A::ToggleGrass,
            Action::ToggleArrows => A::ToggleArrows,
            Action::ToggleTimer => A::ToggleTimer,
            Action::ToggleMinimap => A::ToggleMinimap,
            Action::ShowEverything => A::ShowEverything,
        }
    }
}

// A line of the list of keys, see player::KEYS
#[wasm_bindgen]
pub struct KeyHelp {
    keys: &'static str,
    description: &'static str,
    // Does something for a level without a replay
    #[wasm_bindgen(readonly)]
    pub level: bool,
    // Only for Elma: Across has no pictures, grass or gravity arrows
    #[wasm_bindgen(readonly, js_name = elmaOnly)]
    pub elma_only: bool,
}

#[wasm_bindgen]
impl KeyHelp {
    #[wasm_bindgen(getter)]
    pub fn keys(&self) -> String {
        self.keys.into()
    }

    #[wasm_bindgen(getter)]
    pub fn description(&self) -> String {
        self.description.into()
    }
}

#[wasm_bindgen(js_name = keyHelp)]
pub fn key_help() -> Vec<KeyHelp> {
    player::KEYS
        .iter()
        .map(|k| KeyHelp {
            keys: k.keys,
            description: k.description,
            level: k.level,
            elma_only: k.elma_only,
        })
        .collect()
}

// Everything the page shows of the player at a moment, read in one call
#[wasm_bindgen]
#[derive(Clone, Copy, PartialEq)]
pub struct ViewerState {
    #[wasm_bindgen(js_name = hasContent)]
    pub has_content: bool,
    #[wasm_bindgen(js_name = levelOnly)]
    pub level_only: bool,
    pub across: bool,
    pub duration: f64,
    #[wasm_bindgen(js_name = bikeCount)]
    pub bike_count: usize,
    pub playing: bool,
    pub speed: f64,
    // Before the screen's scale
    pub zoom: f64,
    pub follow: usize,
    #[wasm_bindgen(js_name = centeredCamera)]
    pub centered_camera: bool,
    pub timer: bool,
    pub minimap: bool,
    #[wasm_bindgen(js_name = autoZoom)]
    pub auto_zoom: bool,
    #[wasm_bindgen(js_name = autoZoomPaused)]
    pub auto_zoom_paused: bool,
    #[wasm_bindgen(js_name = translucentGhosts)]
    pub translucent_ghosts: bool,
    #[wasm_bindgen(js_name = picturesInBackground)]
    pub pictures_in_background: bool,
    pub grass: bool,
    pub pictures: bool,
    #[wasm_bindgen(js_name = customSky)]
    pub custom_sky: bool,
    pub arrows: bool,
    #[wasm_bindgen(js_name = animatedObjects)]
    pub animated_objects: bool,
    #[wasm_bindgen(js_name = stillObjects)]
    pub still_objects: bool,
    pub fancyboost: bool,
}

// A replay on a canvas: the GPU, the scene and the player core, which keeps the playback,
// the camera and the settings
#[wasm_bindgen]
pub struct Viewer {
    renderer: Renderer,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    targets: Targets,
    player: Player,
    loaded: Option<Loaded>,
    last_render: Option<f64>,
    // The last Across pack and its graphics
    across: Option<(Vec<u8>, Arc<elma_formats::across::gfx::AcrossGraphics>)>,
}

#[wasm_bindgen]
impl Viewer {
    // Opens WebGPU, or WebGL2 where WebGPU isn't available
    pub async fn create(canvas: HtmlCanvasElement) -> Result<Viewer, JsValue> {
        console_error_panic_hook::set_once();
        let (width, height) = (canvas.width().max(1), canvas.height().max(1));
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::GL;
        let instance = wgpu::util::new_instance_with_webgpu_detection(descriptor).await;
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
            .map_err(|e| failure("graphics", e))?;
        let (renderer, adapter) = Renderer::request(&instance, Some(&surface), "rectools")
            .await
            .map_err(|e| failure("graphics", e))?;
        let caps = surface.get_capabilities(&adapter);
        let (Some(format), Some(&alpha_mode)) =
            (render::surface_format(&caps), caps.alpha_modes.first())
        else {
            return Err(failure("graphics", "the canvas can't be drawn to"));
        };
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode,
            view_formats: vec![],
            ..surface
                .get_default_config(&adapter, width, height)
                .ok_or("unsupported surface")?
        };
        surface.configure(renderer.device(), &config);
        let targets = renderer.create_targets(width, height);
        let mut player = Player::new(Settings::default());
        player.resize(width, height, 1.0);
        Ok(Viewer {
            renderer,
            surface,
            config,
            targets,
            player,
            loaded: None,
            last_render: None,
            across: None,
        })
    }

    // The graphics of `pack`, read once
    fn across_graphics(
        &mut self,
        pack: &[u8],
    ) -> Result<Arc<elma_formats::across::gfx::AcrossGraphics>, JsValue> {
        if let Some((last, graphics)) = &self.across
            && last.as_slice() == pack
        {
            return Ok(graphics.clone());
        }
        let files = Files::res(pack.to_vec()).map_err(load_failure)?;
        let graphics = Arc::new(files.graphics().map_err(load_failure)?);
        self.across = Some((pack.to_vec(), graphics.clone()));
        Ok(graphics)
    }

    fn load_across(&mut self, replay: &Replay) -> Result<game_data::Loaded, JsValue> {
        if replay.across_pack.is_empty() {
            return Err(failure(
                "across-pack",
                "Across replays need the Across pack",
            ));
        }
        let graphics = self.across_graphics(&replay.across_pack)?;
        let files = Files::res(replay.across_pack.clone()).map_err(load_failure)?;
        let kinds = graphics.kinds();
        let path = PathBuf::from("the replay");
        let external = |data: &[u8]| {
            game_data::across::parse_level("the level", data, &kinds).map_err(load_failure)
        };
        if replay.rec.is_empty() {
            let level = external(&replay.level)?;
            let descriptor = game_data::across::EXTERNAL;
            return Ok(game_data::across::assemble_level(
                path, level, descriptor, graphics,
            ));
        }
        let rec = AcrossRec::parse(&replay.rec).map_err(|e| failure("rec", e))?;
        let (level, descriptor) = match rec.level {
            Some(number) => {
                game_data::across::builtin_level(&files, number, &kinds).map_err(load_failure)?
            }
            None => (external(&replay.level)?, game_data::across::EXTERNAL),
        };
        let mut ghosts = Vec::new();
        for (i, data) in replay.ghosts.iter().enumerate() {
            let name = format!("replay {}", i + 2);
            let ghost =
                AcrossRec::parse(data).map_err(|e| failure("ghost", format!("{name}: {e}")))?;
            ghosts.push((name, ghost));
        }
        game_data::across::assemble(path, rec, level, descriptor, graphics, ghosts)
            .map_err(load_failure)
    }

    pub fn load(&mut self, replay: &Replay) -> Result<(), JsValue> {
        let data = if replay.is_across() {
            self.load_across(replay)
        } else {
            Self::load_elma(replay)
        };
        match data.and_then(|data| self.show(replay, data)) {
            Ok(()) => Ok(()),
            Err(e) => {
                // The replay playing goes on when merging others into it fails. Otherwise
                // it isn't left on screen as if it were the one that didn't load.
                let merging = !replay.rec.is_empty()
                    && self.loaded.as_ref().is_some_and(|l| l.rec == replay.rec);
                if !merging {
                    self.unload();
                }
                Err(e)
            }
        }
    }

    // Whether there is a replay or level to show
    #[wasm_bindgen(js_name = hasContent)]
    pub fn has_content(&self) -> bool {
        self.loaded.is_some()
    }

    // Shows nothing
    pub fn unload(&mut self) {
        self.loaded = None;
        self.player.unload();
        if let Some(frame) = render::acquire(&self.surface, self.renderer.device(), &self.config) {
            let view = frame.texture.create_view(&Default::default());
            self.renderer.clear(Output {
                view: &view,
                format: self.config.format,
            });
            self.renderer.queue().present(frame);
        }
    }

    fn load_elma(replay: &Replay) -> Result<game_data::Loaded, JsValue> {
        let lgr = Lgr::parse(&replay.lgr).map_err(|e| failure("lgr", e))?;
        Ok(if replay.rec.is_empty() {
            let level = Level::parse(&replay.level, false).map_err(|e| failure("lev", e))?;
            game_data::assemble_level(PathBuf::from("the level"), level, lgr)
                .map_err(load_failure)?
        } else {
            let rec = Rec::parse(&replay.rec).map_err(|e| failure("rec", e))?;
            let level = match internal_level_file(&rec.level_filename) {
                Some(name) if replay.level.is_empty() => {
                    let res = Res::parse(&replay.res)
                        .map_err(|e| failure("res", format!("elma.res: {e}")))?;
                    game_data::internal_level(&res, name).map_err(load_failure)?
                }
                _ => Level::parse(&replay.level, false).map_err(|e| failure("lev", e))?,
            };
            let palette = lgr.palette;
            let shirt = |i: usize| replay.shirts[i].as_ref()?.decode(&palette);
            let mut ghosts = Vec::new();
            for (i, data) in replay.ghosts.iter().enumerate() {
                let ghost = Rec::parse(data)
                    .map_err(|e| failure("ghost", format!("replay {}: {e}", i + 2)))?;
                ghosts.push((format!("replay {}", i + 2), ghost, shirt(i + 1)));
            }
            game_data::assemble(
                PathBuf::from("the replay"),
                rec,
                level,
                lgr,
                shirt(0),
                ghosts,
            )
            .map_err(load_failure)?
        })
    }

    fn show(&mut self, replay: &Replay, data: game_data::Loaded) -> Result<(), JsValue> {
        let built_with = self.player.scene_options();
        let scene = self.build_scene(&data, &built_with)?;
        // Merging replays into the one playing, or leaving them out, carries on from the same
        // moment with the same view
        let merge = self.loaded.as_ref().is_some_and(|l| l.rec == replay.rec);
        self.player.load(Content::new(&data), merge);
        self.loaded = Some(Loaded {
            scene,
            built_with,
            data,
            rec: replay.rec.clone(),
        });
        Ok(())
    }

    fn build_scene(
        &self,
        data: &game_data::Loaded,
        options: &SceneOptions,
    ) -> Result<GpuScene, JsValue> {
        let scene = Scene::new(
            &data.look,
            &data.level,
            &data.shirts(),
            options,
            self.renderer.atlas_limit(),
        );
        Ok(self
            .renderer
            .upload_scene(scene.map_err(|e| failure("graphics", e))?))
    }

    // Drawn with Across's graphics, which have no LGR, grass, pictures or gravity apples
    #[wasm_bindgen(js_name = isAcross)]
    pub fn is_across(&self) -> bool {
        self.player.across()
    }

    // A level without a replay, which doesn't play
    #[wasm_bindgen(js_name = levelOnly)]
    pub fn level_only(&self) -> bool {
        self.player.level_only()
    }

    // The bikes there are to follow: the replay's riders, then the ghosts'
    #[wasm_bindgen(js_name = bikeCount)]
    pub fn bike_count(&self) -> usize {
        self.player.bike_count()
    }

    pub fn state(&self) -> ViewerState {
        let p = &self.player;
        let s = p.settings();
        ViewerState {
            has_content: self.loaded.is_some(),
            level_only: p.level_only(),
            across: p.across(),
            duration: p.shown_duration(),
            bike_count: p.bike_count(),
            playing: p.is_playing(),
            speed: p.speed(),
            zoom: p.zoom(),
            follow: p.follow().0,
            centered_camera: s.centered_camera,
            timer: s.timer,
            minimap: s.minimap,
            auto_zoom: s.auto_zoom,
            auto_zoom_paused: p.auto_zoom_paused(),
            translucent_ghosts: s.translucent_ghosts,
            pictures_in_background: s.pictures_in_background,
            grass: s.level.grass,
            pictures: s.level.pictures,
            custom_sky: s.level.custom_sky,
            arrows: s.level.arrows,
            animated_objects: s.level.animated_objects,
            still_objects: s.level.still_objects,
            fancyboost: s.level.fancyboost,
        }
    }

    // Moves time on by `dt` real seconds. Returns whether the replay played to its end and
    // stopped, which only happens without looping.
    pub fn tick(&mut self, dt: f64) -> bool {
        self.player.tick(dt).ended
    }

    // `key` as KeyboardEvent.key names it. Returns whether it is a shortcut.
    pub fn key(&mut self, key: &str, repeat: bool) -> bool {
        self.player.key(key, repeat)
    }

    pub fn act(&mut self, action: Action) {
        self.player.act(action.into());
    }

    pub fn playing(&self) -> bool {
        self.player.is_playing()
    }

    #[wasm_bindgen(js_name = setPlaying)]
    pub fn set_playing(&mut self, playing: bool) {
        self.player.set_playing(playing);
    }

    // Negative plays backwards
    pub fn speed(&self) -> f64 {
        self.player.speed()
    }

    #[wasm_bindgen(js_name = setSpeed)]
    pub fn set_speed(&mut self, speed: f64) {
        self.player.set_speed(speed);
    }

    #[wasm_bindgen(js_name = setLooping)]
    pub fn set_looping(&mut self, looping: bool) {
        self.player.set_looping(looping);
    }

    // `{ grass, pictures, customSky, arrows, animatedObjects, stillObjects, fancyboost }`; a
    // field left out keeps its value. Only grass, pictures, the sky and fancyboost build the
    // scene again, when it is next drawn.
    #[wasm_bindgen(js_name = setLevelOptions)]
    pub fn set_level_options(&mut self, options: &JsValue) {
        let mut o = self.player.settings().level;
        let fields: [(&str, &mut bool); 7] = [
            ("grass", &mut o.grass),
            ("pictures", &mut o.pictures),
            ("customSky", &mut o.custom_sky),
            ("arrows", &mut o.arrows),
            ("animatedObjects", &mut o.animated_objects),
            ("stillObjects", &mut o.still_objects),
            ("fancyboost", &mut o.fancyboost),
        ];
        for (name, value) in fields {
            if let Some(v) = js_sys::Reflect::get(options, &name.into())
                .ok()
                .and_then(|v| v.as_bool())
            {
                *value = v;
            }
        }
        self.player.set_level_options(o);
    }

    // Apples the followed bike has taken by now, and when it took the last one in seconds
    #[wasm_bindgen(js_name = appleInfo)]
    pub fn apple_info(&self) -> AppleInfo {
        let a = self.player.apples();
        AppleInfo {
            taken: a.taken,
            total: a.total,
            last_time: a.last_time,
        }
    }

    // The replay's time in seconds, which can be a moment past its last frame when it finishes
    pub fn duration(&self) -> f64 {
        self.player.shown_duration()
    }

    // Current position in seconds, and at the end the replay's time
    pub fn time(&self) -> f64 {
        self.player.shown_time()
    }

    // Moves to `seconds`
    pub fn seek(&mut self, seconds: f64) {
        self.player.seek(seconds);
    }

    // The zoom set, 1 being the game's, before the screen's scale
    pub fn zoom(&self) -> f64 {
        self.player.zoom()
    }

    // Zooming by hand pauses auto zoom until the camera is back on the bike
    #[wasm_bindgen(js_name = setZoom)]
    pub fn set_zoom(&mut self, zoom: f64) {
        self.player.set_zoom(zoom);
    }

    // From the zoom on screen
    #[wasm_bindgen(js_name = zoomBy)]
    pub fn zoom_by(&mut self, factor: f64) {
        self.player.zoom_by(factor);
    }

    // The zoom on screen, which auto zoom can make smaller than the one set
    #[wasm_bindgen(js_name = shownZoom)]
    pub fn shown_zoom(&self) -> f64 {
        self.player.shown_zoom()
    }

    // The bike in the middle, or the game's classic camera
    #[wasm_bindgen(js_name = setCenteredCamera)]
    pub fn set_centered_camera(&mut self, centered: bool) {
        self.player.set_centered_camera(centered);
    }

    // Drags the view by device pixels, right and down
    #[wasm_bindgen(js_name = panBy)]
    pub fn pan_by(&mut self, dx: f64, dy: f64) {
        self.player.pan_by(dx, dy);
    }

    // Shows the whole level and stops following the bike
    #[wasm_bindgen(js_name = fitLevel)]
    pub fn fit_level(&mut self) {
        self.player.fit_level();
    }

    // Back to following the bike, with auto zoom again
    pub fn recenter(&mut self) {
        self.player.recenter();
    }

    // When bike `bike` (see `bikeCount`) finishes, in seconds, if it does
    #[wasm_bindgen(js_name = finishTime)]
    pub fn finish_time(&self, bike: usize) -> Option<f64> {
        self.player.finish_time(BikeId(bike))
    }

    // The bike followed, which a merge keeps
    pub fn follow(&self) -> usize {
        self.player.follow().0
    }

    // Which bike to follow, see `bikeCount`. Split screen always shows both riders of a two
    // player replay.
    #[wasm_bindgen(js_name = setFollow)]
    pub fn set_follow(&mut self, bike: usize) {
        self.player.set_follow(BikeId(bike));
    }

    #[wasm_bindgen(js_name = setSplitScreen)]
    pub fn set_split_screen(&mut self, split: bool) {
        self.player.set_split_screen(split);
    }

    #[wasm_bindgen(js_name = setTimer)]
    pub fn set_timer(&mut self, timer: bool) {
        self.player.set_timer(timer);
    }

    #[wasm_bindgen(js_name = setMinimap)]
    pub fn set_minimap(&mut self, minimap: bool) {
        self.player.set_minimap(minimap);
    }

    // WebGL2 can't, and draws merged bikes like any bike
    #[wasm_bindgen(js_name = canDrawTranslucentGhosts)]
    pub fn can_draw_translucent_ghosts(&self) -> bool {
        self.renderer.translucent_ghosts()
    }

    // Ghosts tinted blue and see-through instead of like any other bike. Ignored where the
    // device can't draw them, see `canDrawTranslucentGhosts`.
    #[wasm_bindgen(js_name = setTranslucentGhosts)]
    pub fn set_translucent_ghosts(&mut self, value: bool) {
        self.player.set_translucent_ghosts(value);
    }

    // Zoom out when needed so every ghost stays in view. Turning it on undoes a pause.
    #[wasm_bindgen(js_name = setAutoZoom)]
    pub fn set_auto_zoom(&mut self, value: bool) {
        self.player.set_auto_zoom(value);
    }

    // Auto zoom is on but paused, since the camera was zoomed or moved by hand. recenter()
    // resumes it.
    #[wasm_bindgen(js_name = autoZoomPaused)]
    pub fn auto_zoom_paused(&self) -> bool {
        self.player.auto_zoom_paused()
    }

    #[wasm_bindgen(js_name = setPicturesInBackground)]
    pub fn set_pictures_in_background(&mut self, value: bool) {
        self.player.set_pictures_in_background(value);
    }

    // Size of the canvas in device pixels, and how many of them a CSS pixel is
    pub fn resize(&mut self, width: u32, height: u32, scale: f64) {
        let (width, height) = (width.max(1), height.max(1));
        self.player.resize(width, height, scale);
        if (width, height) == (self.config.width, self.config.height) {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(self.renderer.device(), &self.config);
        self.targets = self.renderer.create_targets(width, height);
    }

    pub fn render(&mut self) {
        // The level options changed what the scene is built with
        let options = self.player.scene_options();
        if let Some(loaded) = &self.loaded
            && loaded.built_with != options
        {
            match self.build_scene(&loaded.data, &options) {
                Ok(scene) => {
                    if let Some(loaded) = &mut self.loaded {
                        loaded.scene = scene;
                        loaded.built_with = options;
                    }
                }
                // A scene that can't be built leaves the one shown
                Err(e) => {
                    web_sys::console::error_1(&e);
                    if let Some(loaded) = &mut self.loaded {
                        loaded.built_with = options;
                    }
                }
            }
        }
        if self.loaded.is_none() || self.player.session().is_none() {
            return;
        }
        let Some(frame) = render::acquire(&self.surface, self.renderer.device(), &self.config)
        else {
            return;
        };
        let view = frame.texture.create_view(&Default::default());
        let now = js_sys::Date::now() / 1000.0;
        let dt = self
            .last_render
            .map(|t| (now - t).clamp(0.0, 0.25))
            .unwrap_or(0.0);
        self.last_render = Some(now);
        let views = self.player.views(dt);
        let options = self.player.render_options();
        let (Some(loaded), Some(session)) = (&self.loaded, self.player.session()) else {
            return;
        };
        let rendered = self.renderer.render(
            &loaded.scene,
            &self.targets,
            session,
            &views,
            &options,
            Output {
                view: &view,
                format: self.config.format,
            },
        );
        if let Err(e) = rendered {
            web_sys::console::error_1(&error(e));
        }
        self.renderer.queue().present(frame);
    }
}
