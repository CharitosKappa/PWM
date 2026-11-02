import { defineConfig } from "vitest/config";

export default defineConfig({
  // What scripts/build.mjs defines; the tests run as the Chromium build.
  define: { PWM_TARGET: JSON.stringify("chromium"), PWM_TEST: "false" },
  test: {
    include: ["src/**/*.test.ts", "scripts/**/*.test.ts"],
    // Needs the WASM build: `pnpm test:wasm` (run by `just wasm` and CI).
    exclude: ["src/**/*.wasm.test.ts"],
  },
});
