// Seconds as the game shows them, mm:ss:cc
export function formatTime(seconds: number): string {
  const centiseconds = Math.floor(Math.max(0, seconds) * 100);
  const m = Math.floor(centiseconds / 6000);
  const s = Math.floor(centiseconds / 100) % 60;
  const c = centiseconds % 100;
  const pad = (n: number) => n.toString().padStart(2, "0");
  return `${pad(m)}:${pad(s)}:${pad(c)}`;
}

// 1x, 0.8x, -1.25x
export function formatSpeed(speed: number): string {
  return `${Number(speed.toFixed(2))}x`;
}
