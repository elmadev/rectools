export class HttpError extends Error {
  constructor(
    public url: string,
    public status: number,
  ) {
    super(`HTTP ${status} loading ${url}`);
  }
}

// What couldn't be used:
// - rec: the main replay; ghost: a merged one, broken or of another level
// - lev, lgr, res (elma.res), across-pack: a file the replay is drawn from
// - level-changed: the level was edited after the replay was recorded
// - topology: the level has topology errors
// - graphics: WebGPU or WebGL2 couldn't draw
export type RecErrorKind =
  | "rec"
  | "ghost"
  | "lev"
  | "lgr"
  | "res"
  | "across-pack"
  | "level-changed"
  | "topology"
  | "graphics"
  | "other";

// Every error RecViewer and RecPlayer report. `status` is set when the file couldn't be
// downloaded, so a missing LGR (kind "lgr", status 404) is told apart from a broken one.
export class RecViewerError extends Error {
  constructor(
    message: string,
    public kind: RecErrorKind,
    public status?: number,
  ) {
    super(message);
    this.name = "RecViewerError";
  }
}

const KINDS: readonly string[] = [
  "rec",
  "ghost",
  "lev",
  "lgr",
  "res",
  "across-pack",
  "level-changed",
  "topology",
  "graphics",
  "other",
];

// `e` as a RecViewerError: the engine's errors carry their kind, others get `kind`
export function toRecError(e: unknown, kind: RecErrorKind = "other"): RecViewerError {
  if (e instanceof RecViewerError) return e;
  if (e instanceof HttpError) return new RecViewerError(e.message, kind, e.status);
  const message = e instanceof Error ? e.message : String(e);
  const own = typeof e === "object" && e !== null ? (e as { kind?: unknown }).kind : undefined;
  return new RecViewerError(message, typeof own === "string" && KINDS.includes(own) ? (own as RecErrorKind) : kind);
}

// Errors of downloading or reading one file, as that file's kind
export function asKind<T>(kind: RecErrorKind, promise: Promise<T>): Promise<T> {
  return promise.catch((e) => {
    throw toRecError(e, kind);
  });
}
