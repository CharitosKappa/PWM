import { defineConfig } from "vitest/config";

// Tests over the real WASM core; they need `just wasm` first.
export default defineConfig({
  define: { PWM_TARGET: JSON.stringify("chromium"), PWM_TEST: "false" },
  test: {
    include: ["src/**/*.wasm.test.ts"],
  },
});
