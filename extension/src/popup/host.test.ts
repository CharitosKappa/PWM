import { expect, test } from "vitest";
import { biometricName } from "./host.ts";

test("each computer's unlock through the PWM app (D48, D69, D71)", () => {
  expect(
    biometricName(
      "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36",
    ),
  ).toBe("Touch ID");
  expect(
    biometricName("Mozilla/5.0 (Windows NT 10.0; Win64; x64) Chrome/141.0"),
  ).toBe("Windows Hello");
  expect(
    biometricName("Mozilla/5.0 (X11; Linux x86_64; rv:140.0) Firefox/140.0"),
  ).toBe("Linux login");
  expect(
    biometricName("Mozilla/5.0 (Android 15; Mobile; rv:140.0) Firefox/140.0"),
  ).toBeNull();
  expect(
    biometricName("Mozilla/5.0 (Linux; Android 15) Chrome/141.0 Mobile"),
  ).toBeNull();
  expect(
    biometricName("Mozilla/5.0 (X11; CrOS x86_64 16000.0.0) Chrome/141.0"),
  ).toBeNull();
});
