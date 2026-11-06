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

/**
 * What a sample point's topmost element is: the field (or its own label),
 * one of the field's ancestors (the point is where the field doesn't paint,
 * such as outside a rounded corner; D63 §2), a small element wholly inside
 * the field's box such as a show-password button (D65 §3), or anything
 * else.
 */
export type Hit = "field" | "ancestor" | "inside" | "other";

export interface FieldState {
  connected: boolean;
  disabled: boolean;
  readOnly: boolean;
  rect: Rect;
  viewport: { width: number; height: number };
  /** The field's computed style, then each ancestor's up to the root. */
  styles: StyleInfo[];
  /** For each sample point, what is on top there. */
  onTop: Hit[];
  /** IntersectionObserver v2 `isVisible`; undefined where the browser lacks it. */
  visibleV2: boolean | undefined;
  topFrame: boolean;
}

/** Every check's name: the only details a fill error may carry (D63 §3). */
export const PROBLEMS = [
  "gone",
  "disabled",
  "read_only",
  "no_size",
  "off_screen",
  "hidden",
  "transparent",
  "covered",
  "not_visible",
  "frame_unchecked",
] as const;

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

/**
 * Whether `other` lies wholly inside the field's box and covers at most a
 * quarter of it (D65 §3): how show-password and clear buttons sit.
 */
export function insideField(field: Rect, other: Rect): boolean {
  if (other.width <= 0 || other.height <= 0) return false;
  const within =
    other.left >= field.left &&
    other.top >= field.top &&
    other.left + other.width <= field.left + field.width &&
    other.top + other.height <= field.top + field.height;
  return (
    within && other.width * other.height <= (field.width * field.height) / 4
  );
}

/** An element's accessibility state, for `unreachable`. */
export interface Reach {
  ariaHidden: string | null;
  inert: boolean;
}

/**
 * Whether people can't reach a field (D65 §1): it, or an ancestor, is
 * `aria-hidden="true"` or `inert`. `chain` is the field, then each ancestor.
 */
export function unreachable(chain: Reach[]): boolean {
  return chain.some(
    (node) => node.inert || node.ariaHidden?.toLowerCase() === "true",
  );
}

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
  // The centre must be the field; a corner may hold a button inside it.
  if (
    state.onTop.length === 0 ||
    state.onTop[0] === "inside" ||
    state.onTop.some((hit) => hit === "other")
  )
    return "covered";
  // v2 is for a frame its parent page may hide or cover (D63 §1). In the top
  // frame, anything over the field is the matched site's own page, and the
  // checks above decide.
  if (state.topFrame) return null;
  if (state.visibleV2 === false) return "not_visible";
  // Without v2 a frame cannot tell whether its parent covers it.
  if (state.visibleV2 === undefined) return "frame_unchecked";
  return null;
}
