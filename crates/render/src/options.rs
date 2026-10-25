// What the caller asks of a frame: the views and how things look in them
use crate::scene::DynamicOptions;
use replay::Vec2;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CameraMode {
    // Like the game: the bike sits 15% of the screen width from the edge at zoom 1, and that
    // distance in meters stays the same when zooming
    Classic,
    // The bike in the middle of the view
    Centered,
}

impl CameraMode {
    pub const ALL: [CameraMode; 2] = [CameraMode::Classic, CameraMode::Centered];

    pub fn as_str(self) -> &'static str {
        match self {
            CameraMode::Classic => "classic",
            CameraMode::Centered => "centered",
        }
    }

    pub fn centered(centered: bool) -> CameraMode {
        if centered {
            CameraMode::Centered
        } else {
            CameraMode::Classic
        }
    }
}

impl std::str::FromStr for CameraMode {
    type Err = String;

    fn from_str(s: &str) -> Result<CameraMode, String> {
        CameraMode::ALL
            .into_iter()
            .find(|m| m.as_str() == s)
            .ok_or_else(|| format!("unknown camera: {s}"))
    }
}

// Zoom limits for the interactive viewers
pub const MIN_ZOOM: f64 = 0.05;
pub const MAX_ZOOM: f64 = 50.0;

#[derive(Debug, Clone, Copy)]
pub struct ViewParams {
    // Index of the bike to follow: the session's riders, then its ghosts
    pub follow: usize,
    // x, y, width, height in target pixels, from the top left
    pub viewport: [u32; 4],
    pub zoom: f64,
    pub camera: CameraMode,
    // Shift of the camera from the bike, in meters
    pub pan: Vec2,
    pub timer: bool,
    // Shown left of the timer, in centiseconds
    pub best_time: Option<i64>,
    // Look at this point instead of following the bike, see camera::fit_bikes
    pub center: Option<Vec2>,
    pub minimap: bool,
    // The game's "last apple" row in the bottom right, once an apple is taken
    pub last_apple_time: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum GhostStyle {
    // Like the game: ghosts look like any other bike
    #[default]
    Normal,
    // Tinted blue and half see-through
    Translucent,
}

impl GhostStyle {
    pub fn new(translucent: bool) -> GhostStyle {
        if translucent {
            GhostStyle::Translucent
        } else {
            GhostStyle::Normal
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum TextureZoom {
    // Textures stay on the level at any zoom, for zooming smoothly. Each tile is resized like
    // the game does, where it is at zoom 1 to the nearest pixel, so this is the game's when the
    // zoom resizes the textures to whole pixels, as 1.25 or 2 does a 64 pixel texture.
    #[default]
    Stable,
    // Like the game at any zoom: textures are resized to whole pixels and tiled from the corner
    // of the level, so they move as the zoom changes, more the further from that corner
    Game,
}

#[derive(Debug, Clone, Copy)]
pub struct RenderOptions {
    pub dynamic: DynamicOptions,
    pub ghosts: GhostStyle,
    pub textures: TextureZoom,
    // Textures, the ground's, the sky's and qgrass's, are resized for the zoom, like the
    // pictures. Without, they keep their size at zoom 1, as elma-classic's "Zoom Textures"
    // turned off does.
    pub zoom_textures: bool,
    // View pixels to a game pixel for the timer's lines and the info panel's text. The game's
    // is 1, which a high density screen makes hard to see; 0 is taken as 1.
    pub hud_scale: u32,
    // Target pixels at the bottom that something lies over, such as a player's controls. The
    // last apple time and the minimap, in the bottom right, move up out of them.
    pub hud_bottom_inset: u32,
}

impl Default for RenderOptions {
    fn default() -> Self {
        RenderOptions {
            dynamic: DynamicOptions::default(),
            ghosts: GhostStyle::default(),
            textures: TextureZoom::default(),
            zoom_textures: true,
            hud_scale: 0,
            hud_bottom_inset: 0,
        }
    }
}
