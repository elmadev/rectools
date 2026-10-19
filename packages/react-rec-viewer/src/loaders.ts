import {
  GhostInput,
  RecSummary,
  ReplayInput,
  ShirtInput,
  inspectLevel,
  inspectRec,
  levelLgrName,
  loadEngine,
} from "./engine.js";
import { HttpError, RecViewerError, asKind } from "./errors.js";

// Recent downloads, so switching replays of one level doesn't fetch the level and LGR again
const CACHE_SIZE = 40;
const cache = new Map<string, Promise<Uint8Array>>();

export { HttpError };

export function fetchBytes(url: string, init?: RequestInit): Promise<Uint8Array> {
  const cached = cache.get(url);
  if (cached) {
    // Most recently used last
    cache.delete(url);
    cache.set(url, cached);
    return cached;
  }
  const request = fetch(url, init).then(async (response) => {
    if (!response.ok) throw new HttpError(url, response.status);
    return new Uint8Array(await response.arrayBuffer());
  });
  cache.set(url, request);
  request.catch(() => {
    if (cache.get(url) === request) cache.delete(url);
  });
  while (cache.size > CACHE_SIZE) cache.delete(cache.keys().next().value!);
  return request;
}

export interface Sources {
  // Replays to merge, the first one is the main replay. None to show the level on its own.
  recUrls: string[];
  // Not needed for Across replays of its built-in levels
  levUrl?: string;
  // Shirt of each replay, by index. A missing or broken one gets the default body.
  shirtUrls: (string | null | undefined)[];
  // Where the LGR a level asks for is, by its name without the extension
  lgrUrl: (name: string) => string;
  // Use this LGR whatever the level asks for
  lgrFileUrl?: string;
  // Used when the level's LGR can't be loaded
  defaultLgrUrl: string;
  // The Across pack (an across.res), for Across replays
  acrossPackUrl?: string;
  // Show a level on its own as an Across one
  across?: boolean;
  init?: RequestInit;
}

type LgrSources = Pick<Sources, "lgrUrl" | "lgrFileUrl" | "defaultLgrUrl" | "init">;

async function loadLgr(sources: LgrSources, lev: Uint8Array): Promise<Uint8Array> {
  const get = (url: string) => fetchBytes(url, sources.init);
  try {
    // Read from the level as it is, without waiting for the engine to load
    const name = levelLgrName(lev) ?? (await inspectLevel(lev));
    const url = sources.lgrFileUrl ?? sources.lgrUrl(name);
    return await get(url);
  } catch (e) {
    try {
      return await get(sources.defaultLgrUrl);
    } catch {
      throw e;
    }
  }
}

// What preload starts on: the RecPlayer props it will be given, by the same names. What is
// left out isn't fetched.
export interface PreloadSources {
  recUrls?: string[];
  levUrl?: string;
  shirtUrls?: (string | null | undefined)[];
  // With RecPlayer's defaults
  lgrUrl?: (name: string) => string;
  lgrFileUrl?: string;
  defaultLgrUrl?: string;
  fetchInit?: RequestInit;
}

export const ELMA_ONLINE_LGR = (name: string) => `https://api.elma.online/api/lgr/get/${encodeURIComponent(name)}`;
export const ELMA_ONLINE_DEFAULT_LGR = "https://api.elma.online/api/lgr/get/default";

// Starts loading the engine and downloading what a RecPlayer given the same URLs needs: the
// replays, their shirts, the level and the LGR it asks for. The RecPlayer then finds them
// downloaded or on their way. For a page that knows the URLs before it can show the player,
// such as while it waits for its own data. Failures are left for the RecPlayer to report.
export function preload(sources: PreloadSources): void {
  const ignore = () => {};
  loadEngine().catch(ignore);
  const get = (url: string) => fetchBytes(url, sources.fetchInit);
  for (const url of [...(sources.recUrls ?? []), ...(sources.shirtUrls ?? [])]) {
    if (url) get(url).catch(ignore);
  }
  if (!sources.levUrl) return;
  const lgr: LgrSources = {
    lgrUrl: sources.lgrUrl ?? ELMA_ONLINE_LGR,
    lgrFileUrl: sources.lgrFileUrl,
    defaultLgrUrl: sources.defaultLgrUrl ?? ELMA_ONLINE_DEFAULT_LGR,
    init: sources.fetchInit,
  };
  get(sources.levUrl)
    .then((lev) => loadLgr(lgr, lev))
    .catch(ignore);
}

// A bmp as it is, anything else (sites serve shirts as png) decoded by the browser
export async function decodeShirt(data: Uint8Array): Promise<ShirtInput> {
  if (data[0] === 0x42 && data[1] === 0x4d) return data;
  const bitmap = await createImageBitmap(new Blob([data as Uint8Array<ArrayBuffer>]), {
    premultiplyAlpha: "none",
    colorSpaceConversion: "none",
  });
  const { width, height } = bitmap;
  const canvas =
    typeof OffscreenCanvas !== "undefined"
      ? new OffscreenCanvas(width, height)
      : Object.assign(document.createElement("canvas"), { width, height });
  const context = canvas.getContext("2d") as OffscreenCanvasRenderingContext2D | CanvasRenderingContext2D;
  context.drawImage(bitmap, 0, 0);
  bitmap.close();
  return { width, height, pixels: context.getImageData(0, 0, width, height).data };
}

// Each download decoded once, so loading its replays again gives the viewer the same shirt,
// and replays merged into them don't load the others again
const decoded = new WeakMap<Uint8Array, Promise<ShirtInput>>();
function decodeOnce(data: Uint8Array): Promise<ShirtInput> {
  let shirt = decoded.get(data);
  if (!shirt) {
    shirt = decodeShirt(data);
    decoded.set(data, shirt);
  }
  return shirt;
}

export interface LoadedSources {
  input: ReplayInput;
  // How many bikes each replay has, 2 for a two player one
  bikes: number[];
}

// An Elma level file, rather than an Across one (POT06)
function isElmaLevel(lev: Uint8Array): boolean {
  return String.fromCharCode(...lev.subarray(0, 5)) === "POT14";
}

// Everything a replay is drawn from, in the order given however the downloads finish.
// Fails with a RecViewerError naming the file that couldn't be had.
export async function loadSources(sources: Sources): Promise<LoadedSources> {
  const get = (url: string) => fetchBytes(url, sources.init);
  const shirt = (i: number) => {
    const url = sources.shirtUrls[i];
    return url
      ? get(url)
          .then(decodeOnce)
          .catch(() => undefined)
      : Promise.resolve(undefined);
  };
  // The level downloads meanwhile, though an Across replay may not need it
  const levUrl = sources.levUrl;
  const levPromise = asKind(
    "lev",
    levUrl ? get(levUrl) : Promise.reject(new Error("a level is needed for this replay")),
  );
  levPromise.catch(() => {});
  const recsPromise = Promise.all(sources.recUrls.map((url, i) => asKind(i === 0 ? "rec" : "ghost", get(url))));
  // A broken ghost is reported when the replays are put together
  const infosPromise = recsPromise.then((recs) =>
    Promise.all(recs.map((rec, i) => (i === 0 ? asKind("rec", inspectRec(rec)) : inspectRec(rec).catch(() => null)))),
  );
  // The shirts, and an Elma level's LGR, download without waiting for the replays to be read,
  // which needs the engine. Another level's LGR waits: it may be an Across level, whose
  // replays are drawn without one.
  const shirtsPromise = Promise.all(sources.recUrls.map((_, i) => shirt(i)));
  const earlyLgr = levPromise.then((lev) =>
    !sources.across && isElmaLevel(lev) ? asKind("lgr", loadLgr(sources, lev)) : undefined,
  );
  earlyLgr.catch(() => {});
  const first = await infosPromise.then((infos) => infos[0]);
  if (first?.across || (sources.across && sources.recUrls.length === 0)) {
    return loadAcross(sources, await recsPromise, first);
  }
  const [lev, lgr, recs, shirts, infos] = await Promise.all([
    levPromise,
    levPromise.then(async (lev) => (await earlyLgr) ?? asKind("lgr", loadLgr(sources, lev))),
    recsPromise,
    shirtsPromise,
    infosPromise,
  ]);
  const ghosts: GhostInput[] = recs.slice(1).map((rec, i) => ({ rec, shirt: shirts[i + 1] }));
  const bikes = infos.map((info) => (info?.multiplayer ? 2 : 1));
  return { input: { rec: recs[0], lev, lgr, shirt: shirts[0], ghosts }, bikes };
}

// An Across replay: drawn from the Across pack, with the level only for levels from files
async function loadAcross(
  sources: Sources,
  recs: Uint8Array[],
  first: RecSummary | null | undefined,
): Promise<LoadedSources> {
  const get = (url: string) => fetchBytes(url, sources.init);
  if (!sources.acrossPackUrl) throw new RecViewerError("Across replays need the Across pack", "across-pack");
  const needsLevel = !first?.internal;
  if (needsLevel && !sources.levUrl) throw new RecViewerError("a level is needed for this replay", "lev");
  const [acrossPack, lev] = await Promise.all([
    asKind("across-pack", get(sources.acrossPackUrl)),
    needsLevel ? asKind("lev", get(sources.levUrl!)) : Promise.resolve(undefined),
  ]);
  const ghosts: GhostInput[] = recs.slice(1).map((rec) => ({ rec }));
  return { input: { rec: recs[0], lev, acrossPack, ghosts }, bikes: recs.map(() => 1) };
}
