import { defineConfig } from "vitest/config";

// Tests over the real WASM core; they need `just wasm` first.
export default defineConfig({
  test: {
    include: ["src/**/*.wasm.test.ts"],
  },
});
