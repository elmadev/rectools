// How a replay is watched, whatever shows it: playback, the camera and the settings, with the
// keys as data. recview and the web player only turn their input into calls here and draw
// what views() asks for. Where the two used to differ, this follows the web player.

pub mod keys;

pub use keys::{Action, KEYS, KeyHelp, action_for_key};

use render::camera::{self, AutoFrame};
use render::{
    CameraMode, DynamicOptions, GhostStyle, MAX_ZOOM, MIN_ZOOM, RenderOptions, SceneOptions,
    TextureZoom, ViewParams,
};
use replay::{Session, Step, Vec2};

// The last frame stays this long before the replay starts over, as in the game
pub const END_PAUSE: f64 = 0.4;
pub const SPEED_STEP: f64 = 0.8;
pub const ZOOM_STEP: f64 = 1.2;
// Seeking with the arrow keys moves this many seconds of the replay at normal speed
pub const SEEK_SECONDS: f64 = 2.5;
// What stepping one frame moves
pub const FRAME_SECONDS: f64 = 1.0 / 30.0;

// A bike of the session: its riders, then its ghosts
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct BikeId(pub usize);

// How the level is drawn
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelOptions {
    pub grass: bool,
    pub pictures: bool,
    // The level's own ground and sky, instead of the LGR's default ones
    pub custom_sky: bool,
    // Arrows on the gravity apples
    pub arrows: bool,
    // Apples, killers and the exit turn through their frames
    pub animated_objects: bool,
    // Apples and the exit stay put instead of bobbing up and down
    pub still_objects: bool,
    // The single color pictures of a fancyboosted LGR
    pub fancyboost: bool,
}

impl Default for LevelOptions {
    fn default() -> Self {
        LevelOptions {
            grass: true,
            pictures: true,
            custom_sky: true,
            arrows: true,
            animated_objects: true,
            still_objects: false,
            fancyboost: true,
        }
    }
}

// What the viewer chose, kept when another replay is opened
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    // 1 is the game's zoom, before the screen's scale
    pub zoom: f64,
    // The bike in the middle; otherwise the game's classic camera
    pub centered_camera: bool,
    // Both bikes of a two player replay
    pub split_screen: bool,
    pub timer: bool,
    pub minimap: bool,
    // The game's "last apple" row in the bottom right
    pub last_apple_time: bool,
    // Zoom out when needed so every bike stays in view
    pub auto_zoom: bool,
    // Ghosts blue and see-through
    pub translucent_ghosts: bool,
    // Bikes and objects over every picture
    pub pictures_in_background: bool,
    // The ground, sky and qgrass textures grow with the zoom, like the pictures; otherwise
    // they keep their size at zoom 1, as elma-classic's "Zoom Textures" off
    pub zoom_textures: bool,
    // The last apple time in the game's medium font, as elma-classic's "Large Font"
    pub large_font: bool,
    // Start over after the end
    pub looping: bool,
    pub level: LevelOptions,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            zoom: 1.0,
            centered_camera: true,
            split_screen: true,
            timer: true,
            minimap: false,
            last_apple_time: true,
            auto_zoom: false,
            translucent_ghosts: false,
            pictures_in_background: true,
            zoom_textures: true,
            large_font: false,
            looping: true,
            level: LevelOptions::default(),
        }
    }
}

// A replay, or a level on its own, ready to watch
pub struct Content {
    pub session: Session,
    // A level without a replay, to look around
    pub level_only: bool,
    // Of the level's ground, to show all of it
    pub bounds: Option<(Vec2, Vec2)>,
    // From the level's top ten, in centiseconds
    pub best_time: Option<i64>,
    // When each bike finishes, in seconds
    pub finish_times: Vec<Option<f64>>,
    // Drawn with Across's graphics, which have no pictures, grass or gravity arrows
    pub across: bool,
}

impl Content {
    // What a replay or level loaded from a game folder or a page plays
    pub fn new(loaded: &game_data::Loaded) -> Content {
        let session = Session::new(
            &loaded.level,
            &loaded.rec,
            &loaded.ghosts(),
            loaded.settings.clone(),
        );
        let finish_times = match loaded.level_only {
            true => Vec::new(),
            false => loaded
                .finish_times()
                .into_iter()
                .map(|t| t.map(|t| replay::centiseconds_to_seconds(replay::finish_centiseconds(t))))
                .collect(),
        };
        Content {
            session,
            level_only: loaded.level_only,
            bounds: camera::level_bounds(&loaded.level),
            best_time: loaded.best_time,
            finish_times,
            across: loaded.settings.game.is_across(),
        }
    }
}

// What a tick did
#[derive(Debug, Default)]
pub struct Tick {
    // When playing went on from where it was, the step it took, whose sounds play
    pub step: Option<Step>,
    // The time jumped since the last tick, so sounds in between aren't played
    pub seeked: bool,
    // The replay played to its end and stopped, which only happens without looping
    pub ended: bool,
}

pub struct Player {
    content: Option<Content>,
    settings: Settings,
    playing: bool,
    // Negative plays backwards
    speed: f64,
    // Seconds left of the pause at the end, 0 while not pausing
    end_pause: f64,
    seeked: bool,
    // Of the view in device pixels, and device pixels per screen pixel
    viewport: (u32, u32),
    scale: f64,
    // Shift of the camera from the bike, in meters
    pan: Vec2,
    // Look at this point instead of following the bike, after fitting the level
    center: Option<Vec2>,
    // The zoom before fitting the level, for going back onto the bike, unless zoomed since
    zoom_before_fit: Option<f64>,
    follow: BikeId,
    // Zooming or moving the camera by hand pauses auto zoom until the camera is back on
    // the bike
    taken_over: bool,
    auto_frame: AutoFrame,
}

impl Player {
    pub fn new(settings: Settings) -> Player {
        Player {
            content: None,
            settings,
            playing: true,
            speed: 1.0,
            end_pause: 0.0,
            seeked: false,
            viewport: (1, 1),
            scale: 1.0,
            pan: Vec2::default(),
            center: None,
            zoom_before_fit: None,
            follow: BikeId(0),
            taken_over: false,
            auto_frame: AutoFrame::default(),
        }
    }

    // Shows `content`. Merging replays into the one playing, or leaving some out, is `merge`:
    // it carries on from the same moment with the same view.
    pub fn load(&mut self, mut content: Content, merge: bool) {
        let previous = self
            .content
            .as_ref()
            .filter(|c| merge && !c.level_only && !content.level_only)
            .map(|c| c.session.time);
        if let Some(time) = previous {
            let time = time.min(content.session.duration());
            content.session.seek(time);
        }
        let bikes = camera::bike_count(&content.session);
        let level_only = content.level_only;
        self.content = Some(content);
        self.seeked = true;
        if previous.is_some() {
            if self.follow.0 >= bikes {
                self.follow = BikeId(0);
            }
            return;
        }
        self.end_pause = 0.0;
        self.pan = Vec2::default();
        self.center = None;
        self.follow = BikeId(0);
        self.taken_over = false;
        self.auto_frame = AutoFrame::default();
        if level_only {
            self.fit_level();
        }
    }

    pub fn unload(&mut self) {
        self.content = None;
    }

    pub fn content(&self) -> Option<&Content> {
        self.content.as_ref()
    }

    pub fn session(&self) -> Option<&Session> {
        self.content.as_ref().map(|c| &c.session)
    }

    pub fn level_only(&self) -> bool {
        self.content.as_ref().is_some_and(|c| c.level_only)
    }

    pub fn across(&self) -> bool {
        self.content.as_ref().is_some_and(|c| c.across)
    }

    // Moves time on by `dt` real seconds
    pub fn tick(&mut self, dt: f64) -> Tick {
        let mut tick = Tick::default();
        let playing = self.is_playing();
        let (speed, looping) = (self.speed, self.settings.looping);
        let Some(content) = &mut self.content else {
            return tick;
        };
        let session = &mut content.session;
        if !playing {
            self.end_pause = 0.0;
        } else if self.end_pause > 0.0 {
            // At the end before looping, like the game
            self.end_pause -= dt;
            if self.end_pause <= 0.0 {
                self.end_pause = 0.0;
                let start = if speed >= 0.0 {
                    0.0
                } else {
                    session.duration_seconds()
                };
                session.set_seconds(start);
                self.seeked = true;
            }
        } else {
            let duration = session.duration_seconds();
            let t = session.seconds() + dt * speed;
            let at_end = if speed >= 0.0 {
                t >= duration
            } else {
                t <= 0.0
            };
            tick.step = session.set_seconds(t.clamp(0.0, duration));
            if tick.step.is_none() {
                self.seeked = true;
            }
            if at_end && looping {
                self.end_pause = END_PAUSE;
            } else if at_end {
                self.playing = false;
                tick.ended = speed >= 0.0;
            }
        }
        tick.seeked = std::mem::take(&mut self.seeked);
        tick
    }

    // Playing, which a level on its own never is
    pub fn is_playing(&self) -> bool {
        self.playing && self.content.as_ref().is_some_and(|c| !c.level_only)
    }

    pub fn set_playing(&mut self, playing: bool) {
        self.playing = playing;
    }

    pub fn toggle_play(&mut self) {
        if self.content.is_none() || self.level_only() {
            return;
        }
        if !self.playing {
            // Playing again from the end starts over
            let (t, duration) = (self.time(), self.duration());
            if self.speed > 0.0 && t >= duration {
                self.seek(0.0);
            }
            if self.speed < 0.0 && t <= 0.0 {
                self.seek(duration);
            }
        }
        self.playing = !self.playing;
    }

    pub fn speed(&self) -> f64 {
        self.speed
    }

    pub fn set_speed(&mut self, speed: f64) {
        if speed.is_finite() && speed != 0.0 {
            self.speed = speed;
        }
    }

    // Current position and length in seconds
    pub fn time(&self) -> f64 {
        self.session().map_or(0.0, Session::seconds)
    }

    pub fn duration(&self) -> f64 {
        self.session().map_or(0.0, Session::duration_seconds)
    }

    // The time to show, and the replay's time: past the last frame, when it finishes
    pub fn shown_time(&self) -> f64 {
        self.session().map_or(0.0, Session::shown_seconds)
    }

    pub fn shown_duration(&self) -> f64 {
        self.session().map_or(0.0, Session::end_seconds)
    }

    // Moves to `seconds`, which also ends the pause at the end
    pub fn seek(&mut self, seconds: f64) {
        let Some(content) = &mut self.content else {
            return;
        };
        let session = &mut content.session;
        let before = session.time;
        session.set_seconds(seconds.clamp(0.0, session.duration_seconds()));
        if session.time != before {
            self.end_pause = 0.0;
            self.seeked = true;
        }
    }

    pub fn act(&mut self, action: Action) {
        let level_only = self.level_only();
        let across = self.across();
        match action {
            Action::TogglePlay => self.toggle_play(),
            Action::Slower => self.set_speed(self.speed * SPEED_STEP),
            Action::Faster => self.set_speed(self.speed / SPEED_STEP),
            Action::NormalSpeed => self.speed = self.speed.signum(),
            Action::Reverse => self.speed = -self.speed,
            Action::SeekBack | Action::SeekForward if !level_only => {
                let sign = if action == Action::SeekBack {
                    -1.0
                } else {
                    1.0
                };
                self.seek(self.time() + sign * SEEK_SECONDS * self.speed.abs());
            }
            Action::StepBack | Action::StepForward if !level_only => {
                let sign = if action == Action::StepBack {
                    -1.0
                } else {
                    1.0
                };
                self.playing = false;
                self.seek(self.time() + sign * FRAME_SECONDS);
            }
            Action::ZoomOut => self.zoom_step(-1),
            Action::ZoomIn => self.zoom_step(1),
            Action::ResetZoom => self.set_zoom(1.0),
            Action::FitLevel => self.fit_level(),
            Action::Recenter => self.recenter(),
            Action::FollowNext => self.follow_next(1),
            Action::FollowPrevious => self.follow_next(-1),
            Action::ToggleTimer => self.settings.timer = !self.settings.timer,
            Action::ToggleMinimap => self.settings.minimap = !self.settings.minimap,
            Action::ToggleLastAppleTime => {
                self.settings.last_apple_time = !self.settings.last_apple_time
            }
            // These change nothing in Across, so they leave the settings alone there
            Action::TogglePictures if !across => {
                let level = &mut self.settings.level;
                level.pictures = !level.pictures;
                level.custom_sky = level.pictures;
            }
            Action::ToggleGrass if !across => {
                self.settings.level.grass = !self.settings.level.grass;
            }
            Action::ToggleArrows if !across => {
                self.settings.level.arrows = !self.settings.level.arrows;
            }
            Action::ShowEverything if !across => {
                let level = &mut self.settings.level;
                level.grass = true;
                level.pictures = true;
                level.custom_sky = true;
            }
            _ => {}
        }
    }

    // `key` as the browser's KeyboardEvent.key names it. Returns whether it did something.
    // A key pressed, or `repeat`ed by being held down. Whether it is one of the player's.
    pub fn key(&mut self, key: &str, repeat: bool) -> bool {
        match action_for_key(key) {
            Some(action) => {
                if !repeat || action.repeats() {
                    self.act(action);
                }
                true
            }
            None => false,
        }
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    // How many device pixels a screen pixel is
    pub fn scale(&self) -> f64 {
        self.scale
    }

    // Size of the view in device pixels, and how many of them a screen pixel is
    pub fn resize(&mut self, width: u32, height: u32, scale: f64) {
        self.viewport = (width.max(1), height.max(1));
        if scale.is_finite() && scale > 0.0 {
            self.scale = scale;
        }
    }

    // The zoom set, before the screen's scale
    pub fn zoom(&self) -> f64 {
        self.settings.zoom
    }

    // Zooming leaves the camera where it is, and pauses auto zoom until it is back on the bike
    pub fn set_zoom(&mut self, zoom: f64) {
        if !zoom.is_finite() || zoom <= 0.0 {
            return;
        }
        let zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        if (zoom - self.settings.zoom).abs() > 1e-9 * self.settings.zoom {
            self.taken_over = true;
            self.zoom_before_fit = None;
        }
        self.settings.zoom = zoom;
    }

    // From the zoom on screen, which auto zoom may have made smaller than the one set
    pub fn zoom_by(&mut self, factor: f64) {
        self.set_zoom(self.shown_zoom() * factor);
    }

    // To the next power of ZOOM_STEP, so stepping always comes back to 1 however the zoom
    // was reached, by the wheel or by stopping at MIN_ZOOM
    pub fn zoom_step(&mut self, steps: i32) {
        let level = self.shown_zoom().ln() / ZOOM_STEP.ln();
        let next = if steps > 0 {
            (level + 1e-6).floor() as i32 + steps
        } else {
            (level - 1e-6).ceil() as i32 + steps
        };
        self.set_zoom(ZOOM_STEP.powi(next));
    }

    // The zoom on screen, before the screen's scale
    pub fn shown_zoom(&self) -> f64 {
        match self.auto_frame.zoom() {
            Some(zoom) if self.auto_zooming() => zoom / self.scale,
            _ => self.settings.zoom,
        }
    }

    // Drags the view by device pixels, right and down
    pub fn pan_by(&mut self, dx: f64, dy: f64) {
        let ppm = camera::pixels_per_meter(self.settings.zoom * self.scale);
        let shift = Vec2::new(-dx / ppm, dy / ppm);
        self.taken_over = true;
        match &mut self.center {
            Some(center) => *center = *center + shift,
            None => self.pan = self.pan + shift,
        }
    }

    // Shows the whole level and stops following the bike
    pub fn fit_level(&mut self) {
        let Some(bounds) = self.content.as_ref().and_then(|c| c.bounds) else {
            return;
        };
        let (w, h) = self.viewport;
        let (zoom, center) = camera::fit_bounds(bounds, [0, 0, w, h]);
        self.zoom_before_fit.get_or_insert(self.settings.zoom);
        self.settings.zoom = zoom / self.scale;
        self.center = Some(center);
    }

    // Back to following the bike, with auto zoom again, at the zoom from before fitting the
    // level
    pub fn recenter(&mut self) {
        if let Some(zoom) = self.zoom_before_fit.take() {
            self.settings.zoom = zoom;
        }
        self.pan = Vec2::default();
        self.center = None;
        self.taken_over = false;
        self.auto_frame = AutoFrame::default();
    }

    pub fn bike_count(&self) -> usize {
        self.session().map_or(0, camera::bike_count)
    }

    // The bike followed, which a merge keeps
    pub fn follow(&self) -> BikeId {
        self.follow
    }

    pub fn set_follow(&mut self, bike: BikeId) {
        self.follow = bike;
    }

    // Follows the next bike, or the previous one, and puts the camera on it
    pub fn follow_next(&mut self, direction: isize) {
        let n = self.bike_count() as isize;
        if n > 1 {
            let next = (self.follow.0 as isize + direction).rem_euclid(n);
            self.follow = BikeId(next as usize);
            self.recenter();
        }
    }

    pub fn set_centered_camera(&mut self, centered: bool) {
        self.settings.centered_camera = centered;
    }

    pub fn set_split_screen(&mut self, split: bool) {
        self.settings.split_screen = split;
    }

    pub fn set_timer(&mut self, timer: bool) {
        self.settings.timer = timer;
    }

    pub fn set_minimap(&mut self, minimap: bool) {
        self.settings.minimap = minimap;
    }

    pub fn set_last_apple_time(&mut self, shown: bool) {
        self.settings.last_apple_time = shown;
    }

    // Turning it on undoes a pause
    pub fn set_auto_zoom(&mut self, auto_zoom: bool) {
        if auto_zoom && !self.settings.auto_zoom {
            self.taken_over = false;
            self.auto_frame = AutoFrame::default();
        }
        self.settings.auto_zoom = auto_zoom;
    }

    pub fn set_translucent_ghosts(&mut self, translucent: bool) {
        self.settings.translucent_ghosts = translucent;
    }

    pub fn set_pictures_in_background(&mut self, value: bool) {
        self.settings.pictures_in_background = value;
    }

    pub fn set_zoom_textures(&mut self, value: bool) {
        self.settings.zoom_textures = value;
    }

    pub fn set_large_font(&mut self, value: bool) {
        self.settings.large_font = value;
    }

    pub fn set_looping(&mut self, looping: bool) {
        self.settings.looping = looping;
    }

    pub fn set_level_options(&mut self, options: LevelOptions) {
        self.settings.level = options;
    }

    fn auto_zooming(&self) -> bool {
        self.settings.auto_zoom && !self.taken_over && self.center.is_none()
    }

    // Auto zoom is on but paused, since the camera was zoomed or moved by hand. recenter()
    // resumes it.
    pub fn auto_zoom_paused(&self) -> bool {
        self.settings.auto_zoom && self.taken_over && !self.level_only() && self.bike_count() > 1
    }

    // The views of the next frame, `dt` real seconds after the last one: two for a split
    // screen two player replay, otherwise one
    pub fn views(&mut self, dt: f64) -> Vec<ViewParams> {
        let auto_zoom = self.settings.auto_zoom && !self.taken_over;
        let Some(content) = &self.content else {
            return Vec::new();
        };
        let (w, h) = self.viewport;
        let s = &self.settings;
        let base = ViewParams {
            follow: self.follow.0,
            viewport: [0, 0, w, h],
            zoom: s.zoom * self.scale,
            camera: CameraMode::centered(s.centered_camera),
            pan: self.pan,
            // A level on its own has no time to show, and is shown whole, which a minimap only
            // repeats
            timer: s.timer && !content.level_only,
            minimap: s.minimap && !content.level_only,
            last_apple_time: s.last_apple_time && !content.level_only,
            best_time: content.best_time,
            center: self.center,
        };
        // A fixed point to look at is one view, never zoomed out for the bikes
        let free = self.center.is_some();
        let auto = (auto_zoom && !free).then_some((&mut self.auto_frame, dt));
        // Following a merged replay shows that bike alone
        let split = s.split_screen && !free && self.follow.0 < content.session.riders.len();
        camera::layout_views(&content.session, base, (w, h), split, auto)
    }

    // How bikes and objects are drawn
    pub fn render_options(&self) -> RenderOptions {
        let s = &self.settings;
        RenderOptions {
            dynamic: DynamicOptions {
                gravity_arrows: s.level.arrows,
                animated_objects: s.level.animated_objects,
                // A level alone has no clock to bob them with, so bobbing would only put
                // them out of step
                still_objects: s.level.still_objects || self.level_only(),
                pictures_in_background: s.pictures_in_background,
            },
            ghosts: GhostStyle::new(s.translucent_ghosts),
            // Textures stay on the level as the zoom changes, by hand or with auto zoom, rather
            // than move as the game's would
            textures: TextureZoom::Stable,
            zoom_textures: s.zoom_textures,
            // The game's pixels, which the web player makes bigger on dense screens
            hud_scale: 1,
            large_font: s.large_font,
            hud_bottom_inset: 0,
        }
    }

    // What the scene is built with. A change means building it again.
    pub fn scene_options(&self) -> SceneOptions {
        let level = &self.settings.level;
        SceneOptions {
            default_ground: !level.custom_sky,
            default_sky: !level.custom_sky,
            pictures: level.pictures,
            grass: level.grass,
            fancyboost: level.fancyboost,
        }
    }

    // When bike `bike` finishes, in seconds, if it does
    pub fn finish_time(&self, bike: BikeId) -> Option<f64> {
        self.content.as_ref()?.finish_times.get(bike.0).copied()?
    }
}

#[cfg(test)]
mod tests;
