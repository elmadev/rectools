use super::*;
use elma_formats::lev::{Gravity, Level, Object, Polygon, TopTens, Vec2 as LevVec2};
use elma_formats::rec::{FRAME_RATE, Frame, Rec, Ride};
use std::sync::Arc;

fn level() -> Level {
    let v = |x, y| LevVec2 { x, y };
    Level {
        level_id: 1,
        name: String::new(),
        lgr: String::new(),
        foreground: String::new(),
        background: String::new(),
        polygons: vec![Polygon {
            is_grass: false,
            vertices: vec![v(0.0, 0.0), v(40.0, 0.0), v(40.0, 10.0), v(0.0, 10.0)],
        }],
        objects: vec![Object {
            r: v(5.0, 8.0),
            kind: ObjectType::Start,
            gravity: Gravity::None,
            animation: 0,
            motion: None,
        }],
        sprites: vec![],
        topology_errors: false,
        top_tens: TopTens::default(),
    }
}

// A ride of `seconds`, standing at `x`
fn ride(seconds: f64, x: f32) -> Ride {
    let frame = Frame {
        bike_x: x,
        ..Frame::default()
    };
    Ride {
        frames: vec![frame; (seconds * FRAME_RATE) as usize + 1],
        events: vec![],
        frame_rate: FRAME_RATE,
    }
}

fn content(seconds: f64, ghosts: usize) -> Content {
    let level = level();
    let rec = Rec {
        multiplayer: false,
        flagtag: false,
        level_id: 1,
        level_filename: "test.lev".into(),
        rides: vec![ride(seconds, 5.0)],
    };
    let ghosts: Vec<Arc<Ride>> = (0..ghosts)
        .map(|i| Arc::new(ride(seconds, 30.0 + i as f32)))
        .collect();
    Content {
        session: Session::new(&level, &rec, &ghosts, replay::Settings::default()),
        level_only: false,
        bounds: camera::level_bounds(&level),
        best_time: None,
        finish_times: vec![],
        across: false,
    }
}

fn player(seconds: f64, ghosts: usize) -> Player {
    let mut player = Player::new(Settings::default());
    player.resize(800, 600, 1.0);
    player.load(content(seconds, ghosts), false);
    player
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn plays_in_real_time() {
    let mut p = player(10.0, 0);
    p.tick(1.5);
    assert!(near(p.time(), 1.5));
    p.set_speed(-1.0);
    p.tick(0.5);
    assert!(near(p.time(), 1.0));
}

#[test]
fn pauses_at_the_end_then_starts_over() {
    let mut p = player(2.0, 0);
    p.seek(1.9);
    p.tick(0.2);
    assert!(near(p.time(), p.duration()));
    // The last frame stays a moment
    p.tick(END_PAUSE / 2.0);
    assert!(near(p.time(), p.duration()));
    let tick = p.tick(END_PAUSE);
    assert!(tick.seeked);
    assert_eq!(p.time(), 0.0);
    assert!(p.is_playing());
}

#[test]
fn backwards_starts_over_from_the_end() {
    let mut p = player(2.0, 0);
    p.set_speed(-1.0);
    p.seek(0.1);
    p.tick(0.2);
    assert_eq!(p.time(), 0.0);
    p.tick(END_PAUSE + 0.01);
    assert!(near(p.time(), p.duration()));
}

#[test]
fn without_looping_it_stops_at_the_end() {
    let mut p = player(2.0, 0);
    p.set_looping(false);
    p.seek(1.9);
    let tick = p.tick(0.2);
    assert!(tick.ended);
    assert!(!p.is_playing());
    // Playing again from the end starts over
    p.toggle_play();
    assert!(p.is_playing());
    assert_eq!(p.time(), 0.0);
}

#[test]
fn seeking_leaves_the_pause_at_the_end() {
    let mut p = player(2.0, 0);
    p.seek(1.9);
    p.tick(0.2);
    p.seek(0.5);
    p.tick(0.1);
    assert!(near(p.time(), 0.6));
}

#[test]
fn a_level_on_its_own_never_plays() {
    let mut p = Player::new(Settings::default());
    p.resize(800, 600, 1.0);
    p.load(
        Content {
            level_only: true,
            ..content(0.0, 0)
        },
        false,
    );
    assert!(!p.is_playing());
    p.act(Action::TogglePlay);
    assert!(!p.is_playing());
    // It is shown whole
    assert!(p.center.is_some());
}

#[test]
fn keys_seek_by_the_speed_and_step_frames() {
    let mut p = player(10.0, 0);
    p.seek(5.0);
    p.set_speed(2.0);
    assert!(p.key("ArrowRight"));
    assert!(near(p.time(), 5.0 + 2.0 * SEEK_SECONDS));
    p.key(",");
    assert!(!p.is_playing());
    assert!(near(p.time(), 5.0 + 2.0 * SEEK_SECONDS - FRAME_SECONDS));
    assert!(!p.key("x"));
}

#[test]
fn speed_keys_keep_the_direction() {
    let mut p = player(10.0, 0);
    p.key("-");
    assert!(near(p.speed(), SPEED_STEP));
    p.key("r");
    assert!(near(p.speed(), -SPEED_STEP));
    p.key("Backspace");
    assert_eq!(p.speed(), -1.0);
}

#[test]
fn following_the_next_bike_wraps_and_recenters() {
    let mut p = player(10.0, 2);
    p.pan_by(100.0, 0.0);
    p.key("N");
    assert_eq!(p.follow(), BikeId(2));
    assert_eq!(p.pan, Vec2::default());
    p.key("n");
    assert_eq!(p.follow(), BikeId(0));
}

#[test]
fn moving_the_camera_pauses_auto_zoom_until_recentered() {
    let mut p = player(10.0, 1);
    p.set_auto_zoom(true);
    assert!(!p.auto_zoom_paused());
    p.pan_by(10.0, 0.0);
    assert!(p.auto_zoom_paused());
    p.key("c");
    assert!(!p.auto_zoom_paused());
    p.set_zoom(2.0);
    assert!(p.auto_zoom_paused());
    // Turning it on again resumes it too
    p.set_auto_zoom(false);
    p.set_auto_zoom(true);
    assert!(!p.auto_zoom_paused());
}

#[test]
fn auto_zoom_zooms_out_to_show_the_ghost() {
    let mut p = player(10.0, 1);
    p.set_zoom(4.0);
    p.recenter();
    p.set_auto_zoom(true);
    let views = p.views(1.0 / 60.0);
    assert_eq!(views.len(), 1);
    assert!(views[0].zoom < 4.0);
    // Zooming in goes from the zoom on screen
    let shown = p.shown_zoom();
    p.key("e");
    assert!(near(p.zoom(), shown * ZOOM_STEP));
}

#[test]
fn split_screen_only_while_following_a_rider_of_the_replay() {
    let mut p = Player::new(Settings::default());
    p.resize(800, 600, 1.0);
    let mut two = content(10.0, 1);
    let level = level();
    let rec = Rec {
        multiplayer: true,
        flagtag: false,
        level_id: 1,
        level_filename: "test.lev".into(),
        rides: vec![ride(10.0, 5.0), ride(10.0, 6.0)],
    };
    two.session = Session::new(
        &level,
        &rec,
        &[Arc::new(ride(10.0, 30.0))],
        replay::Settings::default(),
    );
    p.load(two, false);
    assert_eq!(p.views(0.0).len(), 2);
    p.set_follow(BikeId(2));
    assert_eq!(p.views(0.0).len(), 1);
    p.set_follow(BikeId(0));
    p.key("f");
    assert_eq!(p.views(0.0).len(), 1);
}

#[test]
fn fitting_the_level_uses_the_screen_scale() {
    let mut p = player(10.0, 0);
    p.key("f");
    let one = p.zoom();
    p.resize(1600, 1200, 2.0);
    p.key("f");
    // The same view on twice the pixels is the same zoom before the scale
    assert!(near(p.zoom(), one));
    assert!(near(p.views(0.0)[0].zoom, 2.0 * one));
}

#[test]
fn a_merge_keeps_the_time_and_the_bike_followed() {
    let mut p = player(10.0, 1);
    p.seek(4.0);
    p.set_follow(BikeId(1));
    p.load(content(10.0, 2), true);
    assert!(near(p.time(), 4.0));
    assert_eq!(p.follow(), BikeId(1));
    // Another replay starts from the beginning
    p.load(content(10.0, 0), false);
    assert_eq!(p.time(), 0.0);
    assert_eq!(p.follow(), BikeId(0));
}

#[test]
fn level_keys_do_nothing_in_across() {
    let mut p = Player::new(Settings::default());
    p.load(
        Content {
            across: true,
            ..content(1.0, 0)
        },
        false,
    );
    p.key("g");
    p.key("p");
    assert_eq!(p.settings().level, LevelOptions::default());
    p.load(content(1.0, 0), false);
    p.key("p");
    assert!(!p.settings().level.pictures && !p.settings().level.custom_sky);
    assert!(!p.scene_options().pictures && p.scene_options().default_sky);
    p.key("G");
    assert_eq!(p.settings().level, LevelOptions::default());
}
