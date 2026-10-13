import { GhostInput, RecSummary, ReplayInput, ShirtInput, inspectLevel, inspectRec } from "./engine.js";
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

async function loadLgr(sources: Sources, lev: Uint8Array): Promise<Uint8Array> {
  const get = (url: string) => fetchBytes(url, sources.init);
  try {
    const url = sources.lgrFileUrl ?? sources.lgrUrl(await inspectLevel(lev));
    return await get(url);
  } catch (e) {
    try {
      return await get(sources.defaultLgrUrl);
    } catch {
      throw e;
    }
  }
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

export interface LoadedSources {
  input: ReplayInput;
  // How many bikes each replay has, 2 for a two player one
  bikes: number[];
}

// Everything a replay is drawn from, in the order given however the downloads finish.
// Fails with a RecViewerError naming the file that couldn't be had.
export async function loadSources(sources: Sources): Promise<LoadedSources> {
  const get = (url: string) => fetchBytes(url, sources.init);
  const shirt = (i: number) => {
    const url = sources.shirtUrls[i];
    return url
      ? get(url)
          .then(decodeShirt)
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
  const first = await infosPromise.then((infos) => infos[0]);
  if (first?.across || (sources.across && sources.recUrls.length === 0)) {
    return loadAcross(sources, await recsPromise, first);
  }
  const [lev, lgr, recs, shirts, infos] = await Promise.all([
    levPromise,
    levPromise.then((lev) => asKind("lgr", loadLgr(sources, lev))),
    recsPromise,
    Promise.all(sources.recUrls.map((_, i) => shirt(i))),
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
