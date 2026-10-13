# rectools

Elasto Mania replay tools built around one GPU renderer, with playback and drawing ported
from [elma-classic](https://github.com/elmaonline/elma-classic). Replays of Across (Action
SuperCross) play too.

- `recview` plays replays in a window, with sound. See [docs/recview.md](docs/recview.md).
- `rec2vid` renders a replay to a video with ffmpeg (`rec2vid --help`).
- [`@elmadev/react-rec-viewer`](packages/react-rec-viewer) plays replays in the browser.

The game data (`elma.res`, `across.res`, LGR files, levels) is not included.

Windows and Linux builds of recview and rec2vid are artifacts of each CI run, and a
`v<version>` tag attaches them to a release.

## Crates

| Crate | |
| --- | --- |
| `elma-formats` | Reads the games' files: replays, levels, LGRs, `.res`, `.wav`, `.bmp` |
| `replay` | Plays a replay against its level |
| `render` | The wgpu renderer, native and on the web |
| `audio` | The game's sound mixer |
| `game-data` | Loads a replay and what it is drawn with from a game folder |
| `player` | Playback, the camera, the settings and the keys, shared by recview and the web player |
| `recview`, `rec2vid` | The apps |
| `web` | The wasm bindings the React package uses |
| `framediff` | Compares two videos frame by frame |

## License

Derived from elma-classic and released under the same dual license, see
[LICENSE.md](LICENSE.md).
