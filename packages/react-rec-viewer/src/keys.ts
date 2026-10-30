import { keyHelp } from "./engine.js";

// A keyboard shortcut, from the player core that handles the keys
export interface KeyLine {
  keys: string;
  description: string;
  // Does something for a level without a replay
  level: boolean;
  // Only for Elma: Across has no pictures, grass or gravity arrows
  elmaOnly: boolean;
  // Only for a two player replay
  multiplayer: boolean;
}

let lines: KeyLine[] | undefined;

// The keyboard shortcuts, once the engine is loaded (see loadEngine)
export function keyList(): KeyLine[] {
  lines ??= keyHelp().map((k) => {
    const line = {
      keys: k.keys,
      description: k.description,
      level: k.level,
      elmaOnly: k.elmaOnly,
      multiplayer: k.multiplayer,
    };
    k.free();
    return line;
  });
  return lines;
}
