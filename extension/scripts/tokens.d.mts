export function tokensCss(tokens: unknown): string;
export interface FontFile {
  family: string;
  file: string;
  weights: string;
  licence: string;
}
export function fontFiles(tokens: unknown): FontFile[];
export function copyFonts(repo: string, dist: string, tokens: unknown): void;
