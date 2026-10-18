import { CSSProperties, forwardRef, useCallback, useEffect, useRef, useState } from "react";
import { ReplayInput } from "./engine.js";
import { RecViewerError, toRecError } from "./errors.js";
import { loadSources } from "./loaders.js";
import { RecViewer, RecViewerHandle, RecViewerProps } from "./RecViewer.js";

export interface RecPlayerProps
  extends Omit<RecViewerProps, "rec" | "lgr" | "lev" | "res" | "shirt" | "ghosts"> {
  // Replays to play together, the first is the main one. Leave out to show the level alone.
  recUrls?: string[];
  // Not needed for Across replays of its built-in levels
  levUrl?: string;
  // Shirt of each replay, by index
  shirtUrls?: (string | null | undefined)[];
  // Who drove each replay, by index, for the follow menu. Without one, the replay's file name.
  recNames?: (string | null | undefined)[];
  // Where the LGR a level asks for is, by its name without the extension
  lgrUrl?: (name: string) => string;
  // Use this LGR whatever the level asks for
  lgrFileUrl?: string;
  // Used when the level's LGR can't be loaded
  defaultLgrUrl?: string;
  // The Across pack (an across.res with its graphics and built-in levels), which Across
  // replays are drawn from
  acrossPackUrl?: string;
  // Show the level on its own as an Across level
  across?: boolean;
  // Passed to fetch, for credentials or headers
  fetchInit?: RequestInit;
}

const ELMA_ONLINE_LGR = (name: string) => `https://api.elma.online/api/lgr/get/${encodeURIComponent(name)}`;
const ELMA_ONLINE_DEFAULT_LGR = "https://api.elma.online/api/lgr/get/default";

const overlay: CSSProperties = {
  position: "absolute",
  inset: 0,
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
  padding: 16,
  textAlign: "center",
  font: "14px system-ui, sans-serif",
  color: "#eee",
  background: "rgba(0, 0, 0, 0.6)",
  pointerEvents: "none",
};

// At the top in the middle, between the timers
const errorBadge: CSSProperties = {
  position: "absolute",
  top: 8,
  left: "50%",
  transform: "translateX(-50%)",
  maxWidth: "50%",
  padding: "4px 10px",
  borderRadius: 4,
  font: "12px system-ui, sans-serif",
  color: "#eee",
  background: "rgba(120, 0, 0, 0.75)",
  pointerEvents: "none",
};

const badge: CSSProperties = {
  position: "absolute",
  top: 8,
  left: 8,
  padding: "2px 8px",
  borderRadius: 4,
  font: "12px system-ui, sans-serif",
  color: "#eee",
  background: "rgba(0, 0, 0, 0.6)",
  pointerEvents: "none",
};

// A replay's file name from its URL, without .rec, when the URL ends in one
function fileName(url: string): string | undefined {
  const last = decodeURIComponent(new URL(url, "http://x/").pathname.split("/").pop() ?? "");
  return /\.rec$/i.test(last) ? last.slice(0, -4) : undefined;
}

// One name per bike: the replay's, and for the second bike of a two player one the same with (2)
function bikeLabels(
  recUrls: string[],
  names: (string | null | undefined)[] | undefined,
  bikes: number[],
): (string | undefined)[] {
  return recUrls.flatMap((url, i) => {
    const name = names?.[i] || fileName(url);
    const count = bikes[i] ?? 1;
    return Array.from({ length: count }, (_, rider) => (name && rider > 0 ? `${name} (${rider + 1})` : name));
  });
}

// Plays replays from URLs: fetches the level, the LGR it asks for, the replays and their
// shirts, and shows a message while loading or when something is wrong
export const RecPlayer = forwardRef<RecViewerHandle, RecPlayerProps>(function RecPlayer(props, ref) {
  const {
    recUrls,
    levUrl,
    shirtUrls,
    recNames,
    lgrUrl = ELMA_ONLINE_LGR,
    lgrFileUrl,
    defaultLgrUrl = ELMA_ONLINE_DEFAULT_LGR,
    acrossPackUrl,
    across,
    fetchInit,
    onError,
    className,
    style,
    ...viewerProps
  } = props;

  const [input, setInput] = useState<ReplayInput | null>(null);
  const [bikesPerRec, setBikesPerRec] = useState<number[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const lgrUrlRef = useRef(lgrUrl);
  lgrUrlRef.current = lgrUrl;
  const initRef = useRef(fetchInit);
  initRef.current = fetchInit;
  const onErrorRef = useRef(onError);
  onErrorRef.current = onError;

  const reportError = useCallback((error: RecViewerError) => {
    if (onErrorRef.current) onErrorRef.current(error);
    else console.error(error);
  }, []);
  // The main replay of the input shown, so a download failing for another one doesn't
  // leave it on screen
  const shownMain = useRef<string | undefined>(undefined);

  // Reload only when a URL changes, not when the parent builds new arrays of the same ones
  const key = JSON.stringify([
    recUrls ?? [],
    levUrl ?? null,
    shirtUrls ?? [],
    lgrFileUrl ?? null,
    defaultLgrUrl,
    acrossPackUrl ?? null,
    across ?? false,
  ]);
  useEffect(() => {
    const [recs, lev, shirts, lgrFile, defaultLgr, acrossPack, acrossLevel] = JSON.parse(key);
    let current = true;
    setLoading(true);
    setError(null);
    loadSources({
      recUrls: recs,
      levUrl: lev ?? undefined,
      shirtUrls: shirts,
      lgrUrl: (name) => lgrUrlRef.current(name),
      lgrFileUrl: lgrFile ?? undefined,
      defaultLgrUrl: defaultLgr,
      acrossPackUrl: acrossPack ?? undefined,
      across: acrossLevel,
      init: initRef.current,
    })
      .then((loaded) => {
        if (!current) return;
        shownMain.current = recs[0];
        setInput(loaded.input);
        setBikesPerRec(loaded.bikes);
        setLoading(false);
      })
      .catch((e) => {
        if (!current) return;
        setLoading(false);
        // A replay being merged in that can't be had leaves the one playing alone
        if (recs[0] === undefined || recs[0] !== shownMain.current) {
          shownMain.current = undefined;
          setInput(null);
        }
        const error = toRecError(e);
        setError(error.message);
        reportError(error);
      });
    // A newer set of URLs wins over downloads still running for an older one
    return () => {
      current = false;
    };
  }, [key, reportError]);

  return (
    <div
      className={className}
      style={{ position: "relative", display: "flex", flexDirection: "column", background: "#000", ...style }}
    >
      {/* Shown while the files download too, so the viewer readies the GPU meanwhile; not after
          a failure, which would leave the replay before it on screen */}
      {input || (loading && !error) ? (
        <RecViewer
          ref={ref}
          bikeLabels={bikeLabels(recUrls ?? [], recNames, bikesPerRec)}
          {...viewerProps}
          {...input}
          onError={reportError}
          style={{ flex: 1, minHeight: 0 }}
        />
      ) : (
        <div style={{ flex: 1 }} />
      )}
      {error ? (
        // Over the replay that goes on when one being merged in couldn't be downloaded
        <div style={input ? errorBadge : overlay} role="alert">
          {input ? `Couldn't add: ${error}` : `Couldn't play this: ${error}`}
        </div>
      ) : (
        loading &&
        (input ? (
          // Replays being merged in: the one playing carries on meanwhile
          <div style={badge} role="status">
            Loading…
          </div>
        ) : (
          <div style={overlay} role="status">
            Loading…
          </div>
        ))
      )}
    </div>
  );
});
