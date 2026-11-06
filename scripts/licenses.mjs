// npm licence allow-list check and THIRD_PARTY_NOTICES generation (decision S6).
//
//   node scripts/licenses.mjs            check shipped packages, exit 1 on violations
//   node scripts/licenses.mjs --notices  also write THIRD_PARTY_NOTICES/npm.txt
//
// Only production dependencies of packages we ship are checked: dev tooling is
// never distributed. Keep ALLOWED identical to `deny.toml` and `about.toml`.

import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import { init } from "license-checker-rseidelsohn";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");

/** Workspace packages that end up in a product. */
const SHIPPED = ["extension", "ui", "apps/desktop/web"];

const ALLOWED = new Set([
  "MIT",
  "Apache-2.0",
  "BSD-2-Clause",
  "BSD-3-Clause",
  "ISC",
  "Zlib",
  "Unicode-3.0",
  "CC0-1.0",
  "MPL-2.0",
]);

/**
 * True if an SPDX expression is satisfied by the allow-list: every `AND` term
 * must be allowed, any one `OR` alternative is enough. Unknown or missing
 * licences are never allowed.
 */
function isAllowed(expression) {
  if (typeof expression !== "string") return false;
  const expr = expression.replace(/[()]/g, " ").trim();
  return expr
    .split(/\s+OR\s+/)
    .some((alt) => alt.split(/\s+AND\s+/).every((id) => ALLOWED.has(id.trim())));
}

const scan = promisify(init);

async function main() {
  const writeNotices = process.argv.includes("--notices");
  const violations = [];
  const notices = [];

  for (const pkg of SHIPPED) {
    let modules;
    try {
      modules = await scan({
        start: join(ROOT, pkg),
        production: true,
        excludePrivatePackages: true,
      });
    } catch (err) {
      // The checker throws when a package has no production dependencies yet.
      if (String(err).includes("No packages found")) continue;
      throw err;
    }

    for (const [id, info] of Object.entries(modules)) {
      const licenses = Array.isArray(info.licenses) ? info.licenses.join(" AND ") : info.licenses;
      if (!isAllowed(licenses)) violations.push(`${pkg}: ${id} is licensed "${licenses ?? "unknown"}"`);
      if (writeNotices) {
        const text = info.licenseFile ? readFileSync(info.licenseFile, "utf8").trim() : "(no licence file)";
        notices.push(`${id}\nLicence: ${licenses}\n${info.repository ?? ""}\n\n${text}\n`);
      }
    }
  }

  if (writeNotices) {
    const out = join(ROOT, "THIRD_PARTY_NOTICES", "npm.txt");
    mkdirSync(dirname(out), { recursive: true });
    const header = "Third-party notices (npm components)\n\n";
    writeFileSync(out, header + (notices.length ? notices.join("\n" + "-".repeat(72) + "\n\n") : "None.\n"));
    console.log(`wrote ${out} (${notices.length} packages)`);
  }

  if (violations.length) {
    console.error("Licences not on the allow-list (decision S6):\n  " + violations.join("\n  "));
    process.exit(1);
  }
  console.log(`npm licences ok (${SHIPPED.join(", ")})`);
}

await main();
