import { CSSProperties, MutableRefObject, ReactNode, RefObject, useEffect, useLayoutEffect, useRef, useState } from "react";
import { formatSpeed, formatTime } from "./format.js";
import { keyList } from "./keys.js";
import type { LevelOptions, LgrChoice } from "./RecViewer.js";

export interface ControlsProps {
  levelOnly: boolean;
  // Drawn with Across's graphics: the LGR, grass, pictures and gravity arrows don't apply
  across: boolean;
  loaded: boolean;
  playing: boolean;
  speed: number;
  zoom: number;
  duration: number;
  bikeCount: number;
  follow: number;
  // With the finish times
  bikeLabels: string[];
  fullscreen: boolean;
  // The whole viewer, which the settings stay inside
  viewRef: RefObject<HTMLElement>;
  // Written by the render loop, so playing never re-renders React
  timeRef: RefObject<HTMLSpanElement>;
  scrubRef: RefObject<HTMLInputElement>;
  applesRef: RefObject<HTMLSpanElement>;
  scrubbing: MutableRefObject<boolean>;
  // Whether the settings are open, for the view: the press that closes them doesn't play or pause
  settingsOpen: MutableRefObject<boolean>;
  onTogglePlay(): void;
  onSeek(seconds: number): void;
  onSlower(): void;
  onFaster(): void;
  onReverse(): void;
  onZoomOut(): void;
  onZoomIn(): void;
  onFitLevel(): void;
  onFollow(bike: number): void;
  onFullscreen(): void;
  centeredCamera: boolean;
  onCenteredCamera(value: boolean): void;
  timer: boolean;
  onTimer(value: boolean): void;
  minimap: boolean;
  onMinimap(value: boolean): void;
  picturesInBackground: boolean;
  onPicturesInBackground(value: boolean): void;
  autoZoom: boolean;
  onAutoZoom(value: boolean): void;
  translucentGhosts: boolean;
  onTranslucentGhosts(value: boolean): void;
  canDrawTranslucentGhosts: boolean;
  autoPlaySetting?: boolean;
  onAutoPlaySetting?(value: boolean): void;
  lgrChoices?: LgrChoice[];
  lgrChoice: string | null;
  onLgrChoice?(value: string | null): void;
  onSettingsOpen?(): void;
  levelOptions: Required<LevelOptions>;
  onLevelOptions(change: Partial<LevelOptions>): void;
}

// Icons drawn in the text color, the same in every browser and font
const ICONS: Record<string, ReactNode> = {
  play: <path d="M8 5v14l11-7z" />,
  pause: <path d="M7 5h4v14H7zM13 5h4v14h-4z" />,
  slower: <path d="M11 6v12l-8.5-6zM20 6v12l-8.5-6z" />,
  faster: <path d="M4 6v12l8.5-6zM13 6v12l8.5-6z" />,
  reverse: (
    <path d="M7 4 3 8l4 4V9h11V7H7zM17 12v3H6v2h11v3l4-4z" />
  ),
  minus: <path d="M5 11h14v2H5z" />,
  plus: <path d="M11 5h2v6h6v2h-6v6h-2v-6H5v-2h6z" />,
  fit: <path d="M2 5h20v14H2zM4 7v10h16V7zM6 15l4-5 3 3 2-2 3 4z" />,
  fullscreen: <path d="M4 4h6v2H6v4H4zM14 4h6v6h-2V6h-4zM4 14h2v4h4v2H4zM18 14h2v6h-6v-2h4z" />,
  exitFullscreen: <path d="M8 4h2v6H4V8h4zM14 4h2v4h4v2h-6zM4 14h6v6H8v-4H4zM14 14h6v2h-4v4h-2z" />,
  settings: (
    <path d="M19.4 13a7.6 7.6 0 0 0 0-2l2.1-1.6-2-3.5-2.5 1a7.4 7.4 0 0 0-1.7-1L15 3.3h-4l-.4 2.6a7.4 7.4 0 0 0-1.7 1l-2.5-1-2 3.5L6.6 11a7.6 7.6 0 0 0 0 2l-2.1 1.6 2 3.5 2.5-1a7.4 7.4 0 0 0 1.7 1l.4 2.6h4l.4-2.6a7.4 7.4 0 0 0 1.7-1l2.5 1 2-3.5zM12 15.5a3.5 3.5 0 1 1 0-7 3.5 3.5 0 0 1 0 7z" />
  ),
};

function Icon({ name }: { name: keyof typeof ICONS }) {
  return (
    <svg viewBox="0 0 24 24" width="18" height="18" fill="currentColor" fillRule="evenodd" aria-hidden="true" style={{ display: "block" }}>
      {ICONS[name]}
    </svg>
  );
}

const bar: CSSProperties = {
  position: "relative",
  display: "flex",
  flexWrap: "wrap",
  gap: 4,
  alignItems: "center",
  padding: "4px 6px",
  font: "13px system-ui, sans-serif",
  background: "#1b1b1b",
  color: "#eee",
};

const buttonStyle: CSSProperties = {
  font: "inherit",
  minWidth: 30,
  height: 28,
  padding: "0 6px",
  display: "inline-flex",
  alignItems: "center",
  justifyContent: "center",
  border: "1px solid #444",
  borderRadius: 4,
  background: "#2b2b2b",
  color: "inherit",
  cursor: "pointer",
};

const numbers: CSSProperties = { fontVariantNumeric: "tabular-nums", whiteSpace: "nowrap" };

const menu: CSSProperties = {
  position: "absolute",
  right: 6,
  bottom: "calc(100% + 4px)",
  zIndex: 1,
  display: "flex",
  flexDirection: "column",
  gap: 6,
  padding: "10px 12px",
  minWidth: 220,
  // The height limit includes the padding and border
  boxSizing: "border-box",
  maxHeight: "min(70vh, 480px)",
  overflowY: "auto",
  border: "1px solid #444",
  borderRadius: 4,
  background: "#1b1b1b",
  boxShadow: "0 4px 16px rgba(0, 0, 0, 0.5)",
};

const row: CSSProperties = { display: "flex", alignItems: "center", gap: 6, cursor: "pointer" };
const heading: CSSProperties = { margin: "4px 0 0", fontSize: 11, textTransform: "uppercase", color: "#999" };

function Button(props: {
  label: string;
  onClick(): void;
  disabled?: boolean;
  pressed?: boolean;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      className="rectools-button"
      title={props.label}
      aria-label={props.label}
      aria-pressed={props.pressed}
      disabled={props.disabled}
      onClick={props.onClick}
      style={{ ...buttonStyle, ...(props.pressed ? { background: "#4a4a4a" } : {}) }}
    >
      {props.children}
    </button>
  );
}

function Toggle(props: {
  label: string;
  value: boolean;
  onChange(value: boolean): void;
  disabled?: boolean;
  note?: string;
}) {
  return (
    <label style={{ ...row, ...(props.disabled ? { opacity: 0.5, cursor: "default" } : {}) }} title={props.note}>
      <input
        type="checkbox"
        checked={props.value}
        disabled={props.disabled}
        onChange={(e) => props.onChange(e.target.checked)}
      />
      {props.label}
    </label>
  );
}

// The camera and how the level is drawn, for replays and levels on their own alike
function Settings(p: ControlsProps) {
  const o = p.levelOptions;
  return (
    <>
      {!p.levelOnly && (
        <>
          <p style={heading}>Camera</p>
          <Toggle label="Centered camera" value={p.centeredCamera} onChange={p.onCenteredCamera} />
          <Toggle label="Timer" value={p.timer} onChange={p.onTimer} />
          <Toggle label="Minimap" value={p.minimap} onChange={p.onMinimap} />
          {p.bikeCount > 1 && (
            <>
              <Toggle label="Zoom out to show every bike" value={p.autoZoom} onChange={p.onAutoZoom} />
              <Toggle
                label="See-through merged bikes"
                value={p.translucentGhosts && p.canDrawTranslucentGhosts}
                onChange={p.onTranslucentGhosts}
                disabled={!p.canDrawTranslucentGhosts}
                note={p.canDrawTranslucentGhosts ? undefined : "Not available in this browser"}
              />
            </>
          )}
        </>
      )}
      {!p.levelOnly && p.onAutoPlaySetting && (
        <>
          <p style={heading}>Playing</p>
          <Toggle label="Autoplay" value={!!p.autoPlaySetting} onChange={p.onAutoPlaySetting} />
        </>
      )}
      <p style={heading}>Level</p>
      {p.onLgrChoice && !p.across && (
        <label style={{ ...row, cursor: "default" }}>
          LGR
          <select
            aria-label="LGR"
            value={p.lgrChoice ?? ""}
            onChange={(e) => p.onLgrChoice?.(e.target.value || null)}
            style={{ ...buttonStyle, height: 24, flex: 1, minWidth: 0 }}
          >
            <option value="">The level's own</option>
            {/* The chosen one even before the list has loaded */}
            {p.lgrChoice && !p.lgrChoices?.some((c) => c.value === p.lgrChoice) && (
              <option value={p.lgrChoice}>{p.lgrChoice}</option>
            )}
            {p.lgrChoices?.map((c) => (
              <option key={c.value} value={c.value}>
                {c.label}
              </option>
            ))}
            {!p.lgrChoices?.length && <option disabled>Loading…</option>}
          </select>
        </label>
      )}
      {!p.across && (
        <>
          <Toggle label="Grass" value={o.grass} onChange={(grass) => p.onLevelOptions({ grass })} />
          <Toggle label="Pictures" value={o.pictures} onChange={(pictures) => p.onLevelOptions({ pictures })} />
          <Toggle
            label="Fancyboost"
            value={o.fancyboost}
            onChange={(fancyboost) => p.onLevelOptions({ fancyboost })}
          />
          <Toggle
            label="Default ground and sky"
            value={!o.customSky}
            onChange={(standard) => p.onLevelOptions({ customSky: !standard })}
          />
          <Toggle label="Gravity arrows" value={o.arrows} onChange={(arrows) => p.onLevelOptions({ arrows })} />
        </>
      )}
      <Toggle
        label="Animated objects"
        value={o.animatedObjects}
        onChange={(animatedObjects) => p.onLevelOptions({ animatedObjects })}
      />
      {!p.across && (
        <>
          <Toggle
            label="Still objects"
            value={o.stillObjects}
            onChange={(stillObjects) => p.onLevelOptions({ stillObjects })}
          />
          <Toggle
            label="Pictures in background"
            value={p.picturesInBackground}
            onChange={p.onPicturesInBackground}
          />
        </>
      )}
      <p style={heading}>Keys</p>
      <table style={{ borderSpacing: "8px 2px", margin: "0 -8px", fontSize: 12 }}>
        <tbody>
          {keyList()
            .filter((k) => (!p.levelOnly || k.level) && !(p.across && k.elmaOnly))
            .map((k) => (
              <tr key={k.keys}>
                <td style={{ whiteSpace: "nowrap", color: "#bbb" }}>{k.keys}</td>
                <td>{k.description}</td>
              </tr>
            ))}
        </tbody>
      </table>
    </>
  );
}

export function Controls(p: ControlsProps) {
  const disabled = !p.loaded;
  const [settingsOpen, setSettingsOpen] = useState(false);
  const barRef = useRef<HTMLDivElement>(null);
  // The room above the bar, so the settings don't reach past the top of the viewer, where a
  // page may have a header over it
  const [menuRoom, setMenuRoom] = useState<number | null>(null);

  useLayoutEffect(() => {
    const view = p.viewRef.current;
    if (!settingsOpen || !view || !barRef.current) return;
    const measure = () => {
      const room = barRef.current!.getBoundingClientRect().top - view.getBoundingClientRect().top - 8;
      setMenuRoom(Math.max(120, room));
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(view);
    return () => observer.disconnect();
  }, [settingsOpen, p.viewRef]);

  // Clicking anywhere else closes the settings
  useEffect(() => {
    p.settingsOpen.current = settingsOpen;
    if (!settingsOpen) return;
    const close = (e: PointerEvent) => {
      if (!barRef.current?.contains(e.target as Node)) setSettingsOpen(false);
    };
    document.addEventListener("pointerdown", close);
    return () => document.removeEventListener("pointerdown", close);
  }, [settingsOpen, p.settingsOpen]);

  return (
    <div ref={barRef} className="rectools-controls" style={bar}>
      {settingsOpen && (
        <div
          className="rectools-settings"
          style={menuRoom === null ? menu : { ...menu, maxHeight: `min(70vh, 480px, ${menuRoom}px)` }}
          role="dialog"
          aria-label="Settings"
        >
          <Settings {...p} />
        </div>
      )}
      {!p.levelOnly && (
        <>
          <Button label={p.playing ? "Pause" : "Play"} onClick={p.onTogglePlay} disabled={disabled}>
            <Icon name={p.playing ? "pause" : "play"} />
          </Button>
          <input
            ref={p.scrubRef}
            type="range"
            min={0}
            max={p.duration || 0}
            step={0.01}
            defaultValue={0}
            disabled={disabled}
            aria-label="Position"
            onPointerDown={() => (p.scrubbing.current = true)}
            onPointerUp={() => (p.scrubbing.current = false)}
            onPointerCancel={() => (p.scrubbing.current = false)}
            onInput={(e) => p.onSeek(Number(e.currentTarget.value))}
            style={{ flex: "1 1 120px", minWidth: 80, touchAction: "none", accentColor: "#0075ff" }}
          />
          <span style={numbers}>
            <span ref={p.timeRef}>{formatTime(0)}</span> / {formatTime(p.duration)}
          </span>
          <span ref={p.applesRef} style={numbers} title="Apples taken, and when the last one was" />
          <Button label="Slower" onClick={p.onSlower} disabled={disabled}>
            <Icon name="slower" />
          </Button>
          <span style={{ ...numbers, minWidth: 42, textAlign: "center" }} title="Speed">
            {formatSpeed(p.speed)}
          </span>
          <Button label="Faster" onClick={p.onFaster} disabled={disabled}>
            <Icon name="faster" />
          </Button>
          <Button label="Reverse" onClick={p.onReverse} disabled={disabled} pressed={p.speed < 0}>
            <Icon name="reverse" />
          </Button>
        </>
      )}
      {p.levelOnly && <span style={{ flex: 1 }} />}
      {p.bikeCount > 1 && (
        <select
          aria-label="Follow"
          title="Follow"
          value={p.follow}
          onChange={(e) => p.onFollow(Number(e.target.value))}
          style={{ ...buttonStyle, maxWidth: 220 }}
        >
          {Array.from({ length: p.bikeCount }, (_, i) => (
            <option key={i} value={i}>
              {p.bikeLabels[i]}
            </option>
          ))}
        </select>
      )}
      <Button label="Zoom out" onClick={p.onZoomOut} disabled={disabled}>
        <Icon name="minus" />
      </Button>
      <span style={{ ...numbers, minWidth: 40, textAlign: "center" }} title="Zoom">
        {p.zoom.toFixed(2)}x
      </span>
      <Button label="Zoom in" onClick={p.onZoomIn} disabled={disabled}>
        <Icon name="plus" />
      </Button>
      {p.levelOnly && (
        <Button label="Show the whole level" onClick={p.onFitLevel} disabled={disabled}>
          <Icon name="fit" />
        </Button>
      )}
      <Button
        label="Settings"
        onClick={() => {
          if (!settingsOpen) p.onSettingsOpen?.();
          setSettingsOpen(!settingsOpen);
        }}
        pressed={settingsOpen}
      >
        <Icon name="settings" />
      </Button>
      <Button label={p.fullscreen ? "Exit full screen" : "Full screen"} onClick={p.onFullscreen}>
        <Icon name={p.fullscreen ? "exitFullscreen" : "fullscreen"} />
      </Button>
    </div>
  );
}
