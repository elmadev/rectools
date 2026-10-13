# Changelog

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
