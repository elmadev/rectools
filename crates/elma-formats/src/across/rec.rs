// Across replays (RECORDER.CPP). Every field is stored for all frames before the next one,
// as floats in meters and radians, 60 frames per second.

use crate::Error;
use crate::reader::Reader;
use crate::rec::{Event, FLAG_FLIPPED, FLAG_GAS, Frame, Rec, Ride, WavEvent};
use std::f64::consts::TAU;

pub const FRAME_RATE: f64 = 60.0;
const VERSION: i32 = 120;
const MAGIC_NUMBER: i32 = 4796277;
const FRAME_SIZE: usize = 9 * 4 + 1 + 4 + 1 + 4 + 4;
const EVENT_SIZE: usize = 16;

// Where the head is drawn: above the bike, a little to the back (LEPTET.CPP: Kor12Fejr)
const HEAD_DISTANCE: f64 = 1.02;
const HEAD_BACK: f64 = 0.1;

#[derive(Debug, Clone, PartialEq)]
pub struct AcrossRec {
    pub rec: Rec,
    // The level's stamp, for a level from a file. The Windows port stores 1 when it couldn't
    // open the level, which is every time it was in the lev folder.
    pub stamp: u32,
    // The built-in level, an index into table::LEVELLISTA
    pub level: Option<i32>,
    // Recorded by Across 1.0 or 1.1: its events count the objects in the level's order
    pub old: bool,
}

struct Header {
    frames: usize,
    stamp: u32,
    level: Option<i32>,
    old: bool,
    size: usize,
}

fn read_header(r: &mut Reader) -> Result<Header, Error> {
    let frames = r.i32()?;
    if frames <= 0 {
        return r.invalid("replay has no frames");
    }
    let version = r.i32()?;
    let (stamp, level, old) = if version < 30 {
        // 1.0 and 1.1 had no version, only the level
        if !(0..=23).contains(&version) {
            return r.invalid("replay has an invalid level");
        }
        (0, Some(remap_old_level(version)), true)
    } else if version == VERSION {
        let stamp = r.u32()?;
        let level = r.i32()?;
        match (stamp, level) {
            (0, l) if (0..super::table::LEVEL_COUNT as i32).contains(&l) => (0, Some(l), false),
            (s, -1) if s != 0 => (s, None, false),
            _ => return r.invalid("replay has an invalid level"),
        }
    } else {
        return r.invalid("not an Across replay");
    };
    let header = if old { 8 } else { 16 };
    Ok(Header {
        frames: frames as usize,
        stamp,
        level,
        old,
        size: header,
    })
}

// The levels of 1.0 and 1.1 have moved (RECORDER.CPP: recorder::load)
fn remap_old_level(level: i32) -> i32 {
    match level {
        12 => 11,
        15 => 17,
        17 => 18,
        18 => 19,
        19 => 20,
        20 => 21,
        21 => 22,
        23 => 38,
        22 => 80,
        11 => 81,
        13 => 82,
        16 => 83,
        14 => 84,
        other => other,
    }
}

// Whether `data` is a whole Across replay: its size follows from the frame and event counts
pub fn is_across_rec(data: &[u8]) -> bool {
    let mut r = Reader::new(data);
    let Ok(header) = read_header(&mut r) else {
        return false;
    };
    let Some(events_at) = header
        .frames
        .checked_mul(FRAME_SIZE)
        .and_then(|frames| frames.checked_add(header.size))
    else {
        return false;
    };
    let Some(count) = data.get(events_at..).and_then(|rest| rest.get(..4)) else {
        return false;
    };
    let Ok(events) = usize::try_from(i32::from_le_bytes(count.try_into().unwrap())) else {
        return false;
    };
    events
        .checked_mul(EVENT_SIZE)
        .and_then(|events| events.checked_add(events_at + 8))
        .is_some_and(|size| size == data.len())
}

impl AcrossRec {
    pub fn parse(data: &[u8]) -> Result<AcrossRec, Error> {
        let mut r = Reader::new(data);
        let header = read_header(&mut r)?;
        let n = header.frames;
        r.has_items(n, FRAME_SIZE)?;
        let mut column = || -> Result<Vec<f32>, Error> { (0..n).map(|_| r.f32()).collect() };
        let floats: Vec<Vec<f32>> = (0..9).map(|_| column()).collect::<Result<_, _>>()?;
        let flipped = r.bytes(n)?.to_vec();
        let frequency: Vec<f32> = (0..n).map(|_| r.f32()).collect::<Result<_, _>>()?;
        let gas = r.bytes(n)?.to_vec();
        let _friction_speed: Vec<f32> = (0..n).map(|_| r.f32()).collect::<Result<_, _>>()?;
        let friction: Vec<f32> = (0..n).map(|_| r.f32()).collect::<Result<_, _>>()?;

        let [
            body_x,
            body_y,
            left_x,
            left_y,
            right_x,
            right_y,
            body_rot,
            left_rot,
            right_rot,
        ] = floats.try_into().unwrap();
        let frames = (0..n)
            .map(|i| {
                let flipped = flipped[i] != 0;
                let rotation = body_rot[i] as f64;
                let offset = |x: f32, y: f32| {
                    (
                        to_i16((x as f64 - body_x[i] as f64) * 1000.0),
                        to_i16((y as f64 - body_y[i] as f64) * 1000.0),
                    )
                };
                let (left_wheel_x, left_wheel_y) = offset(left_x[i], left_y[i]);
                let (right_wheel_x, right_wheel_y) = offset(right_x[i], right_y[i]);
                let (body_dx, body_dy) = elma_body_offset(rotation, flipped);
                let mut flags = 0;
                if gas[i] != 0 {
                    flags |= FLAG_GAS;
                }
                if flipped {
                    flags |= FLAG_FLIPPED;
                }
                Frame {
                    bike_x: body_x[i],
                    bike_y: body_y[i],
                    left_wheel_x,
                    left_wheel_y,
                    right_wheel_x,
                    right_wheel_y,
                    body_x: to_i16(body_dx * 1000.0),
                    body_y: to_i16(body_dy * 1000.0),
                    bike_rotation: (rotation.rem_euclid(TAU) * 10000.0 / TAU) as i16 % 10000,
                    left_wheel_rotation: wheel_rotation(left_rot[i]),
                    right_wheel_rotation: wheel_rotation(right_rot[i]),
                    flags,
                    motor_frequency: ((frequency[i] as f64 - 1.0) * 250.0)
                        .round()
                        .clamp(0.0, 255.0) as u8,
                    friction_volume: (friction[i] as f64 * 125.0).round().clamp(0.0, 255.0) as u8,
                }
            })
            .collect();

        let event_count = r.i32()?;
        if event_count < 0 {
            return r.invalid("replay has a negative event count");
        }
        r.has_items(event_count as usize, EVENT_SIZE)?;
        let mut events = Vec::with_capacity(event_count as usize);
        for _ in 0..event_count {
            let time = r.f64()?;
            let object_id = r.i16()?;
            // Across's sounds are numbered like Elma's
            let event_id = WavEvent::from_u8(r.u8()?);
            let _padding = r.u8()?;
            let volume = r.f32()?;
            events.push(Event {
                time,
                object_id,
                event_id,
                volume,
            });
        }
        if r.i32()? != MAGIC_NUMBER {
            return r.invalid("replay is corrupt (bad magic number)");
        }

        let level_filename = header
            .level
            .and_then(super::table::builtin)
            .map(|entry| entry.file.to_string())
            .unwrap_or_default();
        Ok(AcrossRec {
            rec: Rec {
                multiplayer: false,
                flagtag: false,
                // Built-in levels have no stamp: tell them apart by number
                level_id: match header.level {
                    Some(level) => -1 - level,
                    None => header.stamp as i32,
                },
                level_filename,
                rides: vec![Ride {
                    frames,
                    events,
                    frame_rate: FRAME_RATE,
                }],
            },
            stamp: header.stamp,
            level: header.level,
            old: header.old,
        })
    }

    // The left wheel at the first frame, which the game puts on the start object
    pub fn start(&self) -> (f64, f64) {
        let frame = &self.rec.rides[0].frames[0];
        (
            frame.bike_x as f64 + frame.left_wheel_x as f64 / 1000.0,
            frame.bike_y as f64 + frame.left_wheel_y as f64 / 1000.0,
        )
    }
}

fn to_i16(value: f64) -> i16 {
    value.round().clamp(i16::MIN as f64, i16::MAX as f64) as i16
}

fn wheel_rotation(radians: f32) -> u8 {
    ((radians as f64).rem_euclid(TAU) * 250.0 / TAU) as u8 % 250
}

// The body position that puts the Elma head (replay::head_position) where Across draws its
// head, relative to the bike
fn elma_body_offset(rotation: f64, flipped: bool) -> (f64, f64) {
    let back = if flipped { HEAD_BACK } else { -HEAD_BACK };
    let angle = rotation + std::f64::consts::FRAC_PI_2 + back;
    let head = (HEAD_DISTANCE * angle.cos(), HEAD_DISTANCE * angle.sin());
    let (i, j) = (
        (rotation.cos(), rotation.sin()),
        (-rotation.sin(), rotation.cos()),
    );
    let side = if flipped { 0.09 } else { -0.09 };
    (
        head.0 - i.0 * side - j.0 * 0.63,
        head.1 - i.1 * side - j.1 * 0.63,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(frames: usize, level: i32, events: &[(f64, i16, u8)]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend((frames as i32).to_le_bytes());
        out.extend(VERSION.to_le_bytes());
        out.extend(0u32.to_le_bytes());
        out.extend(level.to_le_bytes());
        let floats = [
            2.75f32, 3.6, 1.9, 3.0, 3.6, 3.0, 0.0, 1.0, 7.0, // one frame of each column
        ];
        for value in floats {
            for _ in 0..frames {
                out.extend(value.to_le_bytes());
            }
        }
        out.extend(vec![1u8; frames]);
        for _ in 0..frames {
            out.extend(1.5f32.to_le_bytes());
        }
        out.extend(vec![1u8; frames]);
        for _ in 0..2 * frames {
            out.extend(0.5f32.to_le_bytes());
        }
        out.extend((events.len() as i32).to_le_bytes());
        for &(time, object, wav) in events {
            out.extend(time.to_le_bytes());
            out.extend(object.to_le_bytes());
            out.push(wav);
            out.push(0);
            out.extend(0.25f32.to_le_bytes());
        }
        out.extend(MAGIC_NUMBER.to_le_bytes());
        out
    }

    #[test]
    fn reads_columns_into_elma_frames() {
        let data = build(2, 46, &[(0.5, -1, 6), (0.7, 3, 0)]);
        assert!(is_across_rec(&data));
        let rec = AcrossRec::parse(&data).unwrap();
        assert_eq!(rec.level, Some(46));
        assert_eq!(rec.rec.level_filename, "paly23.txt");
        let ride = &rec.rec.rides[0];
        assert_eq!(ride.frame_rate, FRAME_RATE);
        let frame = ride.frames[1];
        assert_eq!((frame.left_wheel_x, frame.left_wheel_y), (-850, -600));
        assert_eq!((frame.right_wheel_x, frame.right_wheel_y), (850, -600));
        assert_eq!(frame.flags, FLAG_GAS | FLAG_FLIPPED);
        assert_eq!(frame.motor_frequency, 125);
        assert_eq!(frame.left_wheel_rotation, 39);
        let start = rec.start();
        assert!((start.0 - 1.9).abs() < 1e-6 && (start.1 - 3.0).abs() < 1e-6);
        assert_eq!(ride.events[0].event_id, WavEvent::RightVolt);
        assert_eq!(ride.events[1].object_id, 3);
    }

    #[test]
    fn old_replays_name_their_level_in_place_of_the_version() {
        let mut data = build(1, 0, &[]);
        // 1.0 header: frames, level 21
        data.drain(4..16);
        data.splice(4..4, 21i32.to_le_bytes());
        assert!(is_across_rec(&data));
        let rec = AcrossRec::parse(&data).unwrap();
        assert!(rec.old);
        assert_eq!(rec.level, Some(22));
    }

    #[test]
    fn elma_replays_are_not_across() {
        let mut data = build(1, 0, &[]);
        data[4..8].copy_from_slice(&131i32.to_le_bytes());
        assert!(!is_across_rec(&data));
        assert!(!is_across_rec(&build(1, 0, &[])[..40]));
    }

    #[test]
    fn rejects_counts_larger_than_the_file() {
        let mut data = build(1, 0, &[]);
        data[..4].copy_from_slice(&i32::MAX.to_le_bytes());
        assert!(!is_across_rec(&data));
        // Where the frames would start
        assert_eq!(AcrossRec::parse(&data).err(), Some(Error::Eof { at: 16 }));

        let mut data = build(1, 0, &[]);
        let events_at = 16 + FRAME_SIZE;
        data[events_at..events_at + 4].copy_from_slice(&i32::MAX.to_le_bytes());
        assert!(!is_across_rec(&data));
        assert_eq!(
            AcrossRec::parse(&data).err(),
            Some(Error::Eof { at: events_at + 4 })
        );
    }

    #[test]
    fn body_offset_puts_the_elma_head_on_the_across_head() {
        for flipped in [false, true] {
            let rotation = 0.3;
            let (bx, by) = elma_body_offset(rotation, flipped);
            let (i, j) = (
                (rotation.cos(), rotation.sin()),
                (-rotation.sin(), rotation.cos()),
            );
            let side = if flipped { 0.09 } else { -0.09 };
            let head = (bx + i.0 * side + j.0 * 0.63, by + i.1 * side + j.1 * 0.63);
            let back = if flipped { 0.1 } else { -0.1 };
            let angle = rotation + std::f64::consts::FRAC_PI_2 + back;
            assert!((head.0 - 1.02 * angle.cos()).abs() < 1e-12);
            assert!((head.1 - 1.02 * angle.sin()).abs() < 1e-12);
        }
    }
}
