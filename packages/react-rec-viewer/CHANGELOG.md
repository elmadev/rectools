# Changelog

## Unreleased

- Grass and ground no longer shimmer when the camera moves on a scaled screen or when
  zoomed.
- The bike faces left at the start, like in the game.
- Fixed drawing errors on some phones.
- The sky is drawn more accurately when zoomed.
- The bike is drawn more like in the game.

## 0.1.7 - 2026-10-13

- The Across timer is in the one color Across draws it in, rather than Elma's.

## 0.1.6 - 2026-10-13

- Across replays drew only the timer in browsers without WebGPU, which use WebGL2.

## 0.1.5 - 2026-10-13

- Grass no longer flickers: grass pieces could leave a row of gaps that came and went
  as the camera moved, and on a scaled screen the browser stretched the picture slightly.
- In full screen the controls lie over the view and hide while playing when the mouse is
  still.
- Holding a key repeats only what isn't a toggle, so holding R no longer flips back and
  forth. The key list names `=` next to `+`.
- C after F goes back to the zoom from before F, unless zoomed since.

## 0.1.4 - 2026-10-13

- Renamed from `@rectools/react-rec-viewer`, with the repository moved to
  [elmadev/rectools](https://github.com/elmadev/rectools).

## 0.1.3 - 2026-10-13

- A click that closes the settings no longer plays or pauses.
- The seek bar is blue in dark mode too, rather than muted where it has played.

## 0.1.2 - 2026-10-13

- At the end of a replay the timer, `time()` and `duration()` show its real time, as
  elma.online does: when it finishes, which can be after the last frame, or the latest
  finish of a two player replay. They were a few hundredths short.

## 0.1.1 - 2026-10-13

- The WebAssembly is smaller: 2.8 MB instead of 4 MB, built with binaryen's `wasm-opt`.

## 0.1.0 - 2026-10-13

- First release.
