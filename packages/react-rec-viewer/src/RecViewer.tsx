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
import { Action, ReplayInput, Viewer, buildReplay, loadEngine } from "./engine.js";
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
  // Draw bikes and objects over every picture, on unless set
  picturesInBackground?: boolean;
  defaultPicturesInBackground?: boolean;
  onPicturesInBackgroundChange?: (value: boolean) => void;
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

const fullStyle: CSSProperties = {
  position: "fixed",
  inset: 0,
  width: "100vw",
  height: "100vh",
  zIndex: 10000,
  background: "#000",
};

// What the player core shows at a moment, see readState
interface State {
  hasContent: boolean;
  levelOnly: boolean;
  across: boolean;
  duration: number;
  bikeCount: number;
  autoZoomPaused: boolean;
  playing: boolean;
  speed: number;
  zoom: number;
  follow: number;
  centeredCamera: boolean;
  timer: boolean;
  minimap: boolean;
  autoZoom: boolean;
  translucentGhosts: boolean;
  picturesInBackground: boolean;
  levelOptions: Required<LevelOptions>;
}

function readState(viewer: Viewer): State {
  const s = viewer.state();
  const state: State = {
    hasContent: s.hasContent,
    levelOnly: s.levelOnly,
    across: s.across,
    duration: s.duration,
    bikeCount: s.bikeCount,
    autoZoomPaused: s.autoZoomPaused,
    playing: s.playing,
    speed: s.speed,
    zoom: s.zoom,
    follow: s.follow,
    centeredCamera: s.centeredCamera,
    timer: s.timer,
    minimap: s.minimap,
    autoZoom: s.autoZoom,
    translucentGhosts: s.translucentGhosts,
    picturesInBackground: s.picturesInBackground,
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
  | "autoZoom"
  | "translucentGhosts"
  | "picturesInBackground";

const APPLY: { [K in Setting]: (viewer: Viewer, value: State[K]) => void } = {
  playing: (v, x) => v.setPlaying(x),
  speed: (v, x) => v.setSpeed(x),
  zoom: (v, x) => v.setZoom(x),
  follow: (v, x) => v.setFollow(x),
  centeredCamera: (v, x) => v.setCenteredCamera(x),
  timer: (v, x) => v.setTimer(x),
  minimap: (v, x) => v.setMinimap(x),
  autoZoom: (v, x) => v.setAutoZoom(x),
  translucentGhosts: (v, x) => v.setTranslucentGhosts(x),
  picturesInBackground: (v, x) => v.setPicturesInBackground(x),
};

const applyLevelOptions = (viewer: Viewer, o: Required<LevelOptions>) => viewer.setLevelOptions(o);

const sameOptions = (a: Required<LevelOptions>, b: Required<LevelOptions>) =>
  (Object.keys(a) as (keyof LevelOptions)[]).every((k) => a[k] === b[k]);

export const RecViewer = forwardRef<RecViewerHandle, RecViewerProps>(function RecViewer(props, ref) {
  const {
    rec,
    lgr,
    lev,
    res,
    acrossPack,
    shirt,
    ghosts,
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
    autoZoom: props.autoZoom,
    translucentGhosts: props.translucentGhosts,
    picturesInBackground: props.picturesInBackground,
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
    autoZoom: props.defaultAutoZoom ?? false,
    translucentGhosts: props.defaultTranslucentGhosts ?? false,
    picturesInBackground: props.defaultPicturesInBackground ?? true,
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
    autoZoom: props.onAutoZoomChange,
    translucentGhosts: props.onTranslucentGhostsChange,
    picturesInBackground: props.onPicturesInBackgroundChange,
    levelOptions: props.onLevelOptionsChange,
  } as { [K in Setting | "levelOptions"]?: (value: State[K]) => void };
  const callbacks = useRef({ changed, props });
  callbacks.current = { changed, props };

  const containerRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const timeRef = useRef<HTMLSpanElement>(null);
  const scrubRef = useRef<HTMLInputElement>(null);
  const applesRef = useRef<HTMLSpanElement>(null);
  const scrubbing = useRef(false);
  // Whether the settings were open when the current press began: that press only closes them
  const settingsOpen = useRef(false);
  const pressClosesSettings = useRef(false);
  const viewerRef = useRef<Viewer | null>(null);
  // Something changed that a paused player has to draw
  const dirty = useRef(true);
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
    stateRef.current = next;
    dirty.current = true;
    setState(next);
    if (!prev) return;
    const on = callbacks.current.changed;
    for (const key of Object.keys(APPLY) as Setting[]) {
      if (next[key] !== prev[key]) (on[key] as ((v: unknown) => void) | undefined)?.(next[key]);
    }
    if (!sameOptions(next.levelOptions, prev.levelOptions)) on.levelOptions?.(next.levelOptions);
  }, []);

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
  const act = useCallback((action: Action) => change((v) => v.act(action)), [change]);
  const togglePlay = useCallback(() => act(Action.TogglePlay), [act]);
  const fitLevel = useCallback(() => act(Action.FitLevel), [act]);
  const recenter = useCallback(() => act(Action.Recenter), [act]);
  const seek = useCallback((seconds: number) => change((v) => v.seek(seconds)), [change]);
  const setPlaying = useCallback((value: boolean) => change((v) => v.setPlaying(value)), [change]);

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
    if (canvas.width !== w || canvas.height !== h) {
      canvas.width = w;
      canvas.height = h;
    }
    viewerRef.current?.resize(w, h, dpr());
    dirty.current = true;
  }, []);

  // Create the GPU viewer once per canvas, with the settings as the props start them
  useEffect(() => {
    let cancelled = false;
    const canvas = canvasRef.current;
    if (!canvas) return;
    loadEngine()
      .then(() => Viewer.create(canvas))
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
  useEffect(() => {
    const viewer = viewerRef.current;
    if (!viewerReady || !viewer) return;
    setLoaded(false);
    setError(null);
    try {
      syncSize();
      const replay = buildReplay({ rec, lgr, lev, res, acrossPack, shirt, ghosts });
      try {
        viewer.load(replay);
      } finally {
        replay.free();
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
      dirty.current = true;
    });
    observer.observe(canvas);
    return () => observer.disconnect();
  }, []);

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

  // The render loop. Draws every frame while playing or while objects move, otherwise only
  // after a change, and never while out of sight, though playing time goes on.
  useEffect(() => {
    if (!loaded) return;
    let frame = 0;
    let last = performance.now();
    let reported = -1;
    let apples = NaN;
    const tick = (now: number) => {
      frame = requestAnimationFrame(tick);
      const viewer = viewerRef.current;
      const s = stateRef.current;
      if (!viewer || !s) return;
      const dt = Math.min(0.25, (now - last) / 1000);
      last = now;
      const moving = s.levelOptions.animatedObjects || !s.levelOptions.stillObjects;
      const draw = dirty.current || (onScreen.current && (s.playing || moving));
      if (!draw && !s.playing) return;
      dirty.current = false;
      // The core plays the replay, and pauses at the end before looping
      const ended = viewer.tick(dt);
      if (viewer.playing() !== s.playing || viewer.autoZoomPaused() !== s.autoZoomPaused) sync();
      if (ended) callbacks.current.props.onEnd?.();
      if (draw) viewer.render();
      const t = viewer.time();
      if (t !== reported) {
        reported = t;
        if (timeRef.current) timeRef.current.textContent = formatTime(t);
        if (scrubRef.current && !scrubbing.current) scrubRef.current.value = String(t);
        const info = viewer.appleInfo();
        const key = info.taken * 1e6 + (info.lastTime ?? -1);
        if (applesRef.current && key !== apples) {
          apples = key;
          applesRef.current.textContent =
            info.total > 0
              ? `🍎 ${info.taken}/${info.total}` +
                (info.lastTime !== undefined ? ` ${formatTime(info.lastTime)}` : "")
              : "";
        }
        info.free();
        callbacks.current.props.onTimeChange?.(t);
      }
    };
    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  }, [loaded, sync]);

  // Dragging pans, the wheel and pinching zoom, a tap plays or pauses
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    return attachGestures(canvas, {
      pan: (dx, dy) => change((v) => v.panBy(dx * dpr(), dy * dpr())),
      zoom: (factor) => change((v) => v.zoomBy(factor)),
      tap: () => {
        if (!pressClosesSettings.current) togglePlay();
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
    if (e.key === "Escape" && cssFullscreen) {
      setCssFullscreen(false);
      return;
    }
    if (!keyboard || !loaded) return;
    // Leave typing in the follow menu and the scrubber's own arrow keys alone
    const target = e.target as HTMLElement;
    if (target.tagName === "SELECT" || (target.tagName === "INPUT" && e.key.startsWith("Arrow"))) return;
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    if (viewerRef.current?.key(e.key)) {
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
      onPointerDownCapture={() => (pressClosesSettings.current = settingsOpen.current)}
      onPointerDown={() => containerRef.current?.focus({ preventScroll: true })}
      style={{
        position: "relative",
        display: "flex",
        flexDirection: "column",
        outline: "none",
        ...style,
        ...(cssFullscreen ? fullStyle : nativeFullscreen ? { width: "100%", height: "100%", background: "#000" } : {}),
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
        <Controls
          levelOnly={s?.levelOnly ?? false}
          across={s?.across ?? false}
          loaded={loaded}
          playing={playing}
          speed={s?.speed ?? 1}
          zoom={s?.zoom ?? defaultZoom}
          duration={s?.duration ?? 0}
          bikeCount={bikeCount}
          follow={s?.follow ?? 0}
          bikeLabels={Array.from({ length: bikeCount }, (_, i) => {
            const name = bikeLabels?.[i] || `Bike ${i + 1}`;
            const finish = finishTimes[i];
            return finish === undefined ? name : `${name} ${formatTime(finish)}`;
          })}
          fullscreen={fullscreen}
          timeRef={timeRef}
          scrubRef={scrubRef}
          applesRef={applesRef}
          scrubbing={scrubbing}
          settingsOpen={settingsOpen}
          onTogglePlay={togglePlay}
          onSeek={seek}
          onSlower={() => act(Action.Slower)}
          onFaster={() => act(Action.Faster)}
          onReverse={() => act(Action.Reverse)}
          onZoomOut={() => act(Action.ZoomOut)}
          onZoomIn={() => act(Action.ZoomIn)}
          onFitLevel={fitLevel}
          // Choosing whom to follow puts the camera on them
          onFollow={(bike) =>
            change((v) => {
              v.setFollow(bike);
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
          picturesInBackground={s?.picturesInBackground ?? true}
          onPicturesInBackground={(x) => change((v) => v.setPicturesInBackground(x))}
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
