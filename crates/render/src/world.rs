// What a frame shows, wherever it comes from: a replay playing back, a game being played, bikes
// from the network. `Renderer::render_world` draws one; `Renderer::render` makes one from a
// replay's Session first, with `SessionFrame`.

use crate::camera::followed;
use crate::options::{CameraMode, ViewParams};
use replay::{Animation, LevelObject, Pose, Session, TIME_TO_CENTISECONDS, Vec2};
use std::ops::Range;

// A bike somewhere in the level
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Bike {
    pub pose: Pose,
    pub animation: Animation,
    // Its shirt by index in the scene's, which takes the place of the LGR's body
    pub shirt: Option<usize>,
}

// Everything in the level at one moment, which each view looks at
#[derive(Debug, Clone, Copy)]
pub struct World<'a> {
    // As replay::Session's time, which the objects turn through their frames and bob with
    pub time: f64,
    pub bikes: &'a [Bike],
    // The level's objects in the order of its file, which the scene was built from
    pub objects: &'a [LevelObject],
    // The exit is left out, as in a two player flag tag game
    pub hide_exit: bool,
}

// A bike as a view draws it
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawnBike {
    // Index in World::bikes
    pub bike: usize,
    // Which of the LGR's two bike pictures, 0 or 1; Across has one
    pub pictures: usize,
    // It carries the flag of flag tag
    pub flag: bool,
}

// The timer of a view, in hundredths
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Timer {
    pub centiseconds: i64,
    // Shown left of it
    pub best: Option<i64>,
    // How long the bike has had the flag, in a flag tag game
    pub flagtag: Option<i64>,
}

// One view of the world
#[derive(Debug, Clone)]
pub struct View<'a> {
    // x, y, width, height in target pixels, from the top left
    pub viewport: [u32; 4],
    pub zoom: f64,
    // The bike the camera follows and the minimap marks, by index in World::bikes
    pub follow: usize,
    pub camera: CameraMode,
    // Where the classic camera keeps the bike, as a share of the view's width: None for
    // Elma's, which depends on the zoom
    pub camera_edge: Option<f64>,
    // Shift of the camera from the bike, in meters
    pub pan: Vec2,
    // Look at this point instead of following the bike
    pub center: Option<Vec2>,
    // The bikes in the order they are drawn, after the objects
    pub bikes: &'a [DrawnBike],
    // Those of `bikes` that are drawn see-through with GhostStyle::Translucent
    pub translucent: Range<usize>,
    // Which objects are there in this view and its minimap, by index in World::objects,
    // instead of their `active`
    pub objects: Option<&'a [bool]>,
    pub timer: Option<Timer>,
    // The game's "last apple" row in the bottom right: how many apples are taken, and when
    // the last one was in hundredths
    pub last_apple: Option<(i32, i64)>,
    pub minimap: bool,
}

impl View<'_> {
    pub(crate) fn followed<'w>(&self, world: &World<'w>) -> Option<&'w Bike> {
        world.bikes.get(self.follow)
    }

    // Object `index` of the world is there to be drawn in this view
    pub(crate) fn object_there(&self, index: usize, object: &LevelObject) -> bool {
        match self.objects {
            Some(there) => there.get(index).copied().unwrap_or(false),
            None => object.active,
        }
    }
}

// What one ViewParams becomes, as ranges of SessionFrame's buffers
#[derive(Debug, Clone)]
struct SessionView {
    params: ViewParams,
    follow: usize,
    bikes: Range<usize>,
    translucent: Range<usize>,
    objects: Option<Range<usize>>,
    timer: Option<Timer>,
    last_apple: Option<(i32, i64)>,
    camera_edge: Option<f64>,
}

// A replay's frame as a World and its Views, drawn as viewers have always drawn a Session:
// the bikes are its riders, then its ghosts. Its buffers are kept between frames.
#[derive(Debug, Default)]
pub(crate) struct SessionFrame {
    bikes: Vec<Bike>,
    drawn: Vec<DrawnBike>,
    objects: Vec<bool>,
    views: Vec<SessionView>,
}

impl SessionFrame {
    pub(crate) fn build(&mut self, session: &Session, params: &[ViewParams]) {
        self.bikes.clear();
        self.bikes.extend(
            session
                .riders
                .iter()
                .chain(&session.ghosts)
                .enumerate()
                .map(|(i, r)| Bike {
                    pose: r.pose,
                    animation: r.animation,
                    shirt: Some(i),
                }),
        );
        self.drawn.clear();
        self.objects.clear();
        self.views.clear();
        for p in params {
            let view = self.view(session, p);
            self.views.push(view);
        }
    }

    pub(crate) fn world<'a>(&'a self, session: &'a Session) -> World<'a> {
        World {
            time: session.time,
            bikes: &self.bikes,
            objects: &session.objects,
            hide_exit: session.riders.len() > 1 && session.flagtag,
        }
    }

    pub(crate) fn views(&self) -> impl Iterator<Item = View<'_>> {
        self.views.iter().map(|v| View {
            viewport: v.params.viewport,
            zoom: v.params.zoom,
            follow: v.follow,
            camera: v.params.camera,
            camera_edge: v.camera_edge,
            pan: v.params.pan,
            center: v.params.center,
            bikes: &self.drawn[v.bikes.clone()],
            translucent: v.translucent.clone(),
            objects: v.objects.clone().map(|r| &self.objects[r]),
            timer: v.timer,
            last_apple: v.last_apple,
            minimap: v.params.minimap,
        })
    }

    fn view(&mut self, session: &Session, params: &ViewParams) -> SessionView {
        let riders = session.riders.len();
        let count = riders + session.ghosts.len();
        let follow = params.follow.min(count.saturating_sub(1));
        // Following a ghost draws the main bikes as when following the first, and the
        // apples and timer are the ghost's
        let ghost = follow.checked_sub(riders);
        let rider = if ghost.is_some() { 0 } else { follow };
        let multiplayer = riders > 1;

        let objects = ghost.map(|g| {
            let start = self.objects.len();
            self.objects.resize(start + session.objects.len(), true);
            let taken = &mut self.objects[start..];
            for i in session.ghosts[g].touched(session.time) {
                if let Some(t) = taken.get_mut(i) {
                    *t = false;
                }
            }
            start..self.objects.len()
        });

        // The followed bike uses the first bike pictures, everyone else the second
        let (own_pictures, other_pictures) = if rider == 0 { (0, 1) } else { (1, 0) };
        let flagtag = multiplayer && session.flagtag;
        let last = session.riders.last().map(|r| r.pose);
        let a_has_flag = last.is_some_and(|p| p.flagtag_a);
        let immunity = last.is_some_and(|p| p.flagtag_immunity);
        let has_flag = |index: usize| {
            flagtag
                && ((index == 0) == a_has_flag
                    || (immunity && (session.time * 30.0) as i64 % 2 != 0))
        };
        let drawn = &mut self.drawn;
        let start = drawn.len();
        let ghost_bike = |i: usize| DrawnBike {
            bike: riders + i,
            pictures: other_pictures,
            flag: false,
        };
        let draw_riders = |drawn: &mut Vec<DrawnBike>| {
            // Each rider in their own shirt, whichever view follows them
            if multiplayer {
                drawn.push(DrawnBike {
                    bike: 1 - rider,
                    pictures: other_pictures,
                    flag: has_flag(1 - rider),
                });
            }
            drawn.push(DrawnBike {
                bike: rider,
                pictures: own_pictures,
                flag: has_flag(rider),
            });
        };
        drawn.extend(
            (0..session.ghosts.len())
                .filter(|&i| Some(i) != ghost)
                .map(ghost_bike),
        );
        // Following a merged replay makes the main replay's bikes ghosts too
        if ghost.is_some() {
            draw_riders(drawn);
        }
        let translucent = 0..drawn.len() - start;
        match ghost {
            // On top, drawn like any bike
            Some(i) => drawn.push(ghost_bike(i)),
            None => draw_riders(drawn),
        }

        // The timer stops when the followed replay ends, even if other bikes go on, and then
        // shows its time
        let timer = params.timer.then(|| {
            let flag_time = if rider == 0 {
                session.flag_time_a
            } else {
                session.flag_time_b
            };
            Timer {
                centiseconds: match ghost {
                    Some(g) => session.ghosts[g].timer_centiseconds(session.time),
                    None => session.main_timer_centiseconds(session.time),
                },
                best: params.best_time,
                flagtag: flagtag.then_some((flag_time * TIME_TO_CENTISECONDS) as i64),
            }
        });
        let last_apple = params
            .last_apple_time
            .then(|| session.apples_taken(followed(session, follow)))
            .and_then(|(apples, last)| match last {
                Some(last) if apples > 0 => Some((apples, (last * TIME_TO_CENTISECONDS) as i64)),
                _ => None,
            });

        SessionView {
            params: *params,
            follow,
            bikes: start..self.drawn.len(),
            translucent,
            objects,
            timer,
            last_apple,
            camera_edge: session.settings().game.camera_edge(),
        }
    }
}
