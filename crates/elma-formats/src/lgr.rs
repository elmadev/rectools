use crate::pic::{Pic8, decode_pcx, decode_spr, pcx_palette};
use crate::reader::{Reader, cstr};
use crate::{Error, invalid};
use std::collections::HashMap;

const MAGIC_NUMBER: i32 = 187565543;
const PICLIST_VERSION: i32 = 1002;
pub const ANIM_WIDTH: i32 = 40;
pub const MAX_QFOOD: usize = 9;

// Sub-rectangles of q1bike.pcx / q2bike.pcx: x1, y1, x2, y2 (inclusive)
pub const BIKE_BOXES: [[i32; 4]; 4] = [
    [3, 36, 147, 184],
    [32, 183, 147, 297],
    [146, 141, 273, 264],
    [272, 181, 353, 244],
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Transparency {
    None,
    Palette0,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Transparency {
    fn index(self, pic: &Pic8) -> Option<u8> {
        match self {
            Transparency::None => None,
            Transparency::Palette0 => Some(0),
            Transparency::TopLeft => Some(pic.get(0, 0)),
            Transparency::TopRight => Some(pic.get(pic.width - 1, 0)),
            Transparency::BottomLeft => Some(pic.get(0, pic.height - 1)),
            Transparency::BottomRight => Some(pic.get(pic.width - 1, pic.height - 1)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ListType {
    Picture,
    Texture,
    Mask,
}

struct ListEntry {
    kind: Option<ListType>,
    transparency: Transparency,
}

// An image together with the size the game draws it at, in pixels at zoom 1.
// LGR13 files can store images at a higher resolution than they are drawn at.
#[derive(Debug, Clone)]
pub struct Image {
    pub pic: Pic8,
    pub width: usize,
    pub height: usize,
}

impl Image {
    fn with_target_height(pic: Pic8, target_height: i32) -> Image {
        let width = pic.resized_width(target_height);
        Image {
            width,
            height: target_height.max(1) as usize,
            pic,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Picture {
    pub name: String,
    pub image: Image,
    pub transparency: u8,
    // Fancyboost squares are generated, not stored in the file
    pub solid: Option<u8>,
}

#[derive(Debug, Clone)]
pub struct Mask {
    pub name: String,
    pub image: Image,
    pub transparency: Option<u8>,
}

#[derive(Debug, Clone)]
pub struct Texture {
    pub name: String,
    pub image: Image,
    pub is_qgrass: bool,
}

#[derive(Debug, Clone)]
pub struct Anim {
    // Square frames, transparency is the top left pixel of the sheet
    pub frames: Vec<Pic8>,
    pub transparency: u8,
}

impl Anim {
    fn new(sheet: &Pic8, name: &str, target_height: i32) -> Result<Anim, Error> {
        if target_height != ANIM_WIDTH {
            return invalid(format!(
                "object picture height must be {ANIM_WIDTH}: {name}"
            ));
        }
        let size = sheet.height;
        if !sheet.width.is_multiple_of(size) {
            return invalid(format!(
                "object picture width must be a multiple of its height: {name}"
            ));
        }
        let (count, size) = ((sheet.width / size) as i32, size as i32);
        let frames = (0..count)
            .map(|i| sheet.crop(i * size, 0, i * size + size - 1, size - 1))
            .collect();
        Ok(Anim {
            frames,
            transparency: sheet.get(0, 0),
        })
    }
}

#[derive(Debug, Clone)]
pub struct BikePics {
    // The four pieces of q1bike.pcx / q2bike.pcx, see BIKE_BOXES
    pub parts: [Pic8; 4],
    pub body: Pic8,
    pub thigh: Pic8,
    pub leg: Pic8,
    pub wheel: Pic8,
    pub susp1: Pic8,
    pub susp2: Pic8,
    pub forarm: Pic8,
    pub up_arm: Pic8,
    pub head: Pic8,
}

#[derive(Debug, Clone)]
pub struct GrassPiece {
    pub image: Image,
    pub is_up: bool,
    pub target_height: i32,
}

// The palette indexes the minimap is drawn in
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MinimapColors {
    pub ground: u8,
    pub sky: u8,
    pub border: u8,
    // The followed bike, and the others
    pub bike: u8,
    pub other_bike: u8,
    pub exit: u8,
    pub food: u8,
    pub killer: u8,
}

#[derive(Debug, Clone)]
pub struct Lgr {
    pub palette: [u8; 768],
    pub minimap: MinimapColors,
    pub pictures: Vec<Picture>,
    pub masks: Vec<Mask>,
    pub textures: Vec<Texture>,
    pub bikes: [BikePics; 2],
    pub flag: Pic8,
    pub killer: Anim,
    pub exit: Anim,
    pub food: Vec<Anim>,
    pub qframe: Pic8,
    pub grass: Vec<GrassPiece>,
    picture_index: HashMap<String, usize>,
    mask_index: HashMap<String, usize>,
    texture_index: HashMap<String, usize>,
}

impl Lgr {
    pub fn parse(data: &[u8]) -> Result<Lgr, Error> {
        let mut r = Reader::new(data);
        let magic = r.array::<5>()?;
        if &magic[..3] != b"LGR" {
            return r.invalid("not an LGR file");
        }
        let version = match &magic[3..] {
            b"12" => 12,
            b"13" => 13,
            _ => return r.invalid("LGR version is not supported"),
        };
        let pcx_count = r.i32()?;
        if !(10..=3500).contains(&pcx_count) {
            return r.invalid("corrupt LGR file");
        }
        let list = read_piclist(&mut r)?;

        let mut palette = None;
        let mut q1bike = None;
        let mut q2bike = None;
        let mut qcolors = None;
        let mut qframe = None;
        let mut flag = None;
        let mut killer = None;
        let mut exit = None;
        let mut food: [Option<Anim>; MAX_QFOOD] = Default::default();
        let mut bike_parts: [HashMap<&'static str, Pic8>; 2] = Default::default();
        let mut pictures = Vec::new();
        let mut masks = Vec::new();
        let mut textures = Vec::new();
        let mut grass = Vec::new();

        const BIKE_PARTS: [&str; 9] = [
            "body", "thigh", "leg", "wheel", "susp1", "susp2", "forarm", "up_arm", "head",
        ];

        for _ in 0..pcx_count {
            let filename = cstr(r.bytes(20)?);
            let mut target_height = -1i32;
            if version == 13 {
                let _target_width = r.i16()?;
                target_height = r.i16()? as i32;
            }
            let size = r.i32()?;
            if !(1..=10_000_000).contains(&size) {
                return r.invalid("corrupt LGR file");
            }
            let bytes = r.bytes(size as usize)?;

            // Fancyboost "zzXXXXXX.pcx" files are generated instead
            if filename.len() == 12 && filename.starts_with("zz") {
                continue;
            }

            let lower = filename.to_ascii_lowercase();
            let pic = if lower.ends_with(".spr") {
                decode_spr(bytes)?
            } else if lower.ends_with(".pcx") {
                decode_pcx(bytes)?
            } else {
                return r.invalid(format!("unknown file type in LGR: {filename}"));
            };
            if version == 12 {
                target_height = pic.height as i32;
            }

            match lower.as_str() {
                "q1bike.pcx" => {
                    palette = Some(pcx_palette(bytes)?);
                    q1bike = Some(pic);
                    continue;
                }
                "q2bike.pcx" => {
                    q2bike = Some(pic);
                    continue;
                }
                "qflag.pcx" => {
                    flag = Some(pic);
                    continue;
                }
                "qkiller.pcx" => {
                    killer = Some(Anim::new(&pic, &filename, target_height)?);
                    continue;
                }
                "qexit.pcx" => {
                    exit = Some(Anim::new(&pic, &filename, target_height)?);
                    continue;
                }
                "qframe.pcx" => {
                    qframe = Some(pic);
                    continue;
                }
                "qcolors.pcx" => {
                    qcolors = Some(pic);
                    continue;
                }
                _ => {}
            }

            if let Some(rest) = lower.strip_suffix(".pcx") {
                let bike = if rest.starts_with("q1") {
                    Some(0)
                } else if rest.starts_with("q2") {
                    Some(1)
                } else {
                    None
                };
                if let Some(bike) = bike
                    && let Some(part) = BIKE_PARTS.iter().find(|&&p| p == &rest[2..])
                {
                    bike_parts[bike].insert(part, pic);
                    continue;
                }
                if let Some(n) = rest.strip_prefix("qfood")
                    && let Ok(n) = n.parse::<usize>()
                    && (1..=MAX_QFOOD).contains(&n)
                {
                    food[n - 1] = Some(Anim::new(&pic, &filename, target_height)?);
                    continue;
                }
            }

            if lower.starts_with("qup_") || lower.starts_with("qdown_") {
                grass.push(GrassPiece {
                    image: Image::with_target_height(pic, target_height),
                    is_up: lower.starts_with("qup_"),
                    target_height,
                });
                continue;
            }

            let Some(dot) = filename.find('.') else {
                return r.invalid(format!("cannot find dot in name: {filename}"));
            };
            let name = filename[..dot].to_string();
            if name.len() > 8 {
                return r.invalid(format!("filename is too long in LGR file: {filename}"));
            }

            if name.eq_ignore_ascii_case("qgrass") {
                textures.push(Texture {
                    name: "qgrass".into(),
                    image: Image::with_target_height(pic.flipped_vertically(), target_height),
                    is_qgrass: true,
                });
                continue;
            }

            let Some(entry) = list.get(&name.to_ascii_lowercase()) else {
                return r.invalid(format!(
                    "there is no line in PICTURES.LST corresponding to picture: {name}"
                ));
            };
            match entry.kind {
                Some(ListType::Picture) => {
                    let Some(transparency) = entry.transparency.index(&pic) else {
                        return r
                            .invalid(format!("picture must be transparent in LGR file: {name}"));
                    };
                    pictures.push(Picture {
                        name,
                        image: Image::with_target_height(pic, target_height),
                        transparency,
                        solid: None,
                    });
                }
                Some(ListType::Texture) => textures.push(Texture {
                    name,
                    // Textures are stored bottom row first, like the game's flipped back buffer
                    image: Image::with_target_height(pic.flipped_vertically(), target_height),
                    is_qgrass: false,
                }),
                Some(ListType::Mask) => {
                    let transparency = entry.transparency.index(&pic);
                    masks.push(Mask {
                        name,
                        image: Image::with_target_height(pic, target_height),
                        transparency,
                    });
                }
                None => return r.invalid("corrupt LGR file"),
            }
        }

        if r.i32()? != MAGIC_NUMBER {
            return r.invalid("corrupt LGR file (bad magic number)");
        }

        pictures.extend(fancyboost());

        if textures.len() < 2 {
            return r.invalid("there must be at least two textures in the LGR file");
        }

        let missing = |name: &str| Error::invalid(format!("picture not found in LGR file: {name}"));
        let palette = palette.ok_or_else(|| missing("q1bike.pcx"))?;
        let q1bike = q1bike.ok_or_else(|| missing("q1bike.pcx"))?;
        let q2bike = q2bike.ok_or_else(|| missing("q2bike.pcx"))?;
        let mut bikes = Vec::new();
        for (i, (sheet, mut parts)) in [q1bike, q2bike].into_iter().zip(bike_parts).enumerate() {
            let mut take = |part: &str| {
                parts
                    .remove(part)
                    .ok_or_else(|| missing(&format!("q{}{part}.pcx", i + 1)))
            };
            let pieces = BIKE_BOXES.map(|[x1, y1, x2, y2]| sheet.crop(x1, y1, x2, y2));
            bikes.push(BikePics {
                parts: pieces,
                body: take("body")?,
                thigh: take("thigh")?,
                leg: take("leg")?,
                wheel: take("wheel")?,
                susp1: take("susp1")?,
                susp2: take("susp2")?,
                forarm: take("forarm")?,
                up_arm: take("up_arm")?,
                head: take("head")?,
            });
        }
        let bikes: [BikePics; 2] = bikes.try_into().unwrap();

        // A column of color swatches, 12 pixels apart
        let qcolors: Pic8 = qcolors.ok_or_else(|| missing("qcolors.pcx"))?;
        let swatch = |i: usize| {
            let y = 6 + i * 12;
            if qcolors.width > 6 && y < qcolors.height {
                Ok(qcolors.get(6, y))
            } else {
                r.invalid("qcolors.pcx is too small")
            }
        };
        let minimap = MinimapColors {
            ground: swatch(0)?,
            sky: swatch(1)?,
            border: swatch(2)?,
            bike: swatch(4)?,
            other_bike: swatch(5)?,
            exit: swatch(6)?,
            food: swatch(7)?,
            killer: swatch(8)?,
        };

        let food: Vec<Anim> = food.into_iter().map_while(|f| f).collect();
        if food.is_empty() {
            return Err(missing("qfood1.pcx"));
        }

        let by_name = |a: &str, b: &str| a.to_ascii_lowercase().cmp(&b.to_ascii_lowercase());
        pictures.sort_by(|a, b| by_name(&a.name, &b.name));
        masks.sort_by(|a, b| by_name(&a.name, &b.name));
        textures.sort_by(|a, b| by_name(&a.name, &b.name));

        let index = |names: Vec<&String>| -> Result<HashMap<String, usize>, Error> {
            let mut map = HashMap::new();
            for (i, name) in names.into_iter().enumerate() {
                if map.insert(name.to_ascii_lowercase(), i).is_some() {
                    return r.invalid(format!("name is duplicated in LGR file: {name}"));
                }
            }
            Ok(map)
        };
        let picture_index = index(pictures.iter().map(|p| &p.name).collect())?;
        let mask_index = index(masks.iter().map(|m| &m.name).collect())?;
        let texture_index = index(textures.iter().map(|t| &t.name).collect())?;

        Ok(Lgr {
            palette,
            minimap,
            pictures,
            masks,
            textures,
            bikes,
            flag: flag.ok_or_else(|| missing("qflag.pcx"))?,
            killer: killer.ok_or_else(|| missing("qkiller.pcx"))?,
            exit: exit.ok_or_else(|| missing("qexit.pcx"))?,
            food,
            qframe: qframe.ok_or_else(|| missing("qframe.pcx"))?,
            grass,
            picture_index,
            mask_index,
            texture_index,
        })
    }

    // An LGR of plain colored rectangles, made here so tests don't need the game's files.
    // Color i of its palette is gray i; the parts use the colors 1 to 15.
    pub fn stub() -> Lgr {
        let mut palette = [0u8; 768];
        for (i, c) in palette.chunks_mut(3).enumerate() {
            c.fill(i as u8);
        }
        let image = |pic: Pic8| Image {
            width: pic.width,
            height: pic.height,
            pic,
        };
        let texture = |name: &str, color| Texture {
            name: name.into(),
            image: image(Pic8::filled(64, 64, color)),
            is_qgrass: false,
        };
        let anim = |color| Anim {
            frames: vec![Pic8::filled(40, 40, color)],
            transparency: 0,
        };
        let bike = |color| BikePics {
            parts: BIKE_BOXES.map(|[x1, y1, x2, y2]| {
                Pic8::filled((x2 - x1 + 1) as usize, (y2 - y1 + 1) as usize, color)
            }),
            body: Pic8::filled(40, 40, color),
            thigh: Pic8::filled(40, 10, color),
            leg: Pic8::filled(40, 10, color),
            wheel: Pic8::filled(40, 40, color),
            susp1: Pic8::filled(40, 8, color),
            susp2: Pic8::filled(40, 8, color),
            forarm: Pic8::filled(30, 8, color),
            up_arm: Pic8::filled(30, 8, color),
            head: Pic8::filled(20, 20, color),
        };
        let textures = vec![texture("ground", 1), texture("sky", 2)];
        let index = |names: Vec<&String>| {
            names
                .into_iter()
                .enumerate()
                .map(|(i, n)| (n.to_ascii_lowercase(), i))
                .collect()
        };
        let texture_index = index(textures.iter().map(|t| &t.name).collect());
        let pictures: Vec<Picture> = fancyboost().collect();
        let picture_index = index(pictures.iter().map(|p| &p.name).collect());
        Lgr {
            palette,
            minimap: MinimapColors {
                ground: 3,
                sky: 4,
                border: 5,
                bike: 6,
                other_bike: 7,
                exit: 8,
                food: 9,
                killer: 10,
            },
            pictures,
            masks: Vec::new(),
            textures,
            bikes: [bike(11), bike(12)],
            flag: Pic8::filled(20, 20, 13),
            killer: anim(10),
            exit: anim(8),
            food: vec![anim(9)],
            qframe: Pic8::filled(8, 8, 14),
            grass: Vec::new(),
            picture_index,
            mask_index: HashMap::new(),
            texture_index,
        }
    }

    pub fn picture(&self, name: &str) -> Option<usize> {
        self.picture_index.get(&name.to_ascii_lowercase()).copied()
    }

    pub fn mask(&self, name: &str) -> Option<usize> {
        self.mask_index.get(&name.to_ascii_lowercase()).copied()
    }

    pub fn texture(&self, name: &str) -> Option<usize> {
        if name.is_empty() {
            return None;
        }
        self.texture_index.get(&name.to_ascii_lowercase()).copied()
    }

    pub fn has_grass(&self) -> bool {
        self.texture("qgrass").is_some() && self.grass.len() >= 2
    }

    // Each color of the palette maps to the brightest or darkest color, for the timer
    pub fn timer_palette_map(&self) -> [u8; 256] {
        timer_palette_map(&self.palette)
    }

    // The level's ground and sky texture, after the game's fallback rules
    pub fn default_textures(
        &self,
        foreground: &str,
        background: &str,
        default_ground: bool,
        default_sky: bool,
    ) -> (usize, usize) {
        let mut fg = if default_ground { "ground" } else { foreground }.to_string();
        let mut bg = if default_sky { "sky" } else { background }.to_string();
        if fg.eq_ignore_ascii_case(&bg) {
            if default_sky && !default_ground {
                fg.clear();
            } else {
                bg.clear();
            }
        }
        if self.texture(&fg).is_none() {
            fg.clear();
        }
        if self.texture(&bg).is_none() {
            bg.clear();
        }
        let pick = |exclude: &str| -> String {
            let mut qgrass_fallback = None;
            for t in &self.textures {
                if t.name.eq_ignore_ascii_case(exclude) {
                    continue;
                }
                if t.is_qgrass {
                    qgrass_fallback = Some(t.name.clone());
                    continue;
                }
                return t.name.clone();
            }
            qgrass_fallback.unwrap_or_default()
        };
        if fg.is_empty() {
            fg = pick(&bg);
        }
        if bg.is_empty() {
            bg = pick(&fg);
        }
        (
            self.texture(&fg).unwrap_or(0),
            self.texture(&bg).unwrap_or(1),
        )
    }
}

fn read_piclist(r: &mut Reader) -> Result<HashMap<String, ListEntry>, Error> {
    if r.i32()? != PICLIST_VERSION {
        return r.invalid("pictures.lst in the LGR file has an incorrect version");
    }
    let length = r.i32()?;
    if !(0..=3000).contains(&length) {
        return r.invalid("pictures.lst in the LGR file is corrupt");
    }
    let n = length as usize;
    let names: Vec<String> = (0..n).map(|_| r.cstr(10)).collect::<Result<_, _>>()?;
    let ints = |r: &mut Reader| -> Result<Vec<i32>, Error> { (0..n).map(|_| r.i32()).collect() };
    let types = ints(r)?;
    let _distances = ints(r)?;
    let _clippings = ints(r)?;
    let transparencies = ints(r)?;
    let mut map = HashMap::new();
    for i in 0..n {
        let kind = match types[i] {
            100 => Some(ListType::Picture),
            101 => Some(ListType::Texture),
            102 => Some(ListType::Mask),
            _ => None,
        };
        let transparency = match transparencies[i] {
            11 => Transparency::Palette0,
            12 => Transparency::TopLeft,
            13 => Transparency::TopRight,
            14 => Transparency::BottomLeft,
            15 => Transparency::BottomRight,
            _ => Transparency::None,
        };
        map.entry(names[i].to_ascii_lowercase())
            .or_insert(ListEntry { kind, transparency });
    }
    Ok(map)
}

// The single color pictures of a fancyboosted LGR, one per palette color, which every LGR gets
fn fancyboost() -> impl Iterator<Item = Picture> {
    FANCYBOOST_NAMES
        .iter()
        .enumerate()
        .map(|(index, name)| Picture {
            name: name.to_string(),
            image: Image {
                pic: Pic8::filled(1, 1, index as u8),
                width: 200,
                height: 200,
            },
            transparency: 0,
            solid: Some(index as u8),
        })
}

const FANCYBOOST_NAMES: [&str; 256] = [
    "zz000000", "zz783000", "zz200000", "zzB4C4AC", "zz9C0000", "zzA40000", "zz606038", "zz000808",
    "zz080008", "zz380000", "zz686068", "zz606868", "zz9C1808", "zz500848", "zzF47000", "zz500000",
    "zz9C2808", "zzB40000", "zz9C2800", "zz780000", "zz884010", "zz9C1008", "zz280800", "zz008008",
    "zz008000", "zzF4E410", "zz800000", "zz383030", "zz202030", "zz006000", "zzBCCCBC", "zz000030",
    "zz882800", "zz401808", "zzB4C4B4", "zz381000", "zz2870B4", "zzC46010", "zz000800", "zz88B4D4",
    "zz006800", "zz682800", "zz9C6808", "zz881800", "zz9C9C48", "zzDCE4DC", "zz006008", "zz600000",
    "zz808080", "zz888860", "zz788094", "zz484020", "zz00D418", "zz606060", "zz686060", "zz943000",
    "zz8830C4", "zz782000", "zz9C9C60", "zz889C50", "zz88A4CC", "zz003078", "zz005000", "zz309C30",
    "zz687060", "zz484848", "zz687070", "zzBCC4B4", "zzFC7000", "zz008018", "zz6094C4", "zz6094CC",
    "zz609494", "zz1060AC", "zzA44008", "zz94B4D4", "zzC40000", "zz003000", "zz940000", "zzAC0000",
    "zz883000", "zz002000", "zz401000", "zzB4BCBC", "zz008808", "zz486048", "zz00AC00", "zz9410C4",
    "zzF4D410", "zz783010", "zz942800", "zzC40008", "zz808094", "zz941000", "zz180800", "zz501800",
    "zz080850", "zz103818", "zzB4B4AC", "zz1860AC", "zz9C2088", "zz380800", "zz283840", "zz803008",
    "zz78A4CC", "zz78A470", "zz100000", "zzBCB440", "zzECBC38", "zz202020", "zzA4B4AC", "zz007808",
    "zz505050", "zz007800", "zz880000", "zz1860B4", "zz007810", "zz689CCC", "zz107000", "zz689CB4",
    "zzCCDCC4", "zz107010", "zz580800", "zz680008", "zz087000", "zz007008", "zz580000", "zz482800",
    "zz007000", "zz686860", "zz086000", "zz687878", "zzC40808", "zz384040", "zz004000", "zz946860",
    "zz384020", "zz6000A4", "zzE46008", "zz101008", "zz101010", "zz2068B4", "zz709CCC", "zzB49C30",
    "zz101810", "zz9410CC", "zz200008", "zz943808", "zz181048", "zz5088C4", "zz505858", "zz3878BC",
    "zz781000", "zz4080BC", "zz4880BC", "zz94BCD4", "zz308828", "zz3078BC", "zz782008", "zz780800",
    "zz505038", "zz480000", "zz404040", "zz606058", "zz881808", "zz102008", "zz280000", "zz485040",
    "zz283028", "zz107808", "zz3068BC", "zz001008", "zz5888C4", "zz9C0800", "zz608050", "zz787878",
    "zz4888C4", "zz381800", "zzB40808", "zz787848", "zz786020", "zz08B408", "zz4880C4", "zz884820",
    "zz007020", "zz1868B4", "zz581810", "zz604018", "zzBCCCB4", "zz888894", "zz3070B4", "zz181818",
    "zz103800", "zz78ACCC", "zz081018", "zz50D428", "zz68D4F4", "zz0060CC", "zz683010", "zz6060CC",
    "zz680000", "zz5088BC", "zzA49CA4", "zz940050", "zzBC2030", "zz0858AC", "zz207018", "zz384838",
    "zz0050AC", "zzFCA420", "zzDCA418", "zz106070", "zz0858B4", "zz201818", "zz88FC00", "zz2870BC",
    "zz788880", "zzECDC48", "zz202840", "zz783008", "zz68A4C4", "zzF4A478", "zzEC9C78", "zz784010",
    "zzBC1008", "zz601800", "zz281008", "zz4078BC", "zz001000", "zz40D418", "zz48E408", "zz3828D4",
    "zz20E428", "zz6894C4", "zz0058AC", "zz108010", "zzC4CCC4", "zz085010", "zzDCF4DC", "zzEC1030",
    "zz281000", "zz2868B4", "zz789CDC", "zz5810D4", "zz303050", "zz5894C4", "zzDC0000", "zzD4D4D4",
    "zz00089C", "zz0094C4", "zz585050", "zz48DC28", "zz1050AC", "zzE48060", "zzCC4018", "zzFCFCFC",
];

// Each color of the palette maps to the brightest or darkest color, for the timer
pub fn timer_palette_map(palette: &[u8; 768]) -> [u8; 256] {
    let sum =
        |i: usize| palette[i * 3] as i32 + palette[i * 3 + 1] as i32 + palette[i * 3 + 2] as i32;

    let mut brightest = (-1, 0);
    let mut darkest = (1000, 0);
    for i in 0..256 {
        let value = sum(i);
        if brightest.0 < value {
            brightest = (value, i);
        }
        if darkest.0 > value {
            darkest = (value, i);
        }
    }
    let mut map = [0u8; 256];
    for (i, entry) in map.iter_mut().enumerate() {
        let rgb = &palette[i * 3..i * 3 + 3];
        *entry = if rgb.iter().all(|&c| c < 80) {
            brightest.1 as u8
        } else {
            darkest.1 as u8
        };
    }
    map
}
