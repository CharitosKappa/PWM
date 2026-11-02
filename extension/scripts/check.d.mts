export type Target = "chromium" | "firefox";
export const TARGETS: Target[];
export const PERMISSIONS: Record<Target, string[]>;
export const HOSTED_ORIGIN: string;
export const GECKO_ID: string;
export const OPTIONAL_HOSTS: string[];
export function checkManifest(
  manifest: unknown,
  version: string,
  target?: Target,
): string[];
export function remoteUrls(source: string): string[];
