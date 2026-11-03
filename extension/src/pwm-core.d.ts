/**
 * Types of the WASM core module (`core/pwm_wasm.js`, built by `just wasm`),
 * which `tsconfig.json` maps `pwm-core` to. Only what the background uses;
 * `src/background/core.wasm.test.ts` checks them against the real build.
 */
import type { Core, CoreApprovalAsk } from "./background/core.ts";

export default function init(input: {
  module_or_path: string | URL;
}): Promise<unknown>;

export declare const WasmClient: {
  new (): Core;
  load(snapshot: Uint8Array): Core;
};

export function newId(): string;
export function breachRangeUrl(prefix: string): string;
export declare const WasmApprovalAsk: {
  new (email: string, platform: string): CoreApprovalAsk;
};
export function generatePassword(
  length: number,
  lowercase: boolean,
  uppercase: boolean,
  digits: boolean,
  symbols: boolean,
  avoidLookalikes: boolean,
): string;
export function generatePassphrase(
  words: number,
  separator: string,
  capitalize: boolean,
  addDigit: boolean,
): string;
export function passwordScore(password: string, context: string[]): number;
export function emergencyKit(
  email: string,
  secretKey: string,
  server: string | undefined,
  accountId: string,
  created: string,
  paper: string,
): Uint8Array;
