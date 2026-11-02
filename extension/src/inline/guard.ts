/**
 * When the inline frame takes a click (D35 §3): a trusted click, after the
 * frame was reported visible (IntersectionObserver v2: not covered,
 * transparent, filtered or transformed, in any ancestor frame) for at least
 * 500 ms without a break, and 500 ms after the view last changed.
 */
export const MIN_VISIBLE_MS = 500;

export class ClickGuard {
  private visibleSince: number | null = null;
  private shownAt = 0;

  constructor(private readonly now: () => number) {}

  /** An IntersectionObserver v2 report; `undefined` where v2 is missing. */
  observed(isVisible: boolean | undefined): void {
    if (isVisible === true) this.visibleSince ??= this.now();
    else this.visibleSince = null;
  }

  /** The content changed (icon to menu): the new view must be seen first. */
  viewChanged(): void {
    this.shownAt = this.now();
  }

  accepts(event: { isTrusted: boolean }): boolean {
    if (!event.isTrusted || this.visibleSince === null) return false;
    const since = Math.max(this.visibleSince, this.shownAt);
    return this.now() - since >= MIN_VISIBLE_MS;
  }
}
