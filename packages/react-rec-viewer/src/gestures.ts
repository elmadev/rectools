// Mouse, pen and touch input on the canvas. Positions are CSS pixels from the canvas's top
// left corner.
export interface GestureHandlers {
  // Drag the view by this much, right and down
  pan(dx: number, dy: number): void;
  // Multiply the zoom by `factor`, leaving the camera where it is
  zoom(factor: number): void;
  // A short press without moving
  tap(): void;
}

// How far a press may move, and how long it may last, and still be a tap
const TAP_DISTANCE = 6;
const TAP_MILLISECONDS = 300;

// Wheel deltas come in pixels, lines (Firefox with a mouse) or pages
function wheelPixels(e: WheelEvent, pageHeight: number): number {
  if (e.deltaMode === WheelEvent.DOM_DELTA_LINE) return e.deltaY * 16;
  if (e.deltaMode === WheelEvent.DOM_DELTA_PAGE) return e.deltaY * pageHeight;
  return e.deltaY;
}

export function attachGestures(canvas: HTMLCanvasElement, handlers: GestureHandlers): () => void {
  const pointers = new Map<number, { x: number; y: number }>();
  // The press that may still turn out to be a tap
  let press: { x: number; y: number; time: number } | null = null;
  // Distance and middle of the two fingers of a pinch, when the last move was handled
  let pinch: { distance: number; x: number; y: number } | null = null;

  const local = (e: { clientX: number; clientY: number }) => {
    const rect = canvas.getBoundingClientRect();
    return { x: e.clientX - rect.left, y: e.clientY - rect.top };
  };

  const startPinch = () => {
    const [a, b] = [...pointers.values()];
    pinch = { distance: Math.hypot(a.x - b.x, a.y - b.y), x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 };
  };

  const onWheel = (e: WheelEvent) => {
    e.preventDefault();
    const pixels = wheelPixels(e, canvas.clientHeight);
    // A trackpad pinch arrives as a wheel with ctrlKey, in smaller steps
    handlers.zoom(Math.exp(-pixels * (e.ctrlKey ? 0.01 : 0.002)));
  };

  const onDown = (e: PointerEvent) => {
    if (e.pointerType === "mouse" && e.button !== 0) return;
    canvas.setPointerCapture(e.pointerId);
    const p = local(e);
    pointers.set(e.pointerId, p);
    if (pointers.size === 1) {
      press = { x: p.x, y: p.y, time: e.timeStamp };
    } else {
      // A second finger: a pinch, never a tap
      press = null;
      if (pointers.size === 2) startPinch();
    }
  };

  const onMove = (e: PointerEvent) => {
    const previous = pointers.get(e.pointerId);
    if (!previous) return;
    const current = local(e);
    pointers.set(e.pointerId, current);
    if (press) {
      // Still a tap until it moves far enough, then a drag from where it was pressed
      if (Math.hypot(current.x - press.x, current.y - press.y) <= TAP_DISTANCE) return;
      handlers.pan(current.x - press.x, current.y - press.y);
      press = null;
      return;
    }
    if (pointers.size >= 2 && pinch) {
      const last = pinch;
      startPinch();
      const now = pinch;
      if (last.distance > 0 && now.distance > 0) {
        handlers.zoom(now.distance / last.distance);
      }
      handlers.pan(now.x - last.x, now.y - last.y);
      return;
    }
    if (pointers.size === 1) {
      handlers.pan(current.x - previous.x, current.y - previous.y);
    }
  };

  const onUp = (e: PointerEvent) => {
    if (!pointers.delete(e.pointerId)) return;
    if (e.type === "pointerup" && press && e.timeStamp - press.time <= TAP_MILLISECONDS) {
      handlers.tap();
    }
    press = null;
    // Lifting one finger of a pinch carries on as a drag with the other, without a jump
    // because each pointer keeps its own last position
    if (pointers.size === 2) startPinch();
    else pinch = null;
  };

  // Stop the page from zooming or scrolling while a finger is on the player
  const onTouch = (e: TouchEvent) => e.preventDefault();

  canvas.addEventListener("wheel", onWheel, { passive: false });
  canvas.addEventListener("touchstart", onTouch, { passive: false });
  canvas.addEventListener("touchmove", onTouch, { passive: false });
  canvas.addEventListener("pointerdown", onDown);
  canvas.addEventListener("pointermove", onMove);
  canvas.addEventListener("pointerup", onUp);
  canvas.addEventListener("pointercancel", onUp);
  return () => {
    canvas.removeEventListener("wheel", onWheel);
    canvas.removeEventListener("touchstart", onTouch);
    canvas.removeEventListener("touchmove", onTouch);
    canvas.removeEventListener("pointerdown", onDown);
    canvas.removeEventListener("pointermove", onMove);
    canvas.removeEventListener("pointerup", onUp);
    canvas.removeEventListener("pointercancel", onUp);
  };
}
