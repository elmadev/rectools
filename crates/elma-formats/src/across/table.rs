// The built-in levels of Across 1.2. Replays name them by their place in LEVELLISTA, which
// points into LEVELTOMB. Both are as the game has them (across_w.exe, at 0x432f0 and 0x44a60;
// scripts/across-tables.py prints them): KORNY.CPP in the source differs from 42 on.

pub const LEVEL_COUNT: usize = 90;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    pub name: &'static str,
    pub file: &'static str,
    // Which of the level descriptors (leir1-3.lei) it is drawn with
    pub descriptor: u8,
}

const fn e(name: &'static str, file: &'static str, descriptor: u8) -> Entry {
    Entry {
        name,
        file,
        descriptor,
    }
}

pub const LEVELTOMB: [Entry; 74] = [
    e("Warm Up", "paly01.txt", 0),
    e("Flat Track", "paly02.txt", 0),
    e("Twin Peaks", "paly03.txt", 1),
    e("Over and Under", "paly04.txt", 0),
    e("Uphill Battle", "paly05.txt", 1),
    e("Long Haul", "paly06.txt", 0),
    e("Hi-Flyer", "paly07.txt", 0),
    e("Tunnel Terror", "paly08.txt", 1),
    e("The Steppes", "paly09.txt", 1),
    e("Islands in the Sky", "paly10.txt", 1),
    e("Hill Legend", "paly11.txt", 1),
    e("Fair Trial", "paly12.txt", 0),
    e("Loop-de-Loop", "paly13.txt", 0),
    e("Slippery Slope", "paly14.txt", 1),
    e("The Turnaround", "paly15.txt", 1),
    e("Hangman", "paly16.txt", 1),
    e("On Top", "paly17.txt", 1),
    e("Slalom", "paly18.txt", 1),
    e("Quick Round", "paly19.txt", 0),
    e("Ramp Frenzy", "paly20.txt", 0),
    e("Precarious", "paly21.txt", 1),
    e("Circuitous", "paly22.txt", 1),
    e("The Enigma", "paly23.txt", 0),
    e("Expert System", "paly24.txt", 0),
    e("The End", "paly25.txt", 1),
    e("CSTXT 01", "CS01.txt", 1),
    e("Hang Tight", "CS02.txt", 1),
    e("CSTXT 03", "CS03.txt", 1),
    e("Serpents Tale", "CS04.txt", 1),
    e("Fruit in the Den", "CS05.txt", 0),
    e("Shelf Life", "CS06.txt", 1),
    e("Apple Harvest", "CS07.txt", 1),
    e("Double Trouble", "CS08.txt", 1),
    e("Zig Zag", "CS09.txt", 0),
    e("Bounce Back", "CS10.txt", 0),
    e("Enduro", "CS11.txt", 0),
    e("Enigma II", "paly26.txt", 0),
    e("Turnaround II", "paly27.txt", 1),
    e("Ures hely", "nincs.txt", 1),
    e("Ures hely", "nincs.txt", 1),
    e("Tricks Abound", "proba01.lev", 1),
    e("CSLPRO 02", "proba02.lev", 1),
    e("Spiral", "proba03.lev", 0),
    e("CSLPRO 04", "proba04.lev", 1),
    e("New Wave", "proba05.lev", 1),
    e("CSLPRO 06", "proba06.lev", 1),
    e("CSLPRO 07", "proba07.lev", 1),
    e("Thorny Route", "proba08.lev", 1),
    e("Curvaceous", "proba09.lev", 1),
    e("Jaws", "proba10.lev", 0),
    e("Freefall", "proba11.lev", 1),
    e("He He", "proba12.lev", 1),
    e("CSLPRO 13", "proba13.lev", 1),
    e("Labyrinth Pro", "proba14.lev", 1),
    e("Animal Farm", "proba15.lev", 1),
    e("Framework", "proba16.lev", 1),
    e("Headbanger", "proba17.lev", 1),
    e("More Levels", "visit.lev", 1),
    e("CSL NINCS", "proba00.lev", 1),
    e("CSL NINCS", "proba00.lev", 1),
    e("CSL NINCS", "proba00.lev", 1),
    e("CSL NINCS", "proba00.lev", 1),
    e("CSL NINCS", "proba00.lev", 1),
    e("CSL NINCS", "proba00.lev", 1),
    e("CSL NINCS", "proba00.lev", 1),
    e("CSL NINCS", "proba00.lev", 1),
    e("CSL NINCS", "proba00.lev", 1),
    e("CSL NINCS", "proba00.lev", 1),
    e("CSL NINCS", "proba00.lev", 1),
    e("CSL NINCS", "proba00.lev", 1),
    e("Labyrinth", "bal01.lev", 1),
    e("CSL NINCS", "proba00.lev", 1),
    e("CSL NINCS", "proba00.lev", 1),
    e("CSL NINCS", "proba00.lev", 1),
];

pub const LEVELLISTA: [u8; LEVEL_COUNT] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, //
    10, 12, 28, 44, 70, 42, 37, 15, 17, 18, //
    19, 20, 21, 30, 34, 56, 54, 33, 53, 29, //
    49, 48, 32, 55, 35, 51, 50, 36, 23, 40, //
    26, 31, 11, 13, 14, 16, 22, 57, 39, 39, //
    39, 39, 39, 39, 39, 39, 39, 39, 39, 39, //
    39, 39, 39, 39, 39, 39, 39, 39, 39, 39, //
    39, 39, 39, 39, 39, 39, 39, 39, 39, 39, //
    22, 11, 13, 16, 14, 39, 39, 0, 0, 0,
];

// The descriptor of levels from files
pub const EXTERNAL_DESCRIPTOR: u8 = 2;

// The level a replay's built-in level number stands for
pub fn builtin(level: i32) -> Option<&'static Entry> {
    let index = *LEVELLISTA.get(usize::try_from(level).ok()?)?;
    let entry = &LEVELTOMB[index as usize];
    (entry.file != "nincs.txt" && entry.file != "proba00.lev").then_some(entry)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_replay_levels_like_the_game() {
        assert_eq!(builtin(0).unwrap().name, "Warm Up");
        assert_eq!(builtin(42).unwrap().name, "Fair Trial");
        assert_eq!(builtin(46).unwrap().file, "paly23.txt");
        assert_eq!(builtin(13).unwrap().file, "proba05.lev");
        assert_eq!(builtin(48), None);
        assert_eq!(builtin(-1), None);
        assert_eq!(builtin(90), None);
    }
}
