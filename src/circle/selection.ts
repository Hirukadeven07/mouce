import type { Selection } from "../lib/types";

export interface Point {
  x: number;
  y: number;
}

/** Below this many pixels either way, a drawing is a click or a slip, not a circle. */
export const MIN_SELECTION_PX = 8;

/**
 * The box around a drawn path, as fractions of the `width` x `height` area it was drawn
 * on. Null when the box is too small to mean anything.
 */
export function selectionFrom(points: Point[], width: number, height: number): Selection | null {
  if (points.length === 0 || width <= 0 || height <= 0) return null;
  const xs = points.map((p) => Math.min(Math.max(p.x, 0), width));
  const ys = points.map((p) => Math.min(Math.max(p.y, 0), height));
  const [left, right] = [Math.min(...xs), Math.max(...xs)];
  const [top, bottom] = [Math.min(...ys), Math.max(...ys)];
  if (right - left < MIN_SELECTION_PX || bottom - top < MIN_SELECTION_PX) return null;
  return { x: left / width, y: top / height, w: (right - left) / width, h: (bottom - top) / height };
}

/** An SVG path through the points. */
export function pathData(points: Point[]): string {
  return points.map((p, i) => `${i === 0 ? "M" : "L"}${p.x} ${p.y}`).join(" ");
}
