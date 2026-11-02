/** The Emergency Kit's paper and date, from the browser's locale and clock (D42). */

export type Paper = "a4" | "letter";

/** Regions that print on US Letter; everywhere else uses A4. */
const LETTER_REGIONS = new Set([
  "US",
  "CA",
  "MX",
  "PH",
  "CL",
  "CO",
  "VE",
  "GT",
  "CR",
  "PA",
  "DO",
  "SV",
  "NI",
  "PR",
  "BZ",
]);

export function paperFor(locale: string): Paper {
  try {
    const region = new Intl.Locale(locale).maximize().region;
    return region !== undefined && LETTER_REGIONS.has(region) ? "letter" : "a4";
  } catch {
    return "a4";
  }
}

/** `YYYY-MM-DD` in the local time zone. */
export function localDate(ms: number): string {
  const d = new Date(ms);
  const pad = (n: number): string => String(n).padStart(2, "0");
  return `${String(d.getFullYear())}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}
