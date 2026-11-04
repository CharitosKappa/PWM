// Checks on the built extension (D34, D35, D40, D48): only the listed
// permissions for each target, no remote code, nothing the store review
// would flag.

/**
 * The builds (D40, D48 §4): one for Chrome, Edge, Brave and Opera, one for
 * Firefox, and one that the macOS app carries into Safari.
 */
export const TARGETS = ["chromium", "firefox", "safari"];

/**
 * Firefox and Safari have no offscreen documents; their background page
 * writes the clipboard. Safari has no `idle` API either.
 */
export const PERMISSIONS = {
  chromium: [
    "storage",
    "alarms",
    "idle",
    "offscreen",
    "clipboardWrite",
    "activeTab",
    "scripting",
  ],
  firefox: [
    "storage",
    "alarms",
    "idle",
    "clipboardWrite",
    "activeTab",
    "scripting",
  ],
  safari: ["storage", "alarms", "clipboardWrite", "activeTab", "scripting"],
};

/** The Firefox add-on id, a placeholder until D1 names the product. */
export const GECKO_ID = "pwm@pwm.invalid";

/** Site access, only as optional permissions for the inline icon (D35 §1). */
export const OPTIONAL_HOSTS = ["https://*/*", "http://*/*"];

/** The inline icon frame is the only thing pages may load (D35 §3, §7). */
export const WEB_ACCESSIBLE = [
  {
    resources: ["inline.html", "inline.js", "inline.css", "tokens.css"],
    matches: OPTIONAL_HOSTS,
    use_dynamic_url: true,
  },
];
export const HOSTED_ORIGIN = "https://api.pwm.invalid";

// URLs allowed in bundled scripts: the hosted server, local self-hosting,
// XML namespaces and the optional site-access patterns (strings, never
// fetched).
const ALLOWED_URLS = [
  /^https?:\/\/\*\/\*$/,
  /^https:\/\/api\.pwm\.invalid\b/,
  /^http:\/\/(localhost|127\.0\.0\.1)\b/,
  /^http:\/\/www\.w3\.org\//,
  // The leak check's permission pattern (D44 §2); the core builds the URLs.
  /^https:\/\/api\.pwnedpasswords\.com\/\*$/,
];

/**
 * @param {any} manifest the parsed manifest.json
 * @param {string} version the package version
 * @param {"chromium" | "firefox" | "safari"} target
 * @returns {string[]} problems, empty when fine
 */
export function checkManifest(manifest, version, target = "chromium") {
  const problems = [];
  const same = (/** @type {unknown} */ a, /** @type {unknown} */ b) =>
    JSON.stringify(a) === JSON.stringify(b);
  if (manifest.manifest_version !== 3) problems.push("not Manifest V3");
  if (manifest.version !== version)
    problems.push(`version ${String(manifest.version)} is not ${version}`);
  if (!same(manifest.permissions, PERMISSIONS[target]))
    problems.push(`permissions are ${JSON.stringify(manifest.permissions)}`);
  if (!same(manifest.host_permissions, [`${HOSTED_ORIGIN}/*`]))
    problems.push("host permissions beyond the hosted API");
  if (!same(manifest.optional_host_permissions, OPTIONAL_HOSTS))
    problems.push("optional host permissions");
  // Firefox and Safari have no inline icon or save card (D40 §2, D48 §4):
  // pages load nothing.
  const accessible = target === "chromium" ? WEB_ACCESSIBLE : undefined;
  if (!same(manifest.web_accessible_resources, accessible))
    problems.push("web-accessible resources beyond the inline icon");
  // Content scripts are only injected on user action or registered at run
  // time after the user grants site access, never declared.
  for (const key of [
    "content_scripts",
    "externally_connectable",
    "key",
    "update_url",
  ]) {
    if (key in manifest) problems.push(`unexpected ${key}`);
  }
  const background = {
    chromium: { service_worker: "background.js", type: "module" },
    firefox: { scripts: ["background.js"], type: "module" },
    // An event page, as on Firefox; Safari wants it said.
    safari: { scripts: ["background.js"], type: "module", persistent: false },
  }[target];
  if (!same(manifest.background, background)) problems.push("background");
  if (target === "firefox") {
    const gecko = manifest.browser_specific_settings?.gecko;
    if (gecko?.id !== GECKO_ID) problems.push("gecko id");
    if (gecko?.strict_min_version !== "128.0")
      problems.push("gecko strict_min_version");
  } else if (target === "safari") {
    const safari = manifest.browser_specific_settings?.safari;
    if (safari?.strict_min_version !== "17.0")
      problems.push("safari strict_min_version");
    if (manifest.browser_specific_settings?.gecko !== undefined)
      problems.push("gecko settings in the Safari build");
  } else if (manifest.minimum_chrome_version !== "127") {
    problems.push("minimum_chrome_version");
  }
  const csp = manifest.content_security_policy?.extension_pages ?? "";
  if (!csp.includes("script-src 'self' 'wasm-unsafe-eval';"))
    problems.push("CSP script-src");
  if (/https?:|\*/.test(csp)) problems.push("CSP allows remote sources");
  return problems;
}

/**
 * Remote URLs in a bundled script that are not on the allow-list.
 * @param {string} source
 * @returns {string[]}
 */
export function remoteUrls(source) {
  const found = source.match(/\b(?:https?|wss?):\/\/[^\s"'`)]+/g) ?? [];
  return found.filter((url) => !ALLOWED_URLS.some((ok) => ok.test(url)));
}
