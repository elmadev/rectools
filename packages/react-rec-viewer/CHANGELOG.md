# Changelog

## Unreleased

- No more "Draw with an instance count of 0" warnings in the console.
- Playing takes a little less work each frame: the time shown is changed in place.
- Replays added to the end of `ghosts` are merged into the replay playing, from the same
  moment with the same view, without loading it again.

## 0.2.1 - 2026-10-17

- Fixed a black screen in Firefox on Linux, which 0.2.0 brought: its WebGL2 has too few
  vertex attributes for the shader.

## 0.2.0 - 2026-10-17

- The position slider spans the whole player, above the buttons.
- Grass and ground no longer shimmer with auto zoom on, and stay in place while zooming.
- Each frame takes less work to draw, and a paused player does nothing until something
  changes.
- `zoomTextures`, and a setting for it: off keeps the ground, sky and grass textures at their
  size at zoom 1, like elma-classic's "Zoom Textures" off.
- Pictures and objects are drawn as the game draws them at any zoom, where they were a pixel
  or so off at zooms that aren't whole numbers.
- Frames take less of the GPU to draw, most on high-resolution screens and when zoomed.
- Replays and levels open faster, and so do changes to the grass, pictures, sky and fancyboost.
- `ghosts` given as a new array of the same replays, as `ghosts={[...]}` does on every render,
  no longer loads the replay again.

## 0.1.8 - 2026-10-14

- Grass and ground no longer shimmer when the camera moves on a scaled screen or when
  zoomed.
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
