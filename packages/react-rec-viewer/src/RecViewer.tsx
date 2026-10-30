import {
  CSSProperties,
  KeyboardEvent as ReactKeyboardEvent,
  forwardRef,
  useCallback,
  useEffect,
  useImperativeHandle,
  useRef,
  useState,
} from "react";
import { Controls } from "./Controls.js";
import {
  ActionName,
  GhostInput,
  ReplayInput,
  Viewer,
  action,
  buildGhost,
  buildReplay,
  createViewer,
  loadEngine,
} from "./engine.js";
import { RecViewerError, toRecError } from "./errors.js";
import { formatTime } from "./format.js";
import { attachGestures } from "./gestures.js";


export interface LevelOptions {
  grass?: boolean;
  pictures?: boolean;
  // The level's own sky and ground, instead of the LGR's default ones
  customSky?: boolean;
  // Arrows on the gravity apples
  arrows?: boolean;
  // Apples, killers and the exit turn through their frames
  animatedObjects?: boolean;
  // Apples and the exit stay put instead of bobbing up and down
  stillObjects?: boolean;
  // The single color pictures of a fancyboosted LGR, which every LGR gets
  fancyboost?: boolean;
}

export interface LgrChoice {
  value: string;
  label: string;
}

export interface RecViewerProps extends ReplayInput {
  // Zoom, 1 is the game's default. Leave unset to let the viewer handle it.
  zoom?: number;
  defaultZoom?: number;
  onZoomChange?: (zoom: number) => void;
  playing?: boolean;
  defaultPlaying?: boolean;
  onPlayingChange?: (playing: boolean) => void;
  // Playback speed, 1 is real time and negative plays backwards
  speed?: number;
  defaultSpeed?: number;
  onSpeedChange?: (speed: number) => void;
  // Bike to follow: the riders of `rec`, then those of the ghosts in order
  follow?: number;
  onFollowChange?: (bike: number) => void;
  // Names for the follow menu, in the same order. Each bike's finish time is added.
  bikeLabels?: (string | null | undefined)[];
  // Seek to this time in seconds whenever it changes
  time?: number;
  onTimeChange?: (seconds: number) => void;
  onDurationChange?: (seconds: number) => void;
  // The replay reached its end and stopped, when not looping
  onEnd?: () => void;
  // Every error is a RecViewerError, whose kind says what couldn't be used
  onError?: (error: RecViewerError) => void;
  // Show errors over the view, on unless set: what couldn't be played, or a badge when
  // merging replays failed and the one playing goes on
  showErrors?: boolean;
  // The bike in the middle of the view, on unless set; otherwise the game's classic camera
  centeredCamera?: boolean;
  defaultCenteredCamera?: boolean;
  onCenteredCameraChange?: (centered: boolean) => void;
  // Show both bikes of a two player replay
  splitScreen?: boolean;
  // The timer at the top right, on unless set
  timer?: boolean;
  defaultTimer?: boolean;
  onTimerChange?: (value: boolean) => void;
  // The map of the level around the bike in the bottom right corner, off unless set
  minimap?: boolean;
  defaultMinimap?: boolean;
  onMinimapChange?: (value: boolean) => void;
  // Times the game's size, from 1 to 3, like elma-classic's "Minimap Size": a bigger minimap
  // shows more of the level around the bike. 1 unless set.
  minimapSize?: number;
  defaultMinimapSize?: number;
  onMinimapSizeChange?: (value: number) => void;
  // The game's "last apple" time in the bottom right corner, on unless set. Across has none of
  // its own, so it is drawn as Elma's.
  lastAppleTime?: boolean;
  defaultLastAppleTime?: boolean;
  onLastAppleTimeChange?: (value: boolean) => void;
  // Draw bikes and objects over every picture, on unless set
  picturesInBackground?: boolean;
  defaultPicturesInBackground?: boolean;
  onPicturesInBackgroundChange?: (value: boolean) => void;
  // The ground, sky and grass textures grow with the zoom, like the pictures, on unless set;
  // off keeps them at their size at zoom 1, like elma-classic's "Zoom Textures" off
  zoomTextures?: boolean;
  defaultZoomTextures?: boolean;
  onZoomTexturesChange?: (value: boolean) => void;
  // The last apple time in the game's medium font, like elma-classic's "Large Font"
  largeFont?: boolean;
  defaultLargeFont?: boolean;
  onLargeFontChange?: (value: boolean) => void;
  // Draw ghosts blue and see-through
  translucentGhosts?: boolean;
  defaultTranslucentGhosts?: boolean;
  onTranslucentGhostsChange?: (value: boolean) => void;
  // Zoom out when needed so every merged bike stays in view, off unless set
  autoZoom?: boolean;
  defaultAutoZoom?: boolean;
  onAutoZoomChange?: (value: boolean) => void;
  levelOptions?: LevelOptions;
  // Settings the page keeps, shown in the settings menu when their callback is given: whether
  // replays start playing when they are opened, and an LGR to use for every level
  autoPlaySetting?: boolean;
  onAutoPlaySettingChange?: (value: boolean) => void;
  lgrChoices?: LgrChoice[];
  // The value of the chosen one, or null for each level's own LGR
  lgrChoice?: string | null;
  onLgrChoiceChange?: (value: string | null) => void;
  // The settings menu opened, for loading the LGR choices when they are needed
  onSettingsOpen?: () => void;
  // The level options were changed from the options menu or the keys
  onLevelOptionsChange?: (options: Required<LevelOptions>) => void;
  // Built-in play button, scrubber, speed, zoom and full screen
  controls?: boolean;
  // A level without a replay is coming, so the controls are a level's while nothing is shown
  // yet instead of a replay's
  levelOnly?: boolean;
  // Keyboard shortcuts while the player has focus, see KEYS
  keyboard?: boolean;
  // Start again when the replay ends (default: true)
  loop?: boolean;
  // Pause when the tab is hidden (default: true)
  pauseWhenHidden?: boolean;
  // Keep the screen on while playing (default: true)
  wakeLock?: boolean;
  className?: string;
  style?: CSSProperties;
}

export interface RecViewerHandle {
  play(): void;
  pause(): void;
  seek(seconds: number): void;
  duration(): number;
  time(): number;
  // Move the camera back onto the bike
  recenter(): void;
  fitLevel(): void;
  toggleFullscreen(): void;
}

const dpr = () => window.devicePixelRatio || 1;

const errorOverlay: CSSProperties = {
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

const pausedBadge: CSSProperties = {
  position: "absolute",
  top: 8,
  left: 8,
  display: "flex",
  alignItems: "center",
  gap: 8,
  padding: "4px 6px 4px 10px",
  borderRadius: 4,
  font: "12px system-ui, sans-serif",
  color: "#eee",
  background: "rgba(0, 0, 0, 0.6)",
};

const resumeButton: CSSProperties = {
  font: "inherit",
  color: "#fff",
  background: "rgba(255, 255, 255, 0.15)",
  border: 0,
  borderRadius: 3,
  padding: "2px 8px",
  cursor: "pointer",
};

// Sized by `inset`, as 100vh on a phone reaches under the browser's own bars
const fullStyle: CSSProperties = {
  position: "fixed",
  inset: 0,
  zIndex: 10000,
  background: "#000",
};

// What the player core shows at a moment, see readState
interface State {
  hasContent: boolean;
  levelOnly: boolean;
  multiplayer: boolean;
  across: boolean;
  duration: number;
  bikeCount: number;
  autoZoomPaused: boolean;
  playing: boolean;
  speed: number;
  zoom: number;
  follow: number;
  bothViews: boolean;
  centeredCamera: boolean;
  timer: boolean;
  minimap: boolean;
  minimapSize: number;
  lastAppleTime: boolean;
  autoZoom: boolean;
  translucentGhosts: boolean;
  picturesInBackground: boolean;
  zoomTextures: boolean;
  largeFont: boolean;
  levelOptions: Required<LevelOptions>;
}

function readState(viewer: Viewer): State {
  const s = viewer.state();
  const state: State = {
    hasContent: s.hasContent,
    levelOnly: s.levelOnly,
    multiplayer: s.multiplayer,
    across: s.across,
    duration: s.duration,
    bikeCount: s.bikeCount,
    autoZoomPaused: s.autoZoomPaused,
    playing: s.playing,
    speed: s.speed,
    zoom: s.zoom,
    follow: s.follow,
    bothViews: s.bothViews,
    centeredCamera: s.centeredCamera,
    timer: s.timer,
    minimap: s.minimap,
    minimapSize: s.minimapSize,
    lastAppleTime: s.lastAppleTime,
    autoZoom: s.autoZoom,
    translucentGhosts: s.translucentGhosts,
    picturesInBackground: s.picturesInBackground,
    zoomTextures: s.zoomTextures,
    largeFont: s.largeFont,
    levelOptions: {
      grass: s.grass,
      pictures: s.pictures,
      customSky: s.customSky,
      arrows: s.arrows,
      animatedObjects: s.animatedObjects,
      stillObjects: s.stillObjects,
      fancyboost: s.fancyboost,
    },
  };
  s.free();
  return state;
}

// The settings a prop can give, each with how the core takes it
type Setting =
  | "playing"
  | "speed"
  | "zoom"
  | "follow"
  | "centeredCamera"
  | "timer"
  | "minimap"
  | "minimapSize"
  | "lastAppleTime"
  | "autoZoom"
  | "translucentGhosts"
  | "picturesInBackground"
  | "zoomTextures"
  | "largeFont";

const APPLY: { [K in Setting]: (viewer: Viewer, value: State[K]) => void } = {
  playing: (v, x) => v.setPlaying(x),
  speed: (v, x) => v.setSpeed(x),
  zoom: (v, x) => v.setZoom(x),
  follow: (v, x) => v.setFollow(x),
  centeredCamera: (v, x) => v.setCenteredCamera(x),
  timer: (v, x) => v.setTimer(x),
  minimap: (v, x) => v.setMinimap(x),
  minimapSize: (v, x) => v.setMinimapSize(x),
  lastAppleTime: (v, x) => v.setLastAppleTime(x),
  autoZoom: (v, x) => v.setAutoZoom(x),
  translucentGhosts: (v, x) => v.setTranslucentGhosts(x),
  picturesInBackground: (v, x) => v.setPicturesInBackground(x),
  zoomTextures: (v, x) => v.setZoomTextures(x),
  largeFont: (v, x) => v.setLargeFont(x),
};

const applyLevelOptions = (viewer: Viewer, o: Required<LevelOptions>) => viewer.setLevelOptions(o);

const sameOptions = (a: Required<LevelOptions>, b: Required<LevelOptions>) =>
  (Object.keys(a) as (keyof LevelOptions)[]).every((k) => a[k] === b[k]);

// The same replays and shirts in the same order, though perhaps in a new array
const sameGhosts = (a: GhostInput[] | undefined, b: GhostInput[] | undefined) =>
  a === b ||
  ((a?.length ?? 0) === (b?.length ?? 0) &&
    (a ?? []).every((g, i) => g.rec === b?.[i].rec && g.shirt === b?.[i].shirt));

// The ghosts `next` has after those of `shown`, when that's all that changed, so they can be
// merged into the replay shown. None when nothing changed, undefined when anything else did.
const addedGhosts = (shown: ReplayInput, next: ReplayInput): GhostInput[] | undefined => {
  const files = ["rec", "lgr", "lev", "res", "acrossPack", "shirt"] as const;
  if (!next.rec || files.some((k) => shown[k] !== next[k])) return undefined;
  const before = shown.ghosts ?? [];
  const after = next.ghosts ?? [];
  if (after.length < before.length || !sameGhosts(before, after.slice(0, before.length))) return undefined;
  return after.slice(before.length);
};

const sameState = (a: State, b: State) =>
  (Object.keys(a) as (keyof State)[]).every((k) => k === "levelOptions" || a[k] === b[k]) &&
  sameOptions(a.levelOptions, b.levelOptions);

export const RecViewer = forwardRef<RecViewerHandle, RecViewerProps>(function RecViewer(props, ref) {
  const {
    rec,
    lgr,
    lev,
    res,
    acrossPack,
    shirt,
    ghosts: givenGhosts,
    defaultZoom = 1,
    bikeLabels,
    time,
    showErrors,
    splitScreen = true,
    autoPlaySetting,
    onAutoPlaySettingChange,
    lgrChoices,
    lgrChoice,
    onLgrChoiceChange,
    onSettingsOpen,
    levelOptions,
    controls = true,
    keyboard = true,
    levelOnly: levelOnlyComing,
    loop = true,
    pauseWhenHidden = true,
    wakeLock = true,
    className,
    style,
  } = props;

  // The value of each setting the props give, or undefined where the core keeps its own
  const given: { [K in Setting]: State[K] | undefined } = {
    playing: props.playing,
    speed: props.speed,
    zoom: props.zoom,
    follow: props.follow,
    centeredCamera: props.centeredCamera,
    timer: props.timer,
    minimap: props.minimap,
    minimapSize: props.minimapSize,
    lastAppleTime: props.lastAppleTime,
    autoZoom: props.autoZoom,
    translucentGhosts: props.translucentGhosts,
    picturesInBackground: props.picturesInBackground,
    zoomTextures: props.zoomTextures,
    largeFont: props.largeFont,
  };
  // What each starts as when the props don't give it
  const defaults: { [K in Setting]: State[K] } = {
    playing: props.defaultPlaying ?? true,
    speed: props.defaultSpeed ?? 1,
    zoom: defaultZoom,
    follow: 0,
    centeredCamera: props.defaultCenteredCamera ?? true,
    timer: props.defaultTimer ?? true,
    minimap: props.defaultMinimap ?? false,
    minimapSize: props.defaultMinimapSize ?? 1,
    lastAppleTime: props.defaultLastAppleTime ?? true,
    autoZoom: props.defaultAutoZoom ?? false,
    translucentGhosts: props.defaultTranslucentGhosts ?? false,
    picturesInBackground: props.defaultPicturesInBackground ?? true,
    zoomTextures: props.defaultZoomTextures ?? true,
    largeFont: props.defaultLargeFont ?? false,
  };
  // Who hears about a change of each
  const changed = {
    playing: props.onPlayingChange,
    speed: props.onSpeedChange,
    zoom: props.onZoomChange,
    follow: props.onFollowChange,
    centeredCamera: props.onCenteredCameraChange,
    timer: props.onTimerChange,
    minimap: props.onMinimapChange,
    minimapSize: props.onMinimapSizeChange,
    lastAppleTime: props.onLastAppleTimeChange,
    autoZoom: props.onAutoZoomChange,
    translucentGhosts: props.onTranslucentGhostsChange,
    picturesInBackground: props.onPicturesInBackgroundChange,
    zoomTextures: props.onZoomTexturesChange,
    largeFont: props.onLargeFontChange,
    levelOptions: props.onLevelOptionsChange,
  } as { [K in Setting | "levelOptions"]?: (value: State[K]) => void };
  // A page's `ghosts={[...]}` is a new array each render, which would load the replay again
  const ghostsRef = useRef(givenGhosts);
  if (!sameGhosts(ghostsRef.current, givenGhosts)) ghostsRef.current = givenGhosts;
  const ghosts = ghostsRef.current;
  const callbacks = useRef({ changed, props });
  callbacks.current = { changed, props };

  const containerRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const timeRef = useRef<HTMLSpanElement>(null);
  const scrubRef = useRef<HTMLInputElement>(null);
  const scrubbing = useRef(false);
  // Whether the settings were open when the current press began: that press only closes them
  const settingsOpen = useRef(false);
  const pressClosesSettings = useRef(false);
  // A touch while the controls are hidden only shows them, as a mouse moving does
  const pressShowsControls = useRef(false);
  const viewerRef = useRef<Viewer | null>(null);
  // Something changed that a paused player has to draw
  const dirty = useRef(true);
  // Starts the render loop again, which stops while there is nothing to draw
  const wakeLoop = useRef<() => void>(() => {});
  const invalidate = useCallback(() => {
    dirty.current = true;
    wakeLoop.current();
  }, []);
  // Scrolled into view, so worth drawing
  const onScreen = useRef(true);

  // The core keeps the settings, playback and camera; this is what it showed last
  const [state, setState] = useState<State | null>(null);
  const stateRef = useRef<State | null>(null);
  const [viewerReady, setViewerReady] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [finishTimes, setFinishTimes] = useState<(number | undefined)[]>([]);
  const [canTranslucent, setCanTranslucent] = useState(true);
  const [nativeFullscreen, setNativeFullscreen] = useState(false);
  const [cssFullscreen, setCssFullscreen] = useState(false);
  const fullscreen = nativeFullscreen || cssFullscreen;

  // Reads what the core has now, and tells the page what changed. Everything that changes
  // the core calls this after: the keys, the buttons, the menu, the props and the end.
  const sync = useCallback(() => {
    const viewer = viewerRef.current;
    if (!viewer) return;
    const next = readState(viewer);
    const prev = stateRef.current;
    invalidate();
    // A pan or a zoom by the wheel changes nothing shown outside the canvas
    if (prev && sameState(next, prev)) return;
    stateRef.current = next;
    setState(next);
    if (!prev) return;
    const on = callbacks.current.changed;
    for (const key of Object.keys(APPLY) as Setting[]) {
      if (next[key] !== prev[key]) (on[key] as ((v: unknown) => void) | undefined)?.(next[key]);
    }
    if (!sameOptions(next.levelOptions, prev.levelOptions)) on.levelOptions?.(next.levelOptions);
  }, [invalidate]);

  // Runs `change` on the core, then reports it
  const change = useCallback(
    (f: (viewer: Viewer) => void) => {
      const viewer = viewerRef.current;
      if (!viewer) return;
      f(viewer);
      sync();
    },
    [sync],
  );
  const act = useCallback((name: ActionName) => change((v) => v.act(action(name))), [change]);
  const togglePlay = useCallback(() => act("TogglePlay"), [act]);
  const fitLevel = useCallback(() => act("FitLevel"), [act]);
  const recenter = useCallback(() => act("Recenter"), [act]);
  const seek = useCallback((seconds: number) => change((v) => v.seek(seconds)), [change]);
  const setPlaying = useCallback((value: boolean) => change((v) => v.setPlaying(value)), [change]);
  // Playing stops while the scrubber is held, so the bike stays where it is put, and goes on
  // once it is let go
  const resumeAfterScrub = useRef(false);
  const scrub = useCallback(
    (held: boolean) => {
      if (held === scrubbing.current) return;
      scrubbing.current = held;
      if (held) {
        resumeAfterScrub.current = stateRef.current?.playing ?? false;
        if (resumeAfterScrub.current) setPlaying(false);
      } else if (resumeAfterScrub.current) {
        resumeAfterScrub.current = false;
        setPlaying(true);
      }
    },
    [setPlaying],
  );

  // The last error, and whether something is still shown under it
  const [error, setError] = useState<{ error: RecViewerError; shown: boolean } | null>(null);
  const reportError = useCallback((e: unknown, shown = false) => {
    const error = toRecError(e);
    setError({ error, shown });
    const onError = callbacks.current.props.onError;
    if (onError) onError(error);
    else console.error(error);
  }, []);

  // The canvas's size in device pixels, exactly, where the browser tells it: the size in CSS
  // pixels times the scale is off by a fraction on a scaled screen, and the browser then
  // stretches the picture to fit, which makes rows of pixels jump as the camera moves
  const devicePixels = useRef<{ w: number; h: number } | null>(null);
  const syncSize = useCallback(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const rect = canvas.getBoundingClientRect();
    const exact = devicePixels.current;
    const w = Math.max(1, exact ? exact.w : Math.round(rect.width * dpr()));
    const h = Math.max(1, exact ? exact.h : Math.round(rect.height * dpr()));
    const resized = canvas.width !== w || canvas.height !== h;
    if (resized) {
      canvas.width = w;
      canvas.height = h;
    }
    const viewer = viewerRef.current;
    viewer?.resize(w, h, dpr());
    // The whole level fits the new size, if it was shown and not zoomed or moved since
    if (viewer && stateRef.current && viewer.zoom() !== stateRef.current.zoom) sync();
    invalidate();
    // A new size clears the canvas, which would show blank until the next frame draws it
    if (resized && viewer?.hasContent()) viewer.render();
  }, [invalidate, sync]);

  // Create the GPU viewer once per canvas, with the settings as the props start them
  useEffect(() => {
    let cancelled = false;
    const canvas = canvasRef.current;
    if (!canvas) return;
    loadEngine()
      .then(() => createViewer(canvas))
      .then((viewer) => {
        if (cancelled) {
          viewer.free();
          return;
        }
        viewerRef.current = viewer;
        const { given, defaults } = start.current;
        for (const key of Object.keys(APPLY) as Setting[]) {
          (APPLY[key] as (v: Viewer, x: unknown) => void)(viewer, given[key] ?? defaults[key]);
        }
        stateRef.current = readState(viewer);
        setCanTranslucent(viewer.canDrawTranslucentGhosts());
        setViewerReady(true);
      })
      .catch(reportError);
    return () => {
      cancelled = true;
      viewerRef.current?.free();
      viewerRef.current = null;
      setViewerReady(false);
    };
  }, [reportError]);
  const start = useRef({ given, defaults });

  // Settings the props give go to the core when they change
  useEffect(() => {
    const viewer = viewerRef.current;
    if (!viewer) return;
    for (const key of Object.keys(APPLY) as Setting[]) {
      const value = given[key];
      if (value !== undefined && value !== stateRef.current?.[key]) {
        (APPLY[key] as (v: Viewer, x: unknown) => void)(viewer, value);
      }
    }
    viewer.setSplitScreen(splitScreen);
    viewer.setLooping(loop);
    sync();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [viewerReady, ...Object.values(given), splitScreen, loop, sync]);

  // The level options given, over the ones the core has
  const { grass, pictures, customSky, arrows, animatedObjects, stillObjects, fancyboost } = levelOptions ?? {};
  useEffect(() => {
    const viewer = viewerRef.current;
    const current = stateRef.current;
    if (!viewer || !current) return;
    const o = current.levelOptions;
    const next = {
      grass: grass ?? o.grass,
      pictures: pictures ?? o.pictures,
      customSky: customSky ?? o.customSky,
      arrows: arrows ?? o.arrows,
      animatedObjects: animatedObjects ?? o.animatedObjects,
      stillObjects: stillObjects ?? o.stillObjects,
      fancyboost: fancyboost ?? o.fancyboost,
    };
    if (!sameOptions(next, o)) change((v) => applyLevelOptions(v, next));
  }, [viewerReady, grass, pictures, customSky, arrows, animatedObjects, stillObjects, fancyboost, change]);

  // Load the replay, or the level on its own
  const wasLevelOnly = useRef(false);
  // What the viewer was given, for merging only the replays added to it
  const shownInput = useRef<ReplayInput | null>(null);
  useEffect(() => {
    const viewer = viewerRef.current;
    if (!viewerReady || !viewer) return;
    // Nothing given yet: RecPlayer shows the viewer while it downloads, so the GPU is ready
    // when the files are in
    if (!rec && !lev && !lgr && !res && !acrossPack) return;
    const input = { rec, lgr, lev, res, acrossPack, shirt, ghosts };
    const shown = viewer.hasContent() && !viewer.isAcross() ? shownInput.current : null;
    const added = shown ? addedGhosts(shown, input) : undefined;
    setError(null);
    if (!added) setLoaded(false);
    try {
      if (added) {
        // Merged in one at a time, so those before a broken one stay
        const all = input.ghosts ?? [];
        for (let i = all.length - added.length; i < all.length; i++) {
          const ghost = buildGhost(all[i]);
          try {
            viewer.addGhost(ghost);
          } finally {
            ghost.free();
          }
          shownInput.current = { ...input, ghosts: all.slice(0, i + 1) };
        }
      } else {
        syncSize();
        const replay = buildReplay(input);
        try {
          viewer.load(replay);
        } finally {
          replay.free();
        }
        shownInput.current = input;
      }
      const only = viewer.levelOnly();
      // A replay after a level shown whole goes back to the zoom it was given
      if (!only && wasLevelOnly.current) {
        viewer.setZoom(callbacks.current.props.zoom ?? defaultZoom);
        viewer.recenter();
      }
      wasLevelOnly.current = only;
      const bikes = viewer.bikeCount();
      setFinishTimes(Array.from({ length: bikes }, (_, i) => viewer.finishTime(i)));
      sync();
      callbacks.current.props.onDurationChange?.(viewer.duration());
      setLoaded(true);
    } catch (e) {
      // A failed merge goes on with the replay that was playing
      const shown = viewer.hasContent();
      if (shown) setLoaded(true);
      else shownInput.current = null;
      sync();
      reportError(e, shown);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [viewerReady, rec, lgr, lev, res, acrossPack, shirt, ghosts, reportError]);

  // Seek from the time prop
  useEffect(() => {
    if (loaded && time !== undefined) seek(time);
  }, [loaded, time, seek]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || typeof IntersectionObserver === "undefined") return;
    const observer = new IntersectionObserver(([entry]) => {
      onScreen.current = entry.isIntersecting;
      invalidate();
    });
    observer.observe(canvas);
    return () => observer.disconnect();
  }, [invalidate]);

  // Keep the canvas at device resolution
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const observer = new ResizeObserver((entries) => {
      const size = entries[0]?.devicePixelContentBoxSize?.[0];
      if (size) devicePixels.current = { w: size.inlineSize, h: size.blockSize };
      syncSize();
    });
    try {
      // Also told when moving the canvas changes how it lines up with the device pixels
      observer.observe(canvas, { box: "device-pixel-content-box" });
    } catch {
      // Safari has no device pixel sizes
      observer.observe(canvas);
    }
    syncSize();
    return () => observer.disconnect();
  }, [viewerReady, syncSize]);

  // The render loop. Draws every frame while playing, otherwise only after a change, and never
  // while out of sight, though playing time goes on. Objects move with the replay's time, so
  // they are still while paused, and then the loop stops until something changes.
  useEffect(() => {
    if (!loaded) return;
    let frame = 0;
    let last = performance.now();
    let reported = -1;
    let shownTime = "";
    const tick = (now: number) => {
      frame = 0;
      const viewer = viewerRef.current;
      const s = stateRef.current;
      if (!viewer || !s) return;
      // A frame's time can be from before the loop was woken
      const dt = Math.min(0.25, Math.max(0, now - last) / 1000);
      last = now;
      const draw = dirty.current || (onScreen.current && s.playing);
      if (!draw && !s.playing) return;
      frame = requestAnimationFrame(tick);
      dirty.current = false;
      // The core plays the replay, and pauses at the end before looping
      const ended = viewer.tick(dt);
      if (viewer.playing() !== s.playing || viewer.autoZoomPaused() !== s.autoZoomPaused) sync();
      if (ended) callbacks.current.props.onEnd?.();
      if (draw) viewer.render();
      const t = viewer.time();
      if (t !== reported) {
        reported = t;
        const text = formatTime(t);
        // React's text node is changed rather than replaced, which costs less every frame
        const shown = timeRef.current?.firstChild;
        if (shown instanceof Text && text !== shownTime) {
          shownTime = text;
          shown.data = text;
        }
        if (scrubRef.current && !scrubbing.current) scrubRef.current.value = String(t);
        callbacks.current.props.onTimeChange?.(t);
      }
    };
    wakeLoop.current = () => {
      if (frame) return;
      last = performance.now();
      frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    return () => {
      cancelAnimationFrame(frame);
      wakeLoop.current = () => {};
    };
  }, [loaded, sync]);

  // Dragging pans, the wheel and pinching zoom, a tap plays or pauses
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    return attachGestures(canvas, {
      pan: (dx, dy) => change((v) => v.panBy(dx * dpr(), dy * dpr())),
      zoom: (factor) => change((v) => v.zoomBy(factor)),
      tap: () => {
        if (!pressClosesSettings.current && !pressShowsControls.current) togglePlay();
      },
    });
  }, [change, togglePlay]);

  const toggleFullscreen = useCallback(() => {
    const el = containerRef.current;
    if (!el) return;
    if (document.fullscreenElement === el) {
      void document.exitFullscreen();
    } else if (cssFullscreen) {
      setCssFullscreen(false);
    } else if (document.fullscreenEnabled && el.requestFullscreen) {
      el.requestFullscreen().catch(() => setCssFullscreen(true));
    } else {
      // iPhones have no full screen for elements, so fill the window instead
      setCssFullscreen(true);
    }
  }, [cssFullscreen]);

  useEffect(() => {
    const onChange = () => setNativeFullscreen(document.fullscreenElement === containerRef.current);
    document.addEventListener("fullscreenchange", onChange);
    return () => document.removeEventListener("fullscreenchange", onChange);
  }, []);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    canvas.addEventListener("dblclick", toggleFullscreen);
    return () => canvas.removeEventListener("dblclick", toggleFullscreen);
  }, [toggleFullscreen]);

  // Pause in a hidden tab
  useEffect(() => {
    if (!pauseWhenHidden) return;
    const onVisibility = () => {
      if (document.visibilityState === "hidden" && stateRef.current?.playing) setPlaying(false);
    };
    document.addEventListener("visibilitychange", onVisibility);
    return () => document.removeEventListener("visibilitychange", onVisibility);
  }, [pauseWhenHidden, setPlaying]);

  // Keep the screen on while playing
  const playing = state?.playing ?? false;

  // In full screen the controls lie over the view, and hide while playing when the mouse is
  // still, like a video's
  const [idle, setIdle] = useState(false);
  const overControls = useRef(false);
  const idleTimer = useRef<number | undefined>(undefined);
  const wake = useCallback(() => {
    setIdle(false);
    window.clearTimeout(idleTimer.current);
    if (!fullscreen || !playing) return;
    idleTimer.current = window.setTimeout(() => {
      if (!overControls.current && !settingsOpen.current) setIdle(true);
    }, 2500);
  }, [fullscreen, playing]);
  useEffect(() => {
    wake();
    return () => window.clearTimeout(idleTimer.current);
  }, [wake]);
  const controlsHidden = fullscreen && idle;
  // In full screen the controls lie over the canvas's bottom, and the last apple time and the
  // minimap move up out from under them
  const controlsRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!loaded) return;
    const canvas = canvasRef.current;
    const bar = controlsRef.current;
    const covered = canvas && bar && fullscreen && !controlsHidden ? bar : null;
    const update = () => {
      const inset = covered
        ? canvas!.getBoundingClientRect().bottom - covered.getBoundingClientRect().top
        : 0;
      change((v) => v.setHudBottomInset(Math.max(0, Math.round(inset * dpr()))));
    };
    update();
    if (!covered) return;
    const observer = new ResizeObserver(update);
    observer.observe(covered);
    return () => observer.disconnect();
  }, [loaded, fullscreen, controlsHidden, change]);
  useEffect(() => {
    if (!wakeLock || !playing || !navigator.wakeLock) return;
    let lock: WakeLockSentinel | undefined;
    let done = false;
    navigator.wakeLock
      .request("screen")
      .then((l) => {
        if (done) void l.release();
        else lock = l;
      })
      .catch(() => {});
    return () => {
      done = true;
      void lock?.release();
    };
  }, [wakeLock, playing]);

  const onKeyDown = (e: ReactKeyboardEvent<HTMLDivElement>) => {
    wake();
    if (e.key === "Escape" && cssFullscreen) {
      setCssFullscreen(false);
      return;
    }
    if (!keyboard || !loaded) return;
    // Leave typing in the follow menu and the scrubber's own arrow keys alone
    const target = e.target as HTMLElement;
    if (target.tagName === "SELECT" || (target.tagName === "INPUT" && e.key.startsWith("Arrow"))) return;
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    if (viewerRef.current?.key(e.key, e.repeat)) {
      e.preventDefault();
      sync();
    }
  };

  useImperativeHandle(
    ref,
    () => ({
      play: () => setPlaying(true),
      pause: () => setPlaying(false),
      seek,
      duration: () => viewerRef.current?.duration() ?? 0,
      time: () => viewerRef.current?.time() ?? 0,
      recenter,
      fitLevel,
      toggleFullscreen,
    }),
    [seek, setPlaying, recenter, fitLevel, toggleFullscreen],
  );

  const s = state;
  const bikeCount = s?.bikeCount ?? 0;
  return (
    <div
      ref={containerRef}
      className={className}
      tabIndex={0}
      onKeyDown={onKeyDown}
      onPointerDownCapture={(e) => {
        pressClosesSettings.current = settingsOpen.current;
        pressShowsControls.current = controlsHidden && e.pointerType !== "mouse";
        wake();
      }}
      onPointerMove={wake}
      onPointerDown={() => containerRef.current?.focus({ preventScroll: true })}
      style={{
        position: "relative",
        display: "flex",
        flexDirection: "column",
        outline: "none",
        ...style,
        ...(cssFullscreen ? fullStyle : nativeFullscreen ? { width: "100%", height: "100%", background: "#000" } : {}),
        ...(controlsHidden ? { cursor: "none" } : {}),
      }}
    >
      <canvas
        ref={canvasRef}
        style={{ flex: 1, width: "100%", minHeight: 0, touchAction: "none", display: "block" }}
      />
      {error && showErrors !== false && (
        <div style={error.shown ? errorBadge : errorOverlay} role="alert">
          {error.shown ? `Couldn't add: ${error.error.message}` : `Couldn't play this: ${error.error.message}`}
        </div>
      )}
      {s?.autoZoomPaused && (
        <div style={pausedBadge} role="status">
          Auto zoom paused
          <button type="button" style={resumeButton} onClick={recenter}>
            Resume
          </button>
        </div>
      )}
      {controls && (
        <div
          ref={controlsRef}
          onPointerEnter={() => (overControls.current = true)}
          onPointerLeave={() => (overControls.current = false)}
          style={
            fullscreen
              ? {
                  position: "absolute",
                  left: 0,
                  right: 0,
                  bottom: 0,
                  // Clear of a phone's rounded corners, notch and home bar
                  padding:
                    "0 env(safe-area-inset-right) env(safe-area-inset-bottom) env(safe-area-inset-left)",
                  transition: "opacity 0.3s",
                  opacity: controlsHidden ? 0 : 1,
                  pointerEvents: controlsHidden ? "none" : undefined,
                }
              : undefined
          }
        >
          <Controls
            levelOnly={s?.hasContent ? s.levelOnly : (levelOnlyComing ?? (!!lev && !rec))}
            multiplayer={s?.multiplayer ?? false}
            across={s?.across ?? false}
            loaded={loaded}
            playing={playing}
            speed={s?.speed ?? 1}
            zoom={s?.zoom ?? defaultZoom}
            duration={s?.duration ?? 0}
            bikeCount={bikeCount}
            follow={s?.follow ?? 0}
            splitScreen={splitScreen && (s?.multiplayer ?? false)}
            bothViews={s?.bothViews ?? false}
            bikeLabels={Array.from({ length: bikeCount }, (_, i) => {
              const name = bikeLabels?.[i] || `Bike ${i + 1}`;
              const finish = finishTimes[i];
              return finish === undefined ? name : `${name} ${formatTime(finish)}`;
            })}
            fullscreen={fullscreen}
            viewRef={containerRef}
            timeRef={timeRef}
            scrubRef={scrubRef}
            onScrub={scrub}
            settingsOpen={settingsOpen}
            onTogglePlay={togglePlay}
            onSeek={seek}
            onSlower={() => act("Slower")}
            onFaster={() => act("Faster")}
            onReverse={() => act("Reverse")}
            onZoomOut={() => act("ZoomOut")}
            onZoomIn={() => act("ZoomIn")}
            onFitLevel={fitLevel}
            // Choosing whom to follow puts the camera on them, alone
            onFollow={(bike) =>
              change((v) => {
                v.setFollow(bike);
                v.setBothViews(false);
                v.recenter();
              })
            }
            onBothViews={() =>
              change((v) => {
                v.setBothViews(true);
                v.recenter();
              })
            }
            onFullscreen={toggleFullscreen}
            centeredCamera={s?.centeredCamera ?? true}
            onCenteredCamera={(x) => change((v) => v.setCenteredCamera(x))}
            timer={s?.timer ?? true}
            onTimer={(x) => change((v) => v.setTimer(x))}
            minimap={s?.minimap ?? false}
            onMinimap={(x) => change((v) => v.setMinimap(x))}
            minimapSize={s?.minimapSize ?? 1}
            onMinimapSize={(x) => change((v) => v.setMinimapSize(x))}
            lastAppleTime={s?.lastAppleTime ?? true}
            onLastAppleTime={(x) => change((v) => v.setLastAppleTime(x))}
            picturesInBackground={s?.picturesInBackground ?? true}
            onPicturesInBackground={(x) => change((v) => v.setPicturesInBackground(x))}
            zoomTextures={s?.zoomTextures ?? true}
            onZoomTextures={(x) => change((v) => v.setZoomTextures(x))}
            largeFont={s?.largeFont ?? false}
            onLargeFont={(x) => change((v) => v.setLargeFont(x))}
            autoZoom={s?.autoZoom ?? false}
            onAutoZoom={(x) => change((v) => v.setAutoZoom(x))}
            translucentGhosts={s?.translucentGhosts ?? false}
            onTranslucentGhosts={(x) => change((v) => v.setTranslucentGhosts(x))}
            canDrawTranslucentGhosts={canTranslucent}
            autoPlaySetting={autoPlaySetting}
            onAutoPlaySetting={onAutoPlaySettingChange}
            lgrChoices={lgrChoices}
            lgrChoice={lgrChoice ?? null}
            onLgrChoice={onLgrChoiceChange}
            onSettingsOpen={onSettingsOpen}
            levelOptions={s?.levelOptions ?? DEFAULT_LEVEL_OPTIONS}
            onLevelOptions={(o) =>
              change((v) => applyLevelOptions(v, { ...(stateRef.current?.levelOptions ?? DEFAULT_LEVEL_OPTIONS), ...o }))
            }
          />
        </div>
      )}
    </div>
  );
});

const DEFAULT_LEVEL_OPTIONS: Required<LevelOptions> = {
  grass: true,
  pictures: true,
  customSky: true,
  arrows: true,
  animatedObjects: true,
  stillObjects: false,
  fancyboost: true,
};
