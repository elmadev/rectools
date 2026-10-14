// The keyboard shortcuts, the same as the old recplayer's plus a few new ones. Keys are named
// like the browser's KeyboardEvent.key: the character typed, so shift is part of it ("N"),
// or the key's name ("ArrowLeft").

// What a key or a button does
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    TogglePlay,
    Slower,
    Faster,
    // Back to real time, keeping the direction
    NormalSpeed,
    Reverse,
    SeekBack,
    SeekForward,
    StepBack,
    StepForward,
    ZoomOut,
    ZoomIn,
    // The game's zoom, 1
    ResetZoom,
    FitLevel,
    // Back onto the bike, which also resumes auto zoom
    Recenter,
    FollowNext,
    FollowPrevious,
    // Pictures, and the level's own ground and sky
    TogglePictures,
    ToggleGrass,
    ToggleArrows,
    ToggleTimer,
    ToggleMinimap,
    ToggleLastAppleTime,
    // Grass, pictures and the level's own ground and sky on
    ShowEverything,
}

impl Action {
    // Whether holding the key down does it again and again. A toggle only flips once.
    pub fn repeats(self) -> bool {
        matches!(
            self,
            Action::Slower
                | Action::Faster
                | Action::SeekBack
                | Action::SeekForward
                | Action::StepBack
                | Action::StepForward
                | Action::ZoomOut
                | Action::ZoomIn
        )
    }
}

struct Binding {
    keys: &'static [&'static str],
    action: Action,
}

const BINDINGS: &[Binding] = &[
    Binding {
        keys: &[" "],
        action: Action::TogglePlay,
    },
    Binding {
        keys: &["-", "_"],
        action: Action::Slower,
    },
    Binding {
        keys: &["+", "="],
        action: Action::Faster,
    },
    Binding {
        keys: &["Backspace"],
        action: Action::NormalSpeed,
    },
    Binding {
        keys: &["r", "R"],
        action: Action::Reverse,
    },
    Binding {
        keys: &["ArrowLeft"],
        action: Action::SeekBack,
    },
    Binding {
        keys: &["ArrowRight"],
        action: Action::SeekForward,
    },
    Binding {
        keys: &[","],
        action: Action::StepBack,
    },
    Binding {
        keys: &["."],
        action: Action::StepForward,
    },
    Binding {
        keys: &["w", "W"],
        action: Action::ZoomOut,
    },
    Binding {
        keys: &["e", "E"],
        action: Action::ZoomIn,
    },
    Binding {
        keys: &["0"],
        action: Action::ResetZoom,
    },
    Binding {
        keys: &["f", "F"],
        action: Action::FitLevel,
    },
    Binding {
        keys: &["c", "C"],
        action: Action::Recenter,
    },
    Binding {
        keys: &["n"],
        action: Action::FollowNext,
    },
    Binding {
        keys: &["N"],
        action: Action::FollowPrevious,
    },
    Binding {
        keys: &["p", "P"],
        action: Action::TogglePictures,
    },
    Binding {
        keys: &["g"],
        action: Action::ToggleGrass,
    },
    Binding {
        keys: &["G"],
        action: Action::ShowEverything,
    },
    Binding {
        keys: &["a", "A"],
        action: Action::ToggleArrows,
    },
    Binding {
        keys: &["t", "T"],
        action: Action::ToggleTimer,
    },
    Binding {
        keys: &["v", "V"],
        action: Action::ToggleMinimap,
    },
    Binding {
        keys: &["x", "X"],
        action: Action::ToggleLastAppleTime,
    },
];

// What `key` does, if anything
pub fn action_for_key(key: &str) -> Option<Action> {
    BINDINGS
        .iter()
        .find(|b| b.keys.contains(&key))
        .map(|b| b.action)
}

// A line of the list of keys
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyHelp {
    pub keys: &'static str,
    pub description: &'static str,
    // Does something for a level without a replay
    pub level: bool,
    // Only for Elma: Across has no pictures, grass or gravity arrows
    pub elma_only: bool,
}

const fn help(keys: &'static str, description: &'static str) -> KeyHelp {
    KeyHelp {
        keys,
        description,
        level: false,
        elma_only: false,
    }
}

const fn level(keys: &'static str, description: &'static str, elma_only: bool) -> KeyHelp {
    KeyHelp {
        keys,
        description,
        level: true,
        elma_only,
    }
}

pub const KEYS: &[KeyHelp] = &[
    help("Space", "Play / pause"),
    help("- / + or =", "Slower / faster"),
    help("Backspace", "Normal speed"),
    help("R", "Reverse"),
    help("← / →", "Back / forward 2.5 seconds"),
    help(", / .", "Back / forward one frame"),
    level("W / E", "Zoom out / in", false),
    level("0", "Zoom 1.00x, the game's", false),
    level("F", "Show the whole level", false),
    help("C", "Back to the bike"),
    help("N / Shift N", "Follow the next / previous bike"),
    level("P", "Pictures, and the level's own ground and sky", true),
    level("G", "Grass", true),
    level("A", "Gravity arrows", true),
    help("T", "Timer"),
    help("V", "Minimap"),
    help("X", "Last apple time"),
    level(
        "Shift G",
        "Grass, pictures and the level's own ground and sky on",
        true,
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_named_like_the_browser_names_them() {
        assert_eq!(action_for_key(" "), Some(Action::TogglePlay));
        assert_eq!(action_for_key("ArrowLeft"), Some(Action::SeekBack));
        assert_eq!(action_for_key("n"), Some(Action::FollowNext));
        assert_eq!(action_for_key("N"), Some(Action::FollowPrevious));
        assert_eq!(action_for_key("g"), Some(Action::ToggleGrass));
        assert_eq!(action_for_key("G"), Some(Action::ShowEverything));
        assert_eq!(action_for_key("x"), Some(Action::ToggleLastAppleTime));
        assert_eq!(action_for_key("j"), None);
    }

    #[test]
    fn a_key_does_one_thing() {
        let mut keys: Vec<&str> = BINDINGS
            .iter()
            .flat_map(|b| b.keys.iter().copied())
            .collect();
        let count = keys.len();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), count);
    }
}
