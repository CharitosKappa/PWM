// Checks on the built extension (D34, D35): only the listed permissions,
// no remote code, nothing the store review would flag.

export const PERMISSIONS = [
  "storage",
  "alarms",
  "idle",
  "offscreen",
  "clipboardWrite",
  "activeTab",
  "scripting",
];

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
];

/**
 * @param {any} manifest the parsed manifest.json
 * @param {string} version the package version
 * @returns {string[]} problems, empty when fine
 */
export function checkManifest(manifest, version) {
  const problems = [];
  const same = (/** @type {unknown} */ a, /** @type {unknown} */ b) =>
    JSON.stringify(a) === JSON.stringify(b);
  if (manifest.manifest_version !== 3) problems.push("not Manifest V3");
  if (manifest.version !== version)
    problems.push(`version ${String(manifest.version)} is not ${version}`);
  if (!same(manifest.permissions, PERMISSIONS))
    problems.push(`permissions are ${JSON.stringify(manifest.permissions)}`);
  if (!same(manifest.host_permissions, [`${HOSTED_ORIGIN}/*`]))
    problems.push("host permissions beyond the hosted API");
  if (!same(manifest.optional_host_permissions, OPTIONAL_HOSTS))
    problems.push("optional host permissions");
  if (!same(manifest.web_accessible_resources, WEB_ACCESSIBLE))
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
  if (
    !same(manifest.background, {
      service_worker: "background.js",
      type: "module",
    })
  )
    problems.push("background");
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
