// How a session plays a replay against its level: apples, gravity, the exit, flag tag, sounds,
// and that jumping to a time gives what playing up to it does

use super::*;
use elma_formats::lev::{Object, TopTens, Vec2 as LevVec2};
use elma_formats::rec::{FLAG_FLAGTAG_A, Rec};

// One second at the replay's frame rate
const FRAMES: usize = 31;

fn at(frame: usize) -> f64 {
    frame as f64 * FRAME_INDEX_TO_TIME
}

// Played in the game's order: the apples, then the exit (ids 0, 1 and 2), then the start
fn level() -> Level {
    let object = |x, kind, gravity| Object {
        r: LevVec2 { x, y: 0.0 },
        kind,
        gravity,
        animation: 0,
        motion: None,
    };
    Level {
        level_id: 3,
        name: String::new(),
        lgr: String::new(),
        foreground: String::new(),
        background: String::new(),
        polygons: vec![],
        objects: vec![
            object(0.0, ObjectType::Start, Gravity::None),
            object(5.0, ObjectType::Food, Gravity::Up),
            object(10.0, ObjectType::Food, Gravity::None),
            object(15.0, ObjectType::Exit, Gravity::None),
        ],
        sprites: vec![],
        topology_errors: false,
        top_tens: TopTens::default(),
    }
}

const UP_APPLE: i16 = 0;
const APPLE: i16 = 1;
const EXIT: i16 = 2;

fn touch(frame: usize, object_id: i16) -> Event {
    Event {
        time: at(frame),
        object_id,
        event_id: WavEvent::None,
        volume: 0.0,
    }
}

fn sound(frame: usize, event_id: WavEvent) -> Event {
    Event {
        time: at(frame),
        object_id: -1,
        event_id,
        volume: 0.5,
    }
}

fn ride(events: Vec<Event>, flags: impl Fn(usize) -> u8) -> Ride {
    let frames = (0..FRAMES)
        .map(|i| Frame {
            bike_x: i as f32 * 0.5,
            flags: flags(i),
            ..Frame::default()
        })
        .collect();
    Ride {
        frames,
        events,
        frame_rate: FRAME_RATE,
    }
}

fn rec(rides: Vec<Ride>) -> Rec {
    Rec {
        multiplayer: rides.len() > 1,
        flagtag: false,
        level_id: 3,
        level_filename: "test.lev".into(),
        rides,
    }
}

fn session(events: Vec<Event>) -> Session {
    Session::new(
        &level(),
        &rec(vec![ride(events, |_| 0)]),
        &[],
        Settings::default(),
    )
}

// Every frame up to `frame`, like playing
fn play_to(session: &mut Session, frame: usize) -> Vec<Step> {
    (1..=frame).map(|i| session.step(at(i))).collect()
}

// Which objects are gone; the start is never shown, so it doesn't count
fn taken(session: &Session) -> Vec<bool> {
    session
        .objects
        .iter()
        .map(|o| !o.active && o.kind != ObjectType::Start)
        .collect()
}

#[test]
fn apples_go_when_taken_and_count_for_the_rider() {
    let mut s = session(vec![touch(5, APPLE), touch(10, UP_APPLE)]);
    assert_eq!(s.total_apples, 2);
    play_to(&mut s, 7);
    assert_eq!(taken(&s), [false, true, false, false]);
    assert_eq!(s.riders[0].apple_count, 1);
    play_to(&mut s, 12);
    assert_eq!(taken(&s), [true, true, false, false]);
    assert_eq!(s.riders[0].apple_count, 2);
}

#[test]
fn a_gravity_apple_turns_gravity() {
    let mut s = session(vec![touch(5, UP_APPLE)]);
    assert_eq!(s.riders[0].gravity, Gravity::Down);
    play_to(&mut s, 6);
    assert_eq!(s.riders[0].gravity, Gravity::Up);
}

#[test]
fn the_exit_finishes_only_with_every_apple() {
    let mut s = session(vec![
        touch(3, APPLE),
        touch(6, EXIT),
        touch(9, UP_APPLE),
        touch(12, EXIT),
    ]);
    let steps = play_to(&mut s, 8);
    assert_eq!(s.finish_time(), None);
    assert!(
        steps
            .iter()
            .all(|st| st.sounds.iter().all(|x| x.event != WavEvent::Win))
    );
    let steps = play_to(&mut s, 14);
    assert_eq!(s.finish_time(), Some(at(12)));
    // The finish sound plays once, on the frame the exit is reached
    let wins: Vec<usize> = steps
        .iter()
        .enumerate()
        .filter(|(_, st)| st.sounds.iter().any(|x| x.event == WavEvent::Win))
        .map(|(i, _)| i + 1)
        .collect();
    assert_eq!(wins, [12]);
}

#[test]
fn sound_events_play_on_their_frame() {
    let mut s = session(vec![sound(4, WavEvent::Bump), touch(5, APPLE)]);
    let steps = play_to(&mut s, 6);
    let sounds: Vec<(usize, WavEvent)> = steps
        .iter()
        .enumerate()
        .flat_map(|(i, st)| st.sounds.iter().map(move |x| (i + 1, x.event)))
        .collect();
    assert_eq!(sounds, [(4, WavEvent::Bump)]);
}

#[test]
fn jumping_to_a_time_is_like_playing_to_it() {
    let events = vec![
        touch(3, APPLE),
        touch(9, UP_APPLE),
        touch(12, EXIT),
        sound(14, WavEvent::Bump),
    ];
    for frame in [0, 4, 10, 13, 20, FRAMES - 1] {
        let mut played = session(events.clone());
        play_to(&mut played, frame);
        let mut jumped = session(events.clone());
        jumped.seek(at(frame));
        assert_eq!(taken(&jumped), taken(&played), "frame {frame}");
        let (j, p) = (&jumped.riders[0], &played.riders[0]);
        assert_eq!(j.apple_count, p.apple_count, "frame {frame}");
        assert_eq!(j.gravity, p.gravity, "frame {frame}");
        assert_eq!(j.finish_time, p.finish_time, "frame {frame}");
        assert_eq!(j.pose.bike_r, p.pose.bike_r, "frame {frame}");
    }
}

#[test]
fn jumping_back_puts_the_apples_back() {
    let mut s = session(vec![touch(3, APPLE), touch(9, UP_APPLE), touch(12, EXIT)]);
    play_to(&mut s, 20);
    assert!(s.finish_time().is_some());
    s.seek(at(5));
    assert_eq!(taken(&s), [false, true, false, false]);
    assert_eq!(s.riders[0].gravity, Gravity::Down);
    assert_eq!(s.finish_time(), None);
    s.seek(0.0);
    assert_eq!(taken(&s), [false; 4]);
    assert_eq!(s.riders[0].apple_count, 0);
}

#[test]
fn apples_of_both_riders_count_for_the_exit() {
    // Each rider of a two player replay takes one apple, and the second reaches the exit
    let a = ride(vec![touch(3, APPLE)], |_| 0);
    let b = ride(vec![touch(6, UP_APPLE), touch(9, EXIT)], |_| 0);
    let mut s = Session::new(&level(), &rec(vec![a, b]), &[], Settings::default());
    play_to(&mut s, 12);
    assert_eq!(s.riders[1].finish_time, Some(at(9)));
    assert_eq!(s.riders[0].finish_time, None);
}

#[test]
fn flag_tag_counts_the_time_each_side_has_the_flag() {
    // The second rider's frames say who has the flag: A for the first 10 frames
    let a = ride(vec![], |_| 0);
    let b = ride(vec![], |i| if i < 10 { FLAG_FLAGTAG_A } else { 0 });
    let mut rec = rec(vec![a, b]);
    rec.flagtag = true;
    let mut s = Session::new(&level(), &rec, &[], Settings::default());
    assert!(s.flagtag);
    play_to(&mut s, 20);
    let near = |a: f64, b: f64| (a - b).abs() < 1e-9;
    assert!(near(s.flag_time_a + s.flag_time_b, at(20)));
    // Each frame's time goes to whoever has the flag when it ends: frames 1 to 9 are A's
    assert!(near(s.flag_time_a, at(9)), "{} vs {}", s.flag_time_a, at(9));
    // Jumping starts the count over
    s.seek(at(5));
    assert_eq!((s.flag_time_a, s.flag_time_b), (0.0, 0.0));
}

#[test]
fn ghosts_take_no_apples() {
    let ghost = Arc::new(ride(vec![touch(3, APPLE)], |_| 0));
    let mut s = Session::new(
        &level(),
        &rec(vec![ride(vec![], |_| 0)]),
        &[ghost],
        Settings::default(),
    );
    play_to(&mut s, 6);
    assert_eq!(taken(&s), [false; 4]);
    assert_eq!(s.ghosts[0].apple_count, 0);
}

#[test]
fn the_session_is_done_when_every_bike_runs_out() {
    let short = Arc::new(Ride {
        frames: ride(vec![], |_| 0).frames[..5].to_vec(),
        events: vec![],
        frame_rate: FRAME_RATE,
    });
    let mut s = Session::new(
        &level(),
        &rec(vec![ride(vec![], |_| 0)]),
        &[short],
        Settings::default(),
    );
    let steps = play_to(&mut s, FRAMES + 1);
    let first_done = steps.iter().position(|st| st.done).map(|i| i + 1);
    // The ghost ran out long before, so it's the main bike's end that counts
    assert!(
        first_done.is_some_and(|f| f >= FRAMES - 1),
        "{first_done:?}"
    );
}
