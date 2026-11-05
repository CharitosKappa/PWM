/** A failure the popup can show: `code` is stable (core codes, D31, or the background's own). */
export class ServiceError extends Error {
  /** `detail`: a known code saying more, such as which fill check failed. */
  constructor(
    readonly code: string,
    readonly detail?: string,
  ) {
    super(code);
  }
}
