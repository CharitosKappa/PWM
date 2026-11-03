import type { CoreFunctions } from "./core.ts";
import type * as Wasm from "pwm-core";

/** The core's free functions with the extension's generator defaults. */
export function coreFunctions(wasm: typeof Wasm): CoreFunctions {
  return {
    newCore: () => new wasm.WasmClient(),
    loadCore: (snapshot) => wasm.WasmClient.load(snapshot),
    newId: () => wasm.newId(),
    generatePassword: (length) =>
      wasm.generatePassword(length, true, true, true, true, true),
    generatePassphrase: (words) =>
      wasm.generatePassphrase(words, "-", false, true),
    passwordScore: (password, context) => wasm.passwordScore(password, context),
    emergencyKit: (email, secretKey, server, accountId, created, paper) =>
      wasm.emergencyKit(email, secretKey, server, accountId, created, paper),
    breachRangeUrl: (prefix) => wasm.breachRangeUrl(prefix),
    newApprovalAsk: (email) => new wasm.WasmApprovalAsk(email, "extension"),
  };
}
