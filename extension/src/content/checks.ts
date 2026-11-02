/**
 * The checks before every fill (D35 §4), as pure functions over what the
 * content script measured. Any failure means nothing is filled.
 */

export interface Rect {
  left: number;
  top: number;
  width: number;
  height: number;
}

export interface StyleInfo {
  display: string;
  visibility: string;
  opacity: string;
  filter: string;
  clipPath: string;
}

export interface FieldState {
  connected: boolean;
  disabled: boolean;
  readOnly: boolean;
  rect: Rect;
  viewport: { width: number; height: number };
  /** The field's computed style, then each ancestor's up to the root. */
  styles: StyleInfo[];
  /** For each sample point: is the field (or its own label) on top there? */
  onTop: boolean[];
  /** IntersectionObserver v2 `isVisible`; undefined where the browser lacks it. */
  visibleV2: boolean | undefined;
  topFrame: boolean;
}

export type Problem =
  | "gone"
  | "disabled"
  | "read_only"
  | "no_size"
  | "off_screen"
  | "hidden"
  | "transparent"
  | "covered"
  | "not_visible"
  | "frame_unchecked";

/** The points sampled for covering: the centre and two inner corners. */
export function samplePoints(rect: Rect): [number, number][] {
  const inset = Math.min(2, rect.width / 4, rect.height / 4);
  return [
    [rect.left + rect.width / 2, rect.top + rect.height / 2],
    [rect.left + inset, rect.top + inset],
    [rect.left + rect.width - inset, rect.top + rect.height - inset],
  ];
}

export function inViewport(
  rect: Rect,
  viewport: { width: number; height: number },
): boolean {
  return (
    rect.left >= 0 &&
    rect.top >= 0 &&
    rect.left + rect.width <= viewport.width &&
    rect.top + rect.height <= viewport.height
  );
}

/** Why the field must not be filled, or null when every check passes. */
export function fieldProblem(state: FieldState): Problem | null {
  if (!state.connected) return "gone";
  if (state.disabled) return "disabled";
  if (state.readOnly) return "read_only";
  if (state.rect.width < 1 || state.rect.height < 1) return "no_size";
  if (!inViewport(state.rect, state.viewport)) return "off_screen";
  for (const style of state.styles) {
    if (
      style.display === "none" ||
      (style.visibility !== "visible" && style.visibility !== "")
    )
      return "hidden";
    if (style.clipPath !== "none" && style.clipPath !== "") return "hidden";
    if (Number(style.opacity) !== 1) return "transparent";
    if (style.filter !== "none" && style.filter !== "") return "transparent";
  }
  if (state.onTop.length === 0 || state.onTop.some((top) => !top))
    return "covered";
  if (state.visibleV2 === false) return "not_visible";
  // Without v2 a frame cannot tell whether its parent covers it.
  if (state.visibleV2 === undefined && !state.topFrame)
    return "frame_unchecked";
  return null;
}
