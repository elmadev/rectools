mod vec2;

pub use vec2::{Vec2, circles_intersection};

use elma_formats::lev::{Gravity, Level, Motion, ObjectType};
use elma_formats::rec::{
    Event, FLAG_FLAGTAG_A, FLAG_FLAGTAG_IMMUNITY, FLAG_FLIPPED, FLAG_GAS, Frame, Rec, Ride,
    WavEvent,
};
use std::sync::Arc;

const STOPWATCH_MULTIPLIER: f64 = 0.182;
const STOPWATCH_TO_PHYS_TIME: f64 = 0.0024;

// Game time ("physics time") passed per real millisecond
pub const MILLISECONDS_TO_PHYS_TIME: f64 = STOPWATCH_MULTIPLIER * STOPWATCH_TO_PHYS_TIME;
pub const TIME_TO_CENTISECONDS: f64 =
    100.0 / (STOPWATCH_MULTIPLIER * 1000.0 * STOPWATCH_TO_PHYS_TIME);
pub const FRAME_RATE: f64 = 30.0;
pub const TIME_TO_FRAME_INDEX: f64 =
    FRAME_RATE / (STOPWATCH_MULTIPLIER * 1000.0 * STOPWATCH_TO_PHYS_TIME);
pub const FRAME_INDEX_TO_TIME: f64 = 1.0 / TIME_TO_FRAME_INDEX;

// Frames of a ride passed per unit of game time. Scaled from Elma's so Elma replays are
// timed exactly as before.
pub fn frames_per_time(ride: &Ride) -> f64 {
    TIME_TO_FRAME_INDEX * (ride.frame_rate / FRAME_RATE)
}

pub fn time_per_frame(ride: &Ride) -> f64 {
    FRAME_INDEX_TO_TIME * (FRAME_RATE / ride.frame_rate)
}
pub const VOLT_DELAY: f64 = 0.4;
pub const DEFAULT_TURN_TIME: f64 = 0.35;

const POSITION_RATIO: f64 = 1000.0;
const WHEEL_ROTATION_RANGE: i32 = 250;
const WHEEL_ROTATION_RATIO: f64 = WHEEL_ROTATION_RANGE as f64 / (2.0 * std::f64::consts::PI);
const BIKE_ROTATION_RANGE: i32 = 10000;
const BIKE_ROTATION_RATIO: f64 = BIKE_ROTATION_RANGE as f64 / (2.0 * std::f64::consts::PI);
const MOTOR_FREQUENCY_RATIO: f64 = 250.0;
const FRICTION_VOLUME_RATIO: f64 = 250.0 / 2.0;

// Convert real seconds to game time
pub fn seconds_to_time(seconds: f64) -> f64 {
    seconds * 1000.0 * MILLISECONDS_TO_PHYS_TIME
}

// A finish time in hundredths as elma.online shows it: elmajs's getTime rounds the game
// time down to milliseconds, then hundredths
pub fn finish_centiseconds(time: f64) -> i64 {
    let ms = (time * (0.001 / (STOPWATCH_MULTIPLIER * STOPWATCH_TO_PHYS_TIME)) * 1000.0).floor();
    (ms / 10.0).floor() as i64
}

// An unfinished ride's time in hundredths, from its frames, rounded like elmajs's getTime
pub fn frames_centiseconds(ride: &Ride) -> i64 {
    let ms = (ride.frames.len() as f64 * (1000.0 / ride.frame_rate)).round();
    (ms / 10.0).floor() as i64
}

// Hundredths as seconds, half a hundredth over so rounding down gives them back
pub fn centiseconds_to_seconds(centiseconds: i64) -> f64 {
    (centiseconds as f64 + 0.5) / 100.0
}

pub fn time_to_seconds(time: f64) -> f64 {
    time / (1000.0 * MILLISECONDS_TO_PHYS_TIME)
}

// Bike position in the game's coordinate system: meters, y pointing up
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Pose {
    pub bike_r: Vec2,
    pub bike_rotation: f64,
    pub left_wheel_r: Vec2,
    pub left_wheel_rotation: f64,
    pub right_wheel_r: Vec2,
    pub right_wheel_rotation: f64,
    pub body_r: Vec2,
    pub head_r: Vec2,
    pub flipped: bool,
    pub gas: bool,
    pub flagtag_a: bool,
    pub flagtag_immunity: bool,
    pub motor_frequency: f64,
    pub friction_volume: f64,
}

fn wrapped(a: i32, b: i32, range: i32) -> (i32, i32) {
    if (a - b).abs() > range / 2 {
        if a > b {
            (a - range, b)
        } else {
            (a, b - range)
        }
    } else {
        (a, b)
    }
}

// Blend the two recorded frames around `time`. The flag is true once `time` reaches the last
// frame.
pub fn interpolate(ride: &Ride, time: f64) -> (Pose, bool) {
    let frames = &ride.frames;
    // The parsers never give a ride without frames, but Ride's fields are public
    if frames.is_empty() {
        return (Pose::default(), true);
    }
    let n = frames.len() as i64;
    let raw = frames_per_time(ride) * time;
    let mut index1 = raw as i64;
    let w2 = (raw - index1 as f64).clamp(0.0, 1.0);
    let w1 = 1.0 - w2;
    let mut index2 = index1 + 1;
    index1 = index1.max(0);
    index2 = index2.max(0);
    let at_end = index1 >= n - 1;
    if at_end {
        index1 = n - 1;
    }
    index2 = index2.min(n - 1);
    let f1 = &frames[index1 as usize];
    let f2 = &frames[index2 as usize];

    let blend = |a: f64, b: f64| a * w1 + b * w2;
    let bike_r = Vec2::new(
        blend(f1.bike_x as f64, f2.bike_x as f64),
        blend(f1.bike_y as f64, f2.bike_y as f64),
    );
    let offset = |ax: i16, ay: i16, bx: i16, by: i16| {
        bike_r
            + Vec2::new(
                blend(ax as f64, bx as f64) / POSITION_RATIO,
                blend(ay as f64, by as f64) / POSITION_RATIO,
            )
    };
    let left_wheel_r = offset(
        f1.left_wheel_x,
        f1.left_wheel_y,
        f2.left_wheel_x,
        f2.left_wheel_y,
    );
    let right_wheel_r = offset(
        f1.right_wheel_x,
        f1.right_wheel_y,
        f2.right_wheel_x,
        f2.right_wheel_y,
    );
    let body_r = offset(f1.body_x, f1.body_y, f2.body_x, f2.body_y);

    let rotation = |a: i32, b: i32, range: i32, ratio: f64| {
        let (a, b) = wrapped(a, b, range);
        blend(a as f64, b as f64) / ratio
    };
    let bike_rotation = rotation(
        f1.bike_rotation as i32,
        f2.bike_rotation as i32,
        BIKE_ROTATION_RANGE,
        BIKE_ROTATION_RATIO,
    );
    let left_wheel_rotation = rotation(
        f1.left_wheel_rotation as i32,
        f2.left_wheel_rotation as i32,
        WHEEL_ROTATION_RANGE,
        WHEEL_ROTATION_RATIO,
    );
    let right_wheel_rotation = rotation(
        f1.right_wheel_rotation as i32,
        f2.right_wheel_rotation as i32,
        WHEEL_ROTATION_RANGE,
        WHEEL_ROTATION_RATIO,
    );

    let flipped = f1.flags & FLAG_FLIPPED != 0;
    let mut pose = Pose {
        bike_r,
        bike_rotation,
        left_wheel_r,
        left_wheel_rotation,
        right_wheel_r,
        right_wheel_rotation,
        body_r,
        head_r: Vec2::default(),
        flipped,
        gas: f1.flags & FLAG_GAS != 0,
        flagtag_a: f1.flags & FLAG_FLAGTAG_A != 0,
        flagtag_immunity: f1.flags & FLAG_FLAGTAG_IMMUNITY != 0,
        motor_frequency: 1.0 + f1.motor_frequency as f64 / MOTOR_FREQUENCY_RATIO,
        friction_volume: f1.friction_volume as f64 / FRICTION_VOLUME_RATIO,
    };
    pose.head_r = head_position(&pose);
    (pose, at_end)
}

// The head is at a fixed position relative to the body
pub fn head_position(pose: &Pose) -> Vec2 {
    let i = Vec2::new(pose.bike_rotation.cos(), pose.bike_rotation.sin());
    let j = i.rotate_90();
    if pose.flipped {
        pose.body_r + i * 0.09 + j * 0.63
    } else {
        pose.body_r - i * 0.09 + j * 0.63
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Turning {
    pub flipped: bool,
    pub turn_time: f64,
    pub turn_phase: f64,
}

impl Default for Turning {
    // Facing left and done turning, as the game's first update leaves it: drawn before
    // that update, a phase of 0 would show the bike just starting to turn, facing right
    fn default() -> Self {
        Turning {
            flipped: false,
            turn_time: -1000.0,
            turn_phase: 1.0,
        }
    }
}

// Animation state that the game derives from a replay while it plays
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Animation {
    pub volt_time: f64,
    pub volt_is_right: bool,
    pub arm_position: f64,
    pub bike_turning: Turning,
    pub camera_turning: Turning,
}

impl Default for Animation {
    fn default() -> Self {
        Animation {
            volt_time: -100.0,
            volt_is_right: false,
            arm_position: 0.0,
            bike_turning: Turning::default(),
            camera_turning: Turning::default(),
        }
    }
}

impl Animation {
    fn note_volt(&mut self, event: &Event, time: f64) {
        match event.event_id {
            WavEvent::RightVolt => {
                self.volt_is_right = true;
                self.volt_time = time;
            }
            WavEvent::LeftVolt => {
                self.volt_is_right = false;
                self.volt_time = time;
            }
            _ => {}
        }
    }

    fn update(&mut self, pose: &Pose, gravity: Gravity, time: f64, turn_time: f64) {
        let data = &mut self.bike_turning;
        if data.flipped != pose.flipped {
            data.flipped = pose.flipped;
            data.turn_time = time;
        }
        data.turn_phase = if turn_time == 0.0 {
            1.0
        } else {
            ((time - data.turn_time) / turn_time).clamp(0.0, 1.0)
        };

        let flipped_camera = pose.flipped != (gravity == Gravity::Up);
        let camera_flip_time = turn_time + 0.15;
        let data = &mut self.camera_turning;
        if data.flipped != flipped_camera {
            data.flipped = flipped_camera;
            let since_previous = time - data.turn_time;
            if camera_flip_time > 0.0 && since_previous < camera_flip_time {
                // Continue seamlessly from the mid-turn position
                data.turn_time = time + since_previous - camera_flip_time;
            } else {
                data.turn_time = time;
            }
        }
        let elapsed = (time - data.turn_time).max(0.0);
        data.turn_phase = (elapsed / camera_flip_time).min(1.0);
        if flipped_camera {
            data.turn_phase = 1.0 - data.turn_phase;
        }

        self.arm_position = (1.0 - (time - self.volt_time) / VOLT_DELAY).max(0.0);
    }
}

// One bike driven by its recording
#[derive(Debug, Clone)]
pub struct Rider {
    pub ride: Arc<Ride>,
    pub pose: Pose,
    pub animation: Animation,
    pub gravity: Gravity,
    pub apple_count: i32,
    pub finish_time: Option<f64>,
    // The replay's time, which the timer shows at its end: when it finishes, which can be a
    // moment after the last frame, or else the length of its frames, like elmajs's getTime.
    // As game time, and in hundredths rounded like elmajs.
    pub end: f64,
    pub end_centiseconds: i64,
    event_index: usize,
    finished: bool,
}

impl Rider {
    pub fn new(ride: Arc<Ride>) -> Rider {
        let (pose, _) = interpolate(&ride, 0.0);
        let end = ride.frames.len() as f64 * time_per_frame(&ride);
        let end_centiseconds = frames_centiseconds(&ride);
        Rider {
            ride,
            pose,
            animation: Animation::default(),
            gravity: Gravity::Down,
            apple_count: 0,
            finish_time: None,
            end,
            end_centiseconds,
            event_index: 0,
            finished: false,
        }
    }

    // What the timer shows at `time`, in hundredths: the replay's time once its last frame is
    // reached
    pub fn timer_centiseconds(&self, time: f64) -> i64 {
        if time >= self.duration() {
            self.end_centiseconds
        } else {
            (time.min(self.end) * TIME_TO_CENTISECONDS) as i64
        }
    }

    // Time at which the last frame is reached
    pub fn duration(&self) -> f64 {
        (self.ride.frames.len() as f64 - 1.0).max(0.0) * time_per_frame(&self.ride)
    }

    // The objects this rider has touched by `time`, as indexes into the session's objects.
    // Ghosts don't take apples in the session, so this tells which ones they have taken.
    pub fn touched(&self, time: f64) -> impl Iterator<Item = usize> + '_ {
        self.ride
            .events
            .iter()
            .take_while(move |e| e.time <= time)
            .filter_map(|e| usize::try_from(e.object_id).ok())
    }

    fn rewind(&mut self) {
        self.animation = Animation::default();
        self.gravity = Gravity::Down;
        self.apple_count = 0;
        self.finish_time = None;
        self.event_index = 0;
        self.finished = false;
    }

    // Load the frame at `time` and hand over the events up to it, or every event left at the
    // last frame with `events_after_end`. Returns false once the replay has run out of frames,
    // one call after the last frame was shown.
    fn recall(
        &mut self,
        time: f64,
        events_after_end: bool,
        mut on_event: impl FnMut(&mut Rider, Event),
    ) -> bool {
        let (pose, at_end) = interpolate(&self.ride, time);
        let until = if at_end && events_after_end {
            f64::INFINITY
        } else {
            time
        };
        let alive = if at_end {
            let alive = !self.finished;
            self.finished = true;
            alive
        } else {
            self.finished = false;
            true
        };
        if alive {
            self.pose = pose;
        } else {
            self.pose.gas = false;
            self.pose.motor_frequency = 1.0;
            self.pose.friction_volume = 0.0;
        }
        while let Some(&event) = self.ride.events.get(self.event_index) {
            if event.time > until {
                break;
            }
            self.event_index += 1;
            if event.object_id < 0 {
                self.animation.note_volt(&event, time);
            }
            on_event(self, event);
        }
        alive
    }

    fn update_animation(&mut self, time: f64, turn_time: f64) {
        let pose = self.pose;
        self.animation.update(&pose, self.gravity, time, turn_time);
    }

    // After recalling a jump target: the animations the game rebuilds after a rewind
    fn settle(&mut self, time: f64, turn_time: f64) {
        self.finished = false;
        self.override_animation(time);
        self.update_animation(time, turn_time);
    }

    // The animation state the game rebuilds after a rewind
    fn override_animation(&mut self, time: f64) {
        let frames = &self.ride.frames;
        if frames.is_empty() {
            return;
        }
        let index = ((frames_per_time(&self.ride) * time) as i64).clamp(0, frames.len() as i64 - 1)
            as usize;
        let current = frames[index].flags & FLAG_FLIPPED != 0;
        let turn_time = (0..index)
            .rev()
            .find(|&i| (frames[i].flags & FLAG_FLIPPED != 0) != current)
            .map(|i| (i + 1) as f64 * time_per_frame(&self.ride))
            .unwrap_or(-1000.0);
        self.animation.bike_turning.flipped = self.pose.flipped;
        self.animation.bike_turning.turn_time = turn_time;
        self.animation.camera_turning.turn_time = -1000.0;
        self.animation.camera_turning.flipped = self.pose.flipped != (self.gravity == Gravity::Up);
        self.animation.volt_time = -1000.0;
        for event in self.ride.events[..self.event_index].iter().rev() {
            match event.event_id {
                WavEvent::RightVolt | WavEvent::LeftVolt => {
                    self.animation.volt_is_right = event.event_id == WavEvent::RightVolt;
                    self.animation.volt_time = event.time;
                    break;
                }
                _ => {}
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct LevelObject {
    // y pointing up, like the bikes
    pub r: Vec2,
    pub kind: ObjectType,
    pub gravity: Gravity,
    pub animation: i32,
    pub active: bool,
    // Apples and the exit bob up and down with a random phase
    pub floating_phase: f64,
    // Across objects that move, in file coordinates
    pub motion: Option<Motion>,
}

impl LevelObject {
    // Where the object is at `time`, y pointing up
    pub fn position(&self, time: f64) -> Vec2 {
        match &self.motion {
            Some(motion) => {
                let r = motion.position(time);
                Vec2::new(r.x, -r.y)
            }
            None => self.r,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sound {
    pub event: WavEvent,
    pub volume: f32,
}

#[derive(Debug, Clone, Default)]
pub struct Step {
    // Sound events of the main bikes, plus the finish sound
    pub sounds: Vec<Sound>,
    // Per main bike: false once it ran out of frames
    pub alive: Vec<bool>,
    // Every bike, ghosts included, has run out of frames
    pub done: bool,
}

// The game a replay is of, and the rules that differ between them
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Game {
    #[default]
    Elma,
    // `old` for the replays of Across 1.0 and 1.1
    Across {
        old: bool,
    },
}

impl Game {
    pub fn is_across(self) -> bool {
        matches!(self, Game::Across { .. })
    }

    // Whether the objects are played in the game's order. Events of Across 1.0 and 1.1
    // replays refer to the objects in the level's order instead.
    pub fn sort_objects(self) -> bool {
        self != Game::Across { old: true }
    }

    // Across stops recording a moment before the last apples and the exit are taken, so their
    // events come after the last frame
    pub fn events_after_end(self) -> bool {
        self.is_across()
    }

    // Where the classic camera keeps the bike, as a share of the view's width from its edge.
    // None for Elma's: 15% of the screen at zoom 1, the same distance in meters at any zoom.
    pub fn camera_edge(self) -> Option<f64> {
        match self {
            Game::Elma => None,
            // Mo_bal at the game's normal size (PLAY.CPP: motorhezag)
            Game::Across { .. } => Some(0.28),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Settings {
    pub game: Game,
    pub turn_time: f64,
    // Seed for the objects' floating phase
    pub seed: u32,
}

impl Settings {
    pub fn new(game: Game) -> Settings {
        Settings {
            game,
            ..Settings::default()
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            game: Game::Elma,
            turn_time: DEFAULT_TURN_TIME,
            seed: 1,
        }
    }
}

// A replay of a level, with ghosts, played back over time
#[derive(Debug, Clone)]
pub struct Session {
    pub objects: Vec<LevelObject>,
    pub total_apples: i32,
    pub flagtag: bool,
    pub riders: Vec<Rider>,
    pub ghosts: Vec<Rider>,
    pub flag_time_a: f64,
    pub flag_time_b: f64,
    pub time: f64,
    flagtag_elapsed: f64,
    settings: Settings,
}

impl Session {
    pub fn new(level: &Level, rec: &Rec, ghosts: &[Arc<Ride>], settings: Settings) -> Session {
        let mut session = Session::unplayed(level, rec, ghosts, settings);
        // Each bike's end is when it finishes, which its touches of the objects tell. A two
        // player replay's is its latest finish, like elmajs's getTime.
        if let Some(finish) = finishes(&session).into_iter().flatten().reduce(f64::max) {
            for rider in &mut session.riders {
                rider.end = finish;
                rider.end_centiseconds = finish_centiseconds(finish);
            }
        }
        for ghost in &mut session.ghosts {
            if let Some(finish) = finish_time(level, rec, &ghost.ride, session.settings.clone()) {
                ghost.end = finish;
                ghost.end_centiseconds = finish_centiseconds(finish);
            }
        }
        session
    }

    // A session whose riders' ends are only the length of their frames
    fn unplayed(level: &Level, rec: &Rec, ghosts: &[Arc<Ride>], settings: Settings) -> Session {
        let mut seed = settings.seed;
        let mut random = || {
            // xorshift, so the floating phases don't depend on platform rand()
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed % 1000
        };
        let objects = if settings.game.sort_objects() {
            level.sorted_objects()
        } else {
            level.objects.clone()
        };
        let objects: Vec<LevelObject> = objects
            .into_iter()
            .map(|o| LevelObject {
                r: Vec2::new(o.r.x, -o.r.y),
                kind: o.kind,
                gravity: o.gravity,
                animation: o.animation,
                active: o.kind != ObjectType::Start,
                floating_phase: random() as f64 * 2.0 * std::f64::consts::PI / 1000.0,
                motion: o.motion,
            })
            .collect();
        let total_apples = objects
            .iter()
            .filter(|o| o.kind == ObjectType::Food)
            .count() as i32;
        let mut riders: Vec<Rider> = rec
            .rides
            .iter()
            .map(|ride| Rider::new(Arc::new(ride.clone())))
            .collect();
        // There is always a bike to follow. The parsers never give a replay without rides,
        // but Rec's fields are public.
        if riders.is_empty() {
            riders.push(Rider::new(Arc::new(Ride {
                frames: Vec::new(),
                events: Vec::new(),
                frame_rate: elma_formats::rec::FRAME_RATE,
            })));
        }
        let ghosts = ghosts.iter().cloned().map(Rider::new).collect();
        Session {
            objects,
            total_apples,
            flagtag: rec.flagtag && rec.multiplayer,
            riders,
            ghosts,
            flag_time_a: 0.0,
            flag_time_b: 0.0,
            time: 0.0,
            flagtag_elapsed: 0.0,
            settings,
        }
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    // What the timer shows at `time` following the main replay, in hundredths: its time once
    // its riders have all reached their last frame
    pub fn main_timer_centiseconds(&self, time: f64) -> i64 {
        let duration = self.riders.iter().map(Rider::duration).fold(0.0, f64::max);
        let end = self.riders.iter().map(|r| r.end).fold(0.0, f64::max);
        if time >= duration {
            self.riders
                .iter()
                .map(|r| r.end_centiseconds)
                .max()
                .unwrap_or(0)
        } else {
            (time.min(end) * TIME_TO_CENTISECONDS) as i64
        }
    }

    // The time to show now, in seconds: the replay's own once every bike has reached its
    // last frame
    pub fn shown_seconds(&self) -> f64 {
        if self.time >= self.duration() {
            self.end_seconds()
        } else {
            self.seconds()
        }
    }

    // The replay's time in seconds, see Rider::end
    pub fn end_seconds(&self) -> f64 {
        let centiseconds = self
            .riders
            .iter()
            .chain(&self.ghosts)
            .map(|r| r.end_centiseconds)
            .max()
            .unwrap_or(0);
        centiseconds_to_seconds(centiseconds)
    }

    // Time at which every bike has reached its last frame
    pub fn duration(&self) -> f64 {
        self.riders
            .iter()
            .chain(&self.ghosts)
            .map(Rider::duration)
            .fold(0.0, f64::max)
    }

    pub fn finish_time(&self) -> Option<f64> {
        self.riders
            .iter()
            .filter_map(|r| r.finish_time)
            .reduce(f64::min)
    }

    fn handle_object_event(
        objects: &mut [LevelObject],
        total_apples: i32,
        apples_taken: i32,
        rider: &mut Rider,
        event: &Event,
    ) {
        let Some(object) = objects.get_mut(event.object_id as usize) else {
            return;
        };
        match object.kind {
            ObjectType::Food => {
                object.active = false;
                rider.apple_count += 1;
                if object.gravity != Gravity::None {
                    rider.gravity = object.gravity;
                }
            }
            ObjectType::Exit if apples_taken >= total_apples && rider.finish_time.is_none() => {
                rider.finish_time = Some(event.time);
            }
            _ => {}
        }
    }

    // Recall main rider `i` at `time`, handling the objects it touches. Its sound events go to
    // `sounds` when given. Returns false once it ran out of frames.
    fn recall_rider(&mut self, i: usize, time: f64, mut sounds: Option<&mut Vec<Sound>>) -> bool {
        let total: i32 = self.riders.iter().map(|r| r.apple_count).sum();
        let others = total - self.riders[i].apple_count;
        let objects = &mut self.objects;
        let total_apples = self.total_apples;
        let after_end = self.settings.game.events_after_end();
        self.riders[i].recall(time, after_end, |rider, event| {
            if event.object_id >= 0 {
                // Apples count before this event is handled, like the game
                let taken = rider.apple_count + others;
                Self::handle_object_event(objects, total_apples, taken, rider, &event);
            } else if let Some(sounds) = &mut sounds {
                sounds.push(Sound {
                    event: event.event_id,
                    volume: event.volume,
                });
            }
        })
    }

    // Move forward to `time`, the way the game's replay loop does each frame
    pub fn step(&mut self, time: f64) -> Step {
        let was_finished = self.finish_time().is_some();
        let turn_time = self.settings.turn_time;
        let mut step = Step::default();

        let mut ghosts_finished = true;
        for ghost in &mut self.ghosts {
            if ghost.recall(time, false, |_, _| {}) {
                ghosts_finished = false;
                ghost.update_animation(time, turn_time);
            }
        }

        for i in 0..self.riders.len() {
            let alive = self.recall_rider(i, time, Some(&mut step.sounds));
            step.alive.push(alive);
        }

        if !was_finished && self.finish_time().is_some() {
            step.sounds.push(Sound {
                event: WavEvent::Win,
                volume: 0.999,
            });
        }

        step.done = step.alive.iter().all(|&alive| !alive) && ghosts_finished;
        for (rider, &alive) in self.riders.iter_mut().zip(&step.alive) {
            if alive {
                rider.update_animation(time, turn_time);
            }
        }

        if self.flagtag {
            let a_has_flag = self.riders.last().is_some_and(|r| r.pose.flagtag_a);
            if a_has_flag {
                self.flag_time_a += time - self.flagtag_elapsed;
            } else {
                self.flag_time_b += time - self.flagtag_elapsed;
            }
            self.flagtag_elapsed = time;
        }
        self.time = time;
        step
    }

    // Jump to `time`, rebuilding the level state and animations the way a rewind does
    pub fn seek(&mut self, time: f64) {
        let turn_time = self.settings.turn_time;
        for object in &mut self.objects {
            object.active = object.kind != ObjectType::Start;
        }
        // Every apple count starts over before any rider is recalled
        for rider in &mut self.riders {
            rider.rewind();
        }
        for i in 0..self.riders.len() {
            self.recall_rider(i, time, None);
            self.riders[i].settle(time, turn_time);
        }
        for ghost in &mut self.ghosts {
            ghost.rewind();
            ghost.recall(time, false, |_, _| {});
            ghost.settle(time, turn_time);
        }

        // Flag tag time is only accumulated frame by frame
        self.flag_time_a = 0.0;
        self.flag_time_b = 0.0;
        self.flagtag_elapsed = time;
        self.time = time;
    }

    // Current position and length in real seconds
    pub fn seconds(&self) -> f64 {
        time_to_seconds(self.time)
    }

    pub fn duration_seconds(&self) -> f64 {
        time_to_seconds(self.duration())
    }

    // Move to `seconds` while playing: stepping forward like the game when it is less than a
    // second ahead, otherwise rebuilding the state. Returns the step when it stepped.
    pub fn set_seconds(&mut self, seconds: f64) -> Option<Step> {
        let time = seconds_to_time(seconds.clamp(0.0, self.duration_seconds()));
        if time >= self.time && time - self.time < seconds_to_time(1.0) {
            Some(self.step(time))
        } else {
            self.seek(time);
            None
        }
    }
}

// A replay of one frame with the bike waiting at the level's start, to look at a level
// without a replay. The start object marks the left wheel.
pub fn start_rec(level: &Level) -> Rec {
    let start = level
        .objects
        .iter()
        .find(|o| o.kind == ObjectType::Start)
        .map_or(Vec2::default(), |o| Vec2::new(o.r.x, -o.r.y));
    let frame = Frame {
        bike_x: (start.x + 0.85) as f32,
        bike_y: (start.y + 0.6) as f32,
        left_wheel_x: -850,
        left_wheel_y: -600,
        right_wheel_x: 850,
        right_wheel_y: -600,
        body_y: 440,
        ..Default::default()
    };
    Rec {
        multiplayer: false,
        flagtag: false,
        level_id: level.level_id,
        level_filename: String::new(),
        rides: vec![Ride {
            frames: vec![frame],
            events: vec![],
            frame_rate: FRAME_RATE,
        }],
    }
}

// When each ride of `rec` reaches the exit, playing them together: in a two player replay
// the apples of both count
pub fn finish_times(level: &Level, rec: &Rec, settings: Settings) -> Vec<Option<f64>> {
    finishes(&Session::unplayed(level, rec, &[], settings))
}

// When a ride reaches the exit with every apple taken, in game time
pub fn finish_time(level: &Level, rec: &Rec, ride: &Ride, settings: Settings) -> Option<f64> {
    let single = Rec {
        multiplayer: false,
        flagtag: false,
        level_id: rec.level_id,
        level_filename: rec.level_filename.clone(),
        rides: vec![ride.clone()],
    };
    let session = Session::unplayed(level, &single, &[], settings);
    finishes(&session)[0]
}

// When each of the session's riders finishes: their touches of the level's objects in time
// order, both riders' together. A rider finishes on reaching the exit once every apple is
// taken, by either rider of a two player replay, and is out once it touches a killer.
fn finishes(session: &Session) -> Vec<Option<f64>> {
    let objects = &session.objects;
    let kind = |object: usize| objects.get(object).map(|o| o.kind);
    let mut touches: Vec<(f64, usize, usize)> = session
        .riders
        .iter()
        .enumerate()
        .flat_map(|(rider, r)| {
            r.ride
                .events
                .iter()
                .filter_map(move |e| Some((e.time, rider, usize::try_from(e.object_id).ok()?)))
        })
        .collect();
    // The game moves the first bike and then the second, so the other rider's apple taken at
    // the same moment counts only when the exit is reached by the second. A rider's own apple
    // counts for the exit it reaches at the same moment.
    touches.sort_by(|a, b| {
        let exit = |t: &(f64, usize, usize)| kind(t.2) == Some(ObjectType::Exit);
        a.0.total_cmp(&b.0)
            .then(a.1.cmp(&b.1))
            .then(exit(a).cmp(&exit(b)))
    });
    let mut taken = vec![false; objects.len()];
    let mut apples = 0;
    let mut out = vec![false; session.riders.len()];
    let mut finishes = vec![None; session.riders.len()];
    for (time, rider, object) in touches {
        if out[rider] {
            continue;
        }
        match kind(object) {
            Some(ObjectType::Food) if !taken[object] => {
                taken[object] = true;
                apples += 1;
            }
            Some(ObjectType::Exit) if apples >= session.total_apples => {
                finishes[rider] = Some(time);
                out[rider] = true;
            }
            Some(ObjectType::Killer) => out[rider] = true,
            _ => {}
        }
    }
    finishes
}

#[cfg(test)]
mod session_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(x: f32, rotation: i16, flags: u8) -> Frame {
        Frame {
            bike_x: x,
            bike_y: 1.0,
            left_wheel_x: -850,
            body_y: 440,
            bike_rotation: rotation,
            left_wheel_rotation: 249,
            flags,
            motor_frequency: 125,
            friction_volume: 125,
            ..Default::default()
        }
    }

    fn ride(frames: &[Frame]) -> Ride {
        Ride {
            frames: frames.to_vec(),
            events: vec![],
            frame_rate: FRAME_RATE,
        }
    }

    #[test]
    fn interpolates_between_frames() {
        let frames = ride(&[frame(0.0, 0, 0), frame(3.0, 0, 0), frame(6.0, 0, 0)]);
        let (pose, at_end) = interpolate(&frames, 0.5 * FRAME_INDEX_TO_TIME);
        assert!(!at_end);
        assert!((pose.bike_r.x - 1.5).abs() < 1e-9);
        assert!((pose.left_wheel_r.x - 0.65).abs() < 1e-9);
        assert!((pose.body_r.y - 1.44).abs() < 1e-9);
        assert!((pose.motor_frequency - 1.5).abs() < 1e-9);
        assert!((pose.friction_volume - 1.0).abs() < 1e-9);
        let (pose, at_end) = interpolate(&frames, 10.0);
        assert!(at_end);
        assert_eq!(pose.bike_r.x, 6.0);
    }

    #[test]
    fn wraps_rotation_the_short_way() {
        let frames = ride(&[frame(0.0, 9990, 0), frame(0.0, 10, 0)]);
        let (pose, _) = interpolate(&frames, 0.5 * FRAME_INDEX_TO_TIME);
        assert!(pose.bike_rotation.abs() < 1e-9);
    }

    #[test]
    fn rider_stops_one_call_after_the_last_frame() {
        let mut rider = Rider::new(Arc::new(ride(&[frame(0.0, 0, 0), frame(1.0, 0, 0)])));
        assert!(rider.recall(0.0, false, |_, _| {}));
        assert!(rider.recall(FRAME_INDEX_TO_TIME, false, |_, _| {}));
        assert!(!rider.recall(2.0 * FRAME_INDEX_TO_TIME, false, |_, _| {}));
    }

    #[test]
    fn a_replay_without_rides_has_a_bike() {
        let level = level_with_start();
        let rec = Rec {
            rides: vec![],
            ..start_rec(&level)
        };
        let mut session = Session::new(&level, &rec, &[], Settings::default());
        assert_eq!(session.riders.len(), 1);
        session.seek(1.0);
        session.step(0.5);
    }

    #[test]
    fn a_ride_without_frames_is_over_at_once() {
        let mut rider = Rider::new(Arc::new(ride(&[])));
        assert!(rider.recall(1.0, true, |_, _| {}));
        assert!(!rider.recall(2.0, true, |_, _| {}));
        rider.settle(0.5, 0.0);
    }

    #[test]
    fn touched_lists_the_objects_up_to_the_time() {
        let event = |time, object_id| Event {
            time,
            object_id,
            event_id: WavEvent::None,
            volume: 0.0,
        };
        let rider = Rider::new(Arc::new(Ride {
            frames: vec![frame(0.0, 0, 0)],
            // A sound event has no object
            events: vec![event(0.1, 2), event(0.2, -1), event(0.3, 0), event(0.5, 1)],
            frame_rate: FRAME_RATE,
        }));
        assert_eq!(rider.touched(0.3).collect::<Vec<_>>(), [2, 0]);
        assert_eq!(rider.touched(0.05).count(), 0);
    }

    // A level with only a start, at (2, 3)
    fn level_with_start() -> Level {
        use elma_formats::lev::{Object, TopTens, Vec2 as LevVec2};
        Level {
            level_id: 7,
            name: String::new(),
            lgr: String::new(),
            foreground: String::new(),
            background: String::new(),
            polygons: vec![],
            objects: vec![Object {
                r: LevVec2 { x: 2.0, y: 3.0 },
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

    #[test]
    fn start_rec_puts_the_left_wheel_on_the_start() {
        let level = level_with_start();
        let rec = start_rec(&level);
        assert_eq!(rec.level_id, 7);
        let session = Session::new(&level, &rec, &[], Settings::default());
        let pose = session.riders[0].pose;
        assert!((pose.left_wheel_r.x - 2.0).abs() < 1e-6);
        assert!((pose.left_wheel_r.y + 3.0).abs() < 1e-6);
        assert_eq!(session.duration(), 0.0);
    }

    #[test]
    fn camera_turn_continues_from_mid_turn() {
        let mut animation = Animation::default();
        let mut pose = Pose::default();
        animation.update(&pose, Gravity::Down, 0.0, DEFAULT_TURN_TIME);
        pose.flipped = true;
        animation.update(&pose, Gravity::Down, 1.0, DEFAULT_TURN_TIME);
        animation.update(&pose, Gravity::Down, 1.1, DEFAULT_TURN_TIME);
        let mid = animation.camera_turning.turn_phase;
        pose.flipped = false;
        animation.update(&pose, Gravity::Down, 1.1, DEFAULT_TURN_TIME);
        assert!((animation.camera_turning.turn_phase - mid).abs() < 1e-9);
    }
}
