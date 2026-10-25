# recview

Plays Elasto Mania and Across replays in a window, with sound, using the same renderer as
`rec2vid` and the web player.

## Building

```sh
cargo build --release -p recview
```

On macOS, `scripts/bundle-macos.sh --install` builds `recview.app` into `~/Applications`, so
replays open from Finder. Make it the default for `.rec` files with Cmd-I, "Open with",
"Change All…". It is signed for your own machine only.

## Usage

```sh
recview [replay.rec | level.lev] [options]
```

| Option | |
| --- | --- |
| `--data <dir>` | The game folder |
| `--merge <rec>` | Another replay of the level, drawn as a ghost. Repeatable. |
| `--nick <nick>` | Shirt from `bmp/<nick>.bmp` for the replay named before it |
| `--zoom <z>` | Starting zoom, 1 being the game's |
| `--no-audio` | Don't open the audio device |

Without a replay it opens an empty window: press O or drop files on it. Dropping several
plays the first with the others as ghosts. A replay of the level that is playing is added
as a ghost; another replaces it. A `.lev` shows the level on its own.

## Game folder

recview reads the game's files from its folder: `elma.res`, `lev/`, `lgr/`, `rec/` and
`bmp/` for Elma, and `across.res`, `lev/` and `rec/` for Across. Both can share one folder.
It takes `--data`, the folder the replay is in, or the one chosen last time, and otherwise
asks. When a replay's level is missing, it asks for the level file.

## Keys

The same as the web player's:

| Key | |
| --- | --- |
| Space, click | Play / pause |
| `-` / `+` or `=` | Slower / faster |
| Backspace | Normal speed |
| R | Reverse |
| ← / → | Back / forward 2.5 seconds |
| `,` / `.` | Back / forward one frame |
| W / E, wheel | Zoom out / in |
| 0 | Zoom 1.00x, the game's |
| Drag | Move the camera |
| F | Show the whole level |
| C | Back onto the bike |
| N / Shift N | Follow the next / previous bike |
| P | Pictures and the level's own ground and sky |
| G | Grass |
| A | Gravity arrows |
| Shift G | Grass, pictures and the level's own ground and sky on |
| T | Timer |
| V | Minimap |
| X | Last apple time |

And recview's own:

| Key | |
| --- | --- |
| K | Centered camera, or the game's classic one |
| S | Split screen for two player replays |
| B | Ghosts blue and see-through |
| Z | Auto zoom, to keep every bike in view |
| Y | Zoom textures: the ground, sky and qgrass grow with the zoom, or keep their size at zoom 1 |
| I | Last apple time in the large font |
| M | Sound |
| L | Loop |
| O | Open replays or a level |
| Shift ← / → | The previous / next replay or level in its folder |
| Shift O | Choose the game folder |
| Esc, Q | Quit |

The settings are remembered between launches, in `~/Library/Application Support/recview`
(`~/.config/recview` on Linux). The sound starts off. `fancyboost=false` in the `settings` file
leaves out the single color pictures of a fancyboosted LGR, which every LGR gets.
