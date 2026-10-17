# @elmadev/react-rec-viewer

Plays Elasto Mania and Across replays, and shows levels, in React. It draws with WebGPU, or
WebGL2 where WebGPU isn't available, using rectools' renderer.

The renderer is a WebAssembly module of about 3 MB, loaded when the first player is shown.
Bundlers that understand `new URL("…", import.meta.url)`, such as Vite and webpack 5, need no
configuration.

## From URLs: `RecPlayer`

```jsx
import { RecPlayer } from "@elmadev/react-rec-viewer";

<RecPlayer
  levUrl="https://api.elma.online/dl/level/123"
  recUrls={[mainRecUrl, otherRecUrl]}
  shirtUrls={[mainShirtUrl, otherShirtUrl]}
  style={{ height: 480 }}
/>;
```

| Prop | |
| --- | --- |
| `levUrl` | The level |
| `recUrls` | The replays: the first plays, the others are merged in. Empty shows the level on its own. |
| `shirtUrls` | Each replay's shirt, a `.bmp` or a picture |
| `recNames` | Each replay's rider, for the follow menu |
| `lgrUrl` | `(name) => url` of the LGR a level asks for. Default: elma.online's. |
| `lgrFileUrl` | One LGR for every level |
| `defaultLgrUrl` | Used when the level's LGR can't be loaded |
| `acrossPackUrl` | The Across graphics and built-in levels, an `across.res` |
| `across` | Show a level on its own as an Across level |
| `fetchInit` | Passed to `fetch` |

Changing the merged replays while the first stays carries on from the same moment.
Downloads are cached by URL. Everything else is passed to `RecViewer`.

## From bytes: `RecViewer`

`RecViewer` takes the files as `Uint8Array`s: `rec`, `lev`, `lgr`, `res` (`elma.res`, for
the internal levels), `acrossPack`, `shirt`, and `ghosts: { rec, shirt }[]`.

Settings with a `default…` and an `on…Change` can be controlled or left to the player.

| Prop | Default | |
| --- | --- | --- |
| `zoom` | 1 | 1 is the game's zoom |
| `playing` | true | |
| `speed` | 1 | Negative plays backwards |
| `follow` | 0 | The bike followed: the replay's riders, then the merged ones' |
| `centeredCamera` | true | Off is the game's classic camera |
| `timer` | true | |
| `minimap` | false | |
| `lastAppleTime` | true | The game's "last apple" time in the bottom right (not for Across) |
| `autoZoom` | false | Zoom out to keep every bike in view. Moving the camera pauses it. |
| `translucentGhosts` | false | Merged replays blue and see-through (not on WebGL2) |
| `picturesInBackground` | true | Bikes and objects over every picture |
| `zoomTextures` | true | The ground, sky and grass textures grow with the zoom, like the pictures. Off keeps them at their size at zoom 1, like elma-classic's "Zoom Textures" off. |
| `levelOptions` | all on but `stillObjects` | `{ grass, pictures, customSky, arrows, animatedObjects, stillObjects, fancyboost }`. Fancyboost is the 256 single color pictures of a fancyboosted LGR, which every LGR gets. |
| `splitScreen` | true | Both bikes of a two player replay |
| `loop` | true | Start over after the end |
| `time` | | Seek here when it changes |
| `bikeLabels` | | Names for the follow menu |
| `controls`, `keyboard` | true | The control bar, and the keys below |
| `pauseWhenHidden`, `wakeLock` | true | Pause in a hidden tab; keep the screen on while playing |
| `autoPlaySetting`, `lgrChoices`, `lgrChoice` | | Settings for the page to keep, shown in the menu with their callbacks |
| `onTimeChange`, `onDurationChange`, `onEnd`, `onError`, `onSettingsOpen` | | |
| `showErrors` | true | Show errors over the view |

A ref gives `play()`, `pause()`, `seek(seconds)`, `time()`, `duration()`, `recenter()`,
`fitLevel()` and `toggleFullscreen()`.

## Errors

Errors are a `RecViewerError`, whose `kind` is `rec`, `ghost` (a merged replay), `lev`,
`lgr`, `res`, `across-pack`, `level-changed`, `topology`, `graphics` or `other`. A download
that failed has its HTTP `status`. A replay that can't be played clears the view; a merged
one that can't be added leaves the replay playing.

## Mouse, touch and keys

Drag to move the camera, wheel or pinch to zoom, tap to play or pause, double-click for
full screen.

| Key | |
| --- | --- |
| Space | Play / pause |
| `-` / `+` or `=` | Slower / faster |
| Backspace | Normal speed |
| R | Reverse |
| ← / → | Back / forward 2.5 seconds |
| `,` / `.` | Back / forward one frame |
| W / E | Zoom out / in |
| 0 | Zoom 1.00x, the game's |
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

`keyList()` returns them once the engine is loaded.

## Building

`pnpm build` at the repository root builds the wasm and `dist/`. It needs Rust with the
`wasm32-unknown-unknown` target and `wasm-bindgen-cli` at the version in `Cargo.lock`, and
uses binaryen's `wasm-opt` when it is installed.

To try a change in a site, `scripts/try-in-site.sh <site folder>` builds the package and copies
it over the one in the site's `node_modules`. `yarn install --check-files` in the site puts the
published one back.
