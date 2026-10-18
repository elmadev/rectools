import init, {
  Action,
  Ghost,
  Replay,
  Viewer,
  keyHelp,
  acrossRecOnLevel,
  levelLgr,
  recInfo,
} from "../wasm/rectools_web.js";

let ready: Promise<unknown> | undefined;

// Loads the wasm module once
export function loadEngine(): Promise<unknown> {
  ready ??= init();
  return ready;
}

// The colors of a decoded picture, like a site's png of a shirt: upright, or on its side
// like the game's bmp
export interface ShirtPixels {
  width: number;
  height: number;
  // RGBA, transparent where the bmp had its transparent color
  pixels: Uint8Array | Uint8ClampedArray;
}

// A bmp/<nick>.bmp file, or a decoded picture
export type ShirtInput = Uint8Array | ShirtPixels;

export interface GhostInput {
  rec: Uint8Array;
  shirt?: ShirtInput;
}

export interface ReplayInput {
  // Leave out to look around the level on its own
  rec?: Uint8Array;
  // Not needed for Across, which is drawn from `acrossPack`
  lgr?: Uint8Array;
  // The .lev file, not needed for replays of the internal levels
  lev?: Uint8Array;
  // elma.res, needed for the internal levels
  res?: Uint8Array;
  // The Across graphics and built-in levels (an across.res), for Across replays. With it and
  // no LGR, a level on its own is shown as an Across level.
  acrossPack?: Uint8Array;
  shirt?: ShirtInput;
  // Other replays of the level merged in, in this order
  ghosts?: GhostInput[];
}

export interface RecSummary {
  levelFilename: string;
  levelId: number;
  multiplayer: boolean;
  // The level is in elma.res, or one of Across's own
  internal: boolean;
  // An Across replay
  across: boolean;
}

// What a replay needs: its level file name, and whether it's in elma.res
export async function inspectRec(rec: Uint8Array): Promise<RecSummary> {
  await loadEngine();
  const info = recInfo(rec);
  const summary = {
    levelFilename: info.levelFilename,
    levelId: info.levelId,
    multiplayer: info.multiplayer,
    internal: info.internal,
    across: info.across,
  };
  info.free();
  return summary;
}

// The LGR name a level uses, without the extension
export async function inspectLevel(lev: Uint8Array): Promise<string> {
  await loadEngine();
  return levelLgr(lev);
}

// Whether an Across replay of a level from a file was recorded on this .lev
export async function isAcrossRecOf(rec: Uint8Array, lev: Uint8Array): Promise<boolean> {
  await loadEngine();
  return acrossRecOnLevel(rec, lev);
}

export function buildReplay(input: ReplayInput): Replay {
  const lgr = input.lgr ?? new Uint8Array();
  let replay: Replay;
  if (!input.rec) {
    if (!input.lev) throw new Error("a level is needed when there is no replay");
    replay = Replay.level(input.lev, lgr);
  } else {
    replay = new Replay(input.rec, lgr);
  }
  if (input.acrossPack) replay.setAcrossPack(input.acrossPack);
  if (!input.rec) return replay;
  if (input.lev) replay.setLevel(input.lev);
  if (input.res) replay.setRes(input.res);
  const setPixels = (index: number, shirt?: ShirtInput) => {
    if (shirt && !(shirt instanceof Uint8Array)) {
      const pixels = shirt.pixels instanceof Uint8Array ? shirt.pixels : new Uint8Array(shirt.pixels.buffer);
      replay.setShirtPixels(index, shirt.width, shirt.height, pixels);
    }
  };
  if (input.shirt instanceof Uint8Array) replay.setShirt(input.shirt);
  setPixels(0, input.shirt);
  (input.ghosts ?? []).forEach((ghost, i) => {
    replay.addGhost(ghost.rec, ghost.shirt instanceof Uint8Array ? ghost.shirt : undefined);
    setPixels(i + 1, ghost.shirt);
  });
  return replay;
}

// A replay to merge into the one a viewer shows, see Viewer.addGhost
export function buildGhost(input: GhostInput): Ghost {
  const ghost = new Ghost(input.rec);
  const shirt = input.shirt;
  if (shirt instanceof Uint8Array) ghost.setShirt(shirt);
  else if (shirt) {
    const pixels = shirt.pixels instanceof Uint8Array ? shirt.pixels : new Uint8Array(shirt.pixels.buffer);
    ghost.setShirtPixels(shirt.width, shirt.height, pixels);
  }
  return ghost;
}

export { Action, Viewer, keyHelp };
