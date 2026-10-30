# Changelog

## Unreleased

- Level pages no longer briefly show the replay controls.
- The follow menu of a two player replay has Split screen, and keys 1 and 2 switch between it and one rider's view.
- Zooming with the wheel back from the smallest or largest zoom reaches 1x again.

## 0.4.0 - 2026-10-27

- Across replays can now also show the last apple time.
- Added a minimap size setting and `minimapSize` prop.
- Added a "Large font" setting and `largeFont` prop for the last apple time.
- In full screen, the last apple time and minimap no longer hide under the controls.
- The "Still objects" setting is hidden for levels shown without a replay.
- Holding the scrubber now pauses the replay.

## 0.3.3 - 2026-10-25

- Fixed the player freezing on Pixel 10 phones.

## 0.3.2 - 2026-10-20

- Rebuilt the engine, which in 0.3.1 had unfinished changes in it.

## 0.3.1 - 2026-10-20

- Fixed pictures and grass showing as stripes on some phones.

## 0.3.0 - 2026-10-19

- No more "Draw with an instance count of 0" warnings in the console.
- Slightly less work each frame while playing.
- Replays added to the end of `ghosts` join without reloading the replay.
- Following a ghost, the minimap shows the apples it has taken.
- The ground and sky take less of the GPU to draw.
- Levels with lots of grass or pictures draw much faster.
- Smaller download with WebGPU: 0.55 MB instead of 2.9 MB.
- `RecPlayer` loads the engine while it downloads the replay and level.
- Added `preload` to start loading a `RecPlayer` before it is shown.
- Shirts and the LGR start downloading sooner.

## 0.2.1 - 2026-10-17

- Fixed a black screen in Firefox on Linux.

## 0.2.0 - 2026-10-17

- The position slider spans the whole player.
- Grass and ground no longer shimmer or move while zooming.
- Faster drawing, most of all on high-resolution screens and when zoomed.
- A paused player does no work until something changes.
- Added a "Zoom textures" setting and `zoomTextures` prop.
- Pictures and objects are drawn like the game at any zoom.
- Replays, levels and setting changes load faster.
- A new `ghosts` array with the same replays no longer reloads the replay.

## 0.1.8 - 2026-10-14

- Grass and ground no longer shimmer on scaled screens or when zoomed.
- The bike faces left at the start, like in the game.
- Fixed drawing errors on some phones.
- The sky is drawn more accurately when zoomed.
- The bike is drawn more like in the game.
- Zoom buttons and the mouse wheel use the same steps.
- Settings stay inside the player.
- A tap in full screen shows the controls without pausing.
- No flicker while resizing.
- Objects move with the replay's time.
- Full screen fits iPhones.
- Thicker timer on high density screens.
- Shirt stays on the first player in two player replays.
- 0 zooms back to 1.00x.
- Last apple time drawn like the game.
- No minimap on a level's page.

## 0.1.7 - 2026-10-13

- The Across timer has the same color as in Across.

## 0.1.6 - 2026-10-13

- Fixed Across replays in browsers without WebGPU.

## 0.1.5 - 2026-10-13

- Grass no longer flickers.
- In full screen the controls hide while playing.
- Holding a key no longer switches settings back and forth.
- C after F goes back to the previous zoom.

## 0.1.4 - 2026-10-13

- Renamed from `@rectools/react-rec-viewer`.

## 0.1.3 - 2026-10-13

- Closing the settings no longer plays or pauses.
- The seek bar is blue in dark mode too.

## 0.1.2 - 2026-10-13

- The timer shows the right time at the end of a replay.

## 0.1.1 - 2026-10-13

- Smaller download: 2.8 MB instead of 4 MB.

## 0.1.0 - 2026-10-13

- First release.
