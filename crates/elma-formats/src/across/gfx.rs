// Across graphics: loose PCX files named by a level descriptor (leir*.lei, LEIRO.CPP) and an
// object file (t4.ttt, TARGY.CPP). Everything is drawn with the sky's palette.

use crate::lev::ObjectType;
use crate::pic::{Pic8, decode_pcx, pcx_palette};
use crate::{Error, invalid};

const DESCRIPTOR_MAGIC: i64 = 17322;
// The first ground texture is shrunk to this many pixels at 50 pixels per meter
const GROUND1_SIZE: f64 = 80.0;

#[derive(Debug, Clone)]
pub struct Picture {
    pub pic: Pic8,
    // The color at its corner (0, 0)
    pub transparency: u8,
}

impl Picture {
    fn new(pic: Pic8) -> Picture {
        let transparency = pic.get(0, 0);
        Picture { pic, transparency }
    }
}

#[derive(Debug, Clone)]
pub struct ObjectKind {
    pub kind: ObjectType,
    // Its dot on the minimap
    pub minimap_color: u8,
    pub radius: f64,
    // Frames per unit of game time
    pub frame_speed: f64,
    pub frames: Vec<Picture>,
}

#[derive(Debug, Clone)]
pub struct Bike {
    pub wheel: Picture,
    // The body, in five parts
    pub parts: [Picture; 5],
    pub fork: Picture,
    pub rear_arm: Picture,
    pub forearm: Picture,
    pub upper_arm: Picture,
    pub head: Picture,
}

#[derive(Debug, Clone)]
pub struct AcrossGraphics {
    pub palette: [u8; 768],
    pub sky: Pic8,
    pub clouds: Picture,
    // For descriptors 0, 1 and 2 (levels from files)
    pub grounds: [Ground; 3],
    pub bike: Bike,
    pub objects: Vec<ObjectKind>,
    // The timer's color, which the game draws it in whatever is behind it
    pub timer_color: u8,
    pub frame_color: u8,
}

#[derive(Debug, Clone)]
pub struct Ground {
    pub pic: Pic8,
    // Its size in meters
    pub size: (f64, f64),
}

fn lines(data: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(data)
        .lines()
        .map(str::to_string)
        .collect()
}

// The first word of a line, like sscanf("%s")
fn word(line: Option<&String>) -> Result<&str, Error> {
    line.and_then(|l| l.split_whitespace().next())
        .ok_or_else(|| Error::invalid("the file ends early"))
}

fn number(line: Option<&String>) -> Result<i64, Error> {
    let w = word(line)?;
    w.parse()
        .or_else(|_| invalid(format!("expected a number, not {w}")))
}

impl AcrossGraphics {
    // `get` finds a file by name, in any case
    pub fn load(get: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<AcrossGraphics, Error> {
        let file =
            |name: &str| get(name).ok_or_else(|| Error::invalid(format!("{name} is missing")));
        let pcx = |name: &str| decode_pcx(&file(name)?);

        let descriptor = lines(&file("leir1.lei")?);
        let mut d = descriptor.iter();
        let object_file = word(d.next())?.to_string();
        let sky_name = word(d.next())?.to_string();
        let _small_clouds = word(d.next())?;
        // The game shows the big clouds at high resolutions
        let clouds = word(d.next())?.to_string();
        let mut names = Vec::new();
        for _ in 0..11 {
            names.push(word(d.next())?.to_string());
        }
        let mut colors = Vec::new();
        for _ in 0..7 {
            colors.push(number(d.next())?);
        }
        if number(d.next())? != DESCRIPTOR_MAGIC {
            return invalid("level descriptor is corrupt");
        }

        let sky_data = file(&sky_name)?;
        let palette = pcx_palette(&sky_data)?;
        let sky = decode_pcx(&sky_data)?;
        let picture = |name: &str| pcx(name).map(Picture::new);

        let [
            wheel,
            a,
            b,
            c,
            d,
            e,
            fork,
            rear_arm,
            forearm,
            upper_arm,
            head,
        ] = names.try_into().unwrap();
        let mut parts = [
            picture(&a)?,
            picture(&b)?,
            picture(&c)?,
            picture(&d)?,
            picture(&e)?,
        ];
        // The body parts share the first one's transparent color
        let transparency = parts[0].transparency;
        for part in &mut parts[1..] {
            part.transparency = transparency;
        }
        let bike = Bike {
            wheel: picture(&wheel)?,
            parts,
            fork: picture(&fork)?,
            rear_arm: picture(&rear_arm)?,
            forearm: picture(&forearm)?,
            upper_arm: picture(&upper_arm)?,
            head: picture(&head)?,
        };

        let ground1 = pcx("4_fold1.pcx")?;
        let ground1_meters = GROUND1_SIZE / 50.0;
        let ground = |pic: Pic8| {
            // The others are shown a pixel per pixel at 100 pixels per meter (640x480)
            let size = (pic.width as f64 / 100.0, pic.height as f64 / 100.0);
            Ground { pic, size }
        };
        let grounds = [
            Ground {
                pic: ground1,
                size: (ground1_meters, ground1_meters),
            },
            ground(pcx("4_fold2.pcx")?),
            ground(pcx("4_fold3.pcx")?),
        ];

        Ok(AcrossGraphics {
            palette,
            sky,
            clouds: picture(&clouds)?,
            grounds,
            bike,
            objects: object_kinds(&file(&object_file)?, &pcx)?,
            timer_color: colors[4].clamp(0, 255) as u8,
            frame_color: colors[6].clamp(0, 255) as u8,
        })
    }

    // The type of each entry of the object file, which levels refer to
    pub fn kinds(&self) -> Vec<ObjectType> {
        self.objects.iter().map(|o| o.kind).collect()
    }
}

// Just the object types of an object file, for reading levels without the graphics
pub fn object_types(data: &[u8]) -> Result<Vec<ObjectType>, Error> {
    Ok(read_object_file(data)?
        .into_iter()
        .map(|o| o.kind)
        .collect())
}

struct ObjectEntry {
    kind: ObjectType,
    radius: f64,
    frame_speed: f64,
    minimap_color: u8,
    frames: Vec<String>,
}

fn read_object_file(data: &[u8]) -> Result<Vec<ObjectEntry>, Error> {
    let lines = lines(data);
    let mut l = lines.iter();
    let count = number(l.next())?;
    if !(0..=10).contains(&count) {
        return invalid("object file is corrupt");
    }
    let mut out = Vec::new();
    for i in 0..count {
        if number(l.next())? != i {
            return invalid("object file is corrupt");
        }
        let kind = match number(l.next())? {
            1 => ObjectType::Exit,
            2 => ObjectType::Food,
            3 => ObjectType::Killer,
            4 => ObjectType::Start,
            _ => return invalid("object file has an invalid type"),
        };
        let radius = number(l.next())? as f64 * 0.01;
        let _wheel_sensitive = number(l.next())?;
        let _head_sensitive = number(l.next())?;
        let frame_speed = number(l.next())? as f64;
        let minimap_color = number(l.next())?.clamp(0, 255) as u8;
        let frames = number(l.next())?;
        if !(1..=20).contains(&frames) {
            return invalid("object file has an invalid frame count");
        }
        let mut names = Vec::new();
        for _ in 0..frames {
            names.push(word(l.next())?.to_string());
        }
        out.push(ObjectEntry {
            kind,
            radius,
            frame_speed,
            minimap_color,
            frames: names,
        });
    }
    Ok(out)
}

fn object_kinds(
    data: &[u8],
    pcx: &dyn Fn(&str) -> Result<Pic8, Error>,
) -> Result<Vec<ObjectKind>, Error> {
    read_object_file(data)?
        .into_iter()
        .map(|o| {
            let frames = o
                .frames
                .iter()
                .map(|name| pcx(name).map(Picture::new))
                .collect::<Result<_, _>>()?;
            Ok(ObjectKind {
                kind: o.kind,
                minimap_color: o.minimap_color,
                radius: o.radius,
                frame_speed: o.frame_speed,
                frames,
            })
        })
        .collect()
}

// The files the graphics are made of, to pack them
pub fn file_names(get: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<Vec<String>, Error> {
    let descriptor = get("leir1.lei").ok_or_else(|| Error::invalid("leir1.lei is missing"))?;
    let lines = lines(&descriptor);
    let mut names: Vec<String> = vec![
        "leir1.lei".into(),
        "leir2.lei".into(),
        "leir3.lei".into(),
        "4_fold1.pcx".into(),
        "4_fold2.pcx".into(),
        "4_fold3.pcx".into(),
    ];
    for line in lines.iter().take(15) {
        names.push(word(Some(line))?.to_string());
    }
    let object_file = names[6].clone();
    let objects =
        get(&object_file).ok_or_else(|| Error::invalid(format!("{object_file} is missing")))?;
    for entry in read_object_file(&objects)? {
        names.extend(entry.frames);
    }
    let mut seen = std::collections::HashSet::new();
    names.retain(|n| seen.insert(n.to_ascii_lowercase()));
    Ok(names)
}
