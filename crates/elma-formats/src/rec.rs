use crate::Error;
use crate::reader::Reader;

pub const VERSION: i32 = 131;
// Frames recorded per real second
pub const FRAME_RATE: f64 = 30.0;
const MAGIC_NUMBER: i32 = 4796277;
const FRAME_SIZE: usize = 27;
const EVENT_SIZE: usize = 16;

pub const FLAG_GAS: u8 = 1 << 0;
pub const FLAG_FLIPPED: u8 = 1 << 1;
pub const FLAG_FLAGTAG_A: u8 = 1 << 2;
pub const FLAG_FLAGTAG_IMMUNITY: u8 = 1 << 3;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Frame {
    pub bike_x: f32,
    pub bike_y: f32,
    pub left_wheel_x: i16,
    pub left_wheel_y: i16,
    pub right_wheel_x: i16,
    pub right_wheel_y: i16,
    pub body_x: i16,
    pub body_y: i16,
    // 0-9999
    pub bike_rotation: i16,
    // 0-249, glitched 0-255 when brake-stretching
    pub left_wheel_rotation: u8,
    pub right_wheel_rotation: u8,
    pub flags: u8,
    pub motor_frequency: u8,
    pub friction_volume: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WavEvent {
    None,
    Bump,
    Dead,
    Win,
    Food,
    Turn,
    RightVolt,
    LeftVolt,
    Unknown(u8),
}

impl WavEvent {
    pub fn from_u8(value: u8) -> Self {
        match value {
            0 => WavEvent::None,
            1 => WavEvent::Bump,
            2 => WavEvent::Dead,
            3 => WavEvent::Win,
            4 => WavEvent::Food,
            5 => WavEvent::Turn,
            6 => WavEvent::RightVolt,
            7 => WavEvent::LeftVolt,
            other => WavEvent::Unknown(other),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Event {
    pub time: f64,
    // The object touched, or negative for a sound event
    pub object_id: i16,
    pub event_id: WavEvent,
    pub volume: f32,
}

// The recording of one bike
#[derive(Debug, Clone, PartialEq)]
pub struct Ride {
    pub frames: Vec<Frame>,
    pub events: Vec<Event>,
    // Frames per real second: 30 in Elma, 60 in Across
    pub frame_rate: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Rec {
    pub multiplayer: bool,
    pub flagtag: bool,
    pub level_id: i32,
    pub level_filename: String,
    // One ride, or two for a multiplayer replay
    pub rides: Vec<Ride>,
}

impl Rec {
    pub fn parse(data: &[u8]) -> Result<Rec, Error> {
        let mut r = Reader::new(data);
        let first = read_ride(&mut r)?;
        let mut rec = Rec {
            multiplayer: first.multiplayer != 0,
            flagtag: first.flagtag != 0,
            level_id: first.level_id,
            level_filename: first.level_filename,
            rides: vec![first.ride],
        };
        if rec.multiplayer {
            let second = read_ride(&mut r)?;
            rec.rides.push(second.ride);
        }
        Ok(rec)
    }
}

struct RideBlock {
    multiplayer: i32,
    flagtag: i32,
    level_id: i32,
    level_filename: String,
    ride: Ride,
}

fn read_ride(r: &mut Reader) -> Result<RideBlock, Error> {
    let frame_count = r.i32()?;
    if frame_count <= 0 {
        return r.invalid("replay has no frames");
    }
    let version = r.i32()?;
    if version < VERSION {
        return r.invalid("replay version is too old");
    }
    if version > VERSION {
        return r.invalid("replay version is too new");
    }
    let multiplayer = r.i32()?;
    let flagtag = r.i32()?;
    let level_id = r.i32()?;
    let level_filename = r.cstr(16)?;

    let n = frame_count as usize;
    // Each field is stored for all frames before the next field
    r.has_items(n, FRAME_SIZE)?;
    let mut frames = vec![Frame::default(); n];
    macro_rules! read_field {
        ($field:ident, $read:ident) => {
            for frame in frames.iter_mut() {
                frame.$field = r.$read()?;
            }
        };
    }
    read_field!(bike_x, f32);
    read_field!(bike_y, f32);
    read_field!(left_wheel_x, i16);
    read_field!(left_wheel_y, i16);
    read_field!(right_wheel_x, i16);
    read_field!(right_wheel_y, i16);
    read_field!(body_x, i16);
    read_field!(body_y, i16);
    read_field!(bike_rotation, i16);
    read_field!(left_wheel_rotation, u8);
    read_field!(right_wheel_rotation, u8);
    read_field!(flags, u8);
    read_field!(motor_frequency, u8);
    read_field!(friction_volume, u8);

    let event_count = r.i32()?;
    if event_count < 0 {
        return r.invalid("replay has a negative event count");
    }
    r.has_items(event_count as usize, EVENT_SIZE)?;
    let mut events = Vec::with_capacity(event_count as usize);
    for _ in 0..event_count {
        let time = r.f64()?;
        let object_id = r.i16()?;
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

    Ok(RideBlock {
        multiplayer,
        flagtag,
        level_id,
        level_filename,
        ride: Ride {
            frames,
            events,
            frame_rate: FRAME_RATE,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(frames: &[Frame], events: &[Event], multiplayer: i32) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend((frames.len() as i32).to_le_bytes());
        out.extend(VERSION.to_le_bytes());
        out.extend(multiplayer.to_le_bytes());
        out.extend(0i32.to_le_bytes());
        out.extend(1234i32.to_le_bytes());
        let mut name = [0u8; 16];
        name[..10].copy_from_slice(b"QWQUU001.L");
        out.extend(name);
        for f in frames {
            out.extend(f.bike_x.to_le_bytes());
        }
        for f in frames {
            out.extend(f.bike_y.to_le_bytes());
        }
        for f in frames {
            out.extend(f.left_wheel_x.to_le_bytes());
        }
        for f in frames {
            out.extend(f.left_wheel_y.to_le_bytes());
        }
        for f in frames {
            out.extend(f.right_wheel_x.to_le_bytes());
        }
        for f in frames {
            out.extend(f.right_wheel_y.to_le_bytes());
        }
        for f in frames {
            out.extend(f.body_x.to_le_bytes());
        }
        for f in frames {
            out.extend(f.body_y.to_le_bytes());
        }
        for f in frames {
            out.extend(f.bike_rotation.to_le_bytes());
        }
        out.extend(frames.iter().map(|f| f.left_wheel_rotation));
        out.extend(frames.iter().map(|f| f.right_wheel_rotation));
        out.extend(frames.iter().map(|f| f.flags));
        out.extend(frames.iter().map(|f| f.motor_frequency));
        out.extend(frames.iter().map(|f| f.friction_volume));
        out.extend((events.len() as i32).to_le_bytes());
        for e in events {
            out.extend(e.time.to_le_bytes());
            out.extend(e.object_id.to_le_bytes());
            out.push(match e.event_id {
                WavEvent::Food => 4,
                WavEvent::RightVolt => 6,
                _ => 0,
            });
            out.push(0);
            out.extend(e.volume.to_le_bytes());
        }
        out.extend(MAGIC_NUMBER.to_le_bytes());
        out
    }

    #[test]
    fn parses_columns_and_events() {
        let frames = [
            Frame {
                bike_x: 1.5,
                bike_y: -2.0,
                bike_rotation: 9999,
                flags: FLAG_GAS,
                ..Default::default()
            },
            Frame {
                bike_x: 2.5,
                left_wheel_x: -850,
                friction_volume: 7,
                ..Default::default()
            },
        ];
        let events = [Event {
            time: 0.25,
            object_id: -1,
            event_id: WavEvent::RightVolt,
            volume: 0.5,
        }];
        let rec = Rec::parse(&build(&frames, &events, 0)).unwrap();
        assert_eq!(rec.level_id, 1234);
        assert_eq!(rec.level_filename, "QWQUU001.L");
        assert_eq!(rec.rides.len(), 1);
        assert_eq!(rec.rides[0].frames, frames);
        assert_eq!(rec.rides[0].events, events);
    }

    #[test]
    fn parses_both_multiplayer_rides() {
        let frames = [Frame::default()];
        let mut data = build(&frames, &[], 1);
        data.extend(build(&frames, &[], 1));
        let rec = Rec::parse(&data).unwrap();
        assert!(rec.multiplayer);
        assert_eq!(rec.rides.len(), 2);
    }

    #[test]
    fn rejects_truncated_files() {
        let data = build(&[Frame::default()], &[], 0);
        assert!(Rec::parse(&data[..data.len() - 1]).is_err());
    }

    #[test]
    fn errors_point_at_the_field() {
        let mut data = build(&[Frame::default()], &[], 0);
        data[4..8].copy_from_slice(&(VERSION + 1).to_le_bytes());
        assert_eq!(
            Rec::parse(&data),
            Err(Error::Invalid {
                message: "replay version is too new".into(),
                at: Some(4)
            })
        );
        assert_eq!(
            Rec::parse(&data).unwrap_err().to_string(),
            "replay version is too new (at byte 4)"
        );
    }

    #[test]
    fn rejects_counts_larger_than_the_file() {
        let mut data = build(&[Frame::default()], &[], 0);
        let events_at = 36 + FRAME_SIZE;
        data[events_at..events_at + 4].copy_from_slice(&i32::MAX.to_le_bytes());
        assert_eq!(Rec::parse(&data), Err(Error::Eof { at: events_at + 4 }));

        let mut data = build(&[Frame::default()], &[], 0);
        data[..4].copy_from_slice(&i32::MAX.to_le_bytes());
        // Where the frames would start
        assert_eq!(Rec::parse(&data), Err(Error::Eof { at: 36 }));
    }
}
