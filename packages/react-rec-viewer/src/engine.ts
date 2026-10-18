import type * as Wasm from "../wasm/rectools_web.js";

export type { Viewer } from "../wasm/rectools_web.js";
// One of the player's actions, see Viewer.act
export type ActionName = keyof typeof Wasm.Action;

// What both builds export, the same from either
type Engine = Omit<typeof Wasm, "default" | "initSync">;

let ready: Promise<Engine> | undefined;
let loaded: Engine | undefined;

// The WebGPU build where the browser has a WebGPU adapter, else the WebGL2 one. They are built
// from the same code, and the WebGPU one is a fifth of the size.
async function load(): Promise<Engine> {
  const gpu = (navigator as { gpu?: { requestAdapter(): Promise<unknown> } }).gpu;
  if (gpu) {
    // Downloaded while the browser is asked for an adapter, which it nearly always has
    const webgpu = import("../wasm/rectools_web.js").then(async (m) => {
      await m.default();
      return m;
    });
    webgpu.catch(() => {});
    if (await gpu.requestAdapter().catch(() => null)) return webgpu;
  }
  const gl = await import("../wasm/rectools_web_gl.js");
  await gl.default();
  return gl;
}

// Loads the wasm module once
export function loadEngine(): Promise<unknown> {
  ready ??= load().then((engine) => (loaded = engine));
  return ready;
}

// The wasm module, once loadEngine has loaded it
function engine(): Engine {
  if (!loaded) throw new Error("the engine isn't loaded yet, see loadEngine");
  return loaded;
}

export function createViewer(canvas: HTMLCanvasElement): Promise<Wasm.Viewer> {
  return engine().Viewer.create(canvas);
}

export function action(name: ActionName): Wasm.Action {
  return engine().Action[name];
}

export function keyHelp(): Wasm.KeyHelp[] {
  return engine().keyHelp();
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
  const info = engine().recInfo(rec);
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
  return engine().levelLgr(lev);
}

// The LGR name a .lev file asks for, read where the file keeps it, so the LGR can be fetched
// while the engine still loads. Undefined for a file this doesn't read, which inspectLevel
// reads instead.
export function levelLgrName(lev: Uint8Array): string | undefined {
  // "POT14": the magic, a checksum, the level's id, four integrity values, then its name in
  // 51 bytes and the LGR's in 16. "POT06" levels all use the default LGR.
  const magic = String.fromCharCode(...lev.subarray(0, 5));
  if (magic === "POT06") return "default";
  if (magic !== "POT14" || lev.length < 110) return undefined;
  const field = lev.subarray(94, 110);
  const end = field.indexOf(0);
  return String.fromCharCode(...field.subarray(0, end < 0 ? field.length : end));
}

// Whether an Across replay of a level from a file was recorded on this .lev
export async function isAcrossRecOf(rec: Uint8Array, lev: Uint8Array): Promise<boolean> {
  await loadEngine();
  return engine().acrossRecOnLevel(rec, lev);
}

export function buildReplay(input: ReplayInput): Wasm.Replay {
  const { Replay } = engine();
  const lgr = input.lgr ?? new Uint8Array();
  let replay: Wasm.Replay;
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
export function buildGhost(input: GhostInput): Wasm.Ghost {
  const { Ghost } = engine();
  const ghost = new Ghost(input.rec);
  const shirt = input.shirt;
  if (shirt instanceof Uint8Array) ghost.setShirt(shirt);
  else if (shirt) {
    const pixels = shirt.pixels instanceof Uint8Array ? shirt.pixels : new Uint8Array(shirt.pixels.buffer);
    ghost.setShirtPixels(shirt.width, shirt.height, pixels);
  }
  return ghost;
}
