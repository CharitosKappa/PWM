/**
 * The plan (D6): what the account has and where it's managed. On the
 * desktop and in the extensions plans are bought with Stripe's Checkout in
 * the browser, and a Stripe plan is canceled or resumed with one click
 * (PRICING.md §4). A plan bought in the App Store or Google Play is managed
 * there.
 */
import { useEffect, useState } from "preact/hooks";
import type { JSX } from "preact";
import type { PlanProvider, PlanView } from "./protocol.ts";
import { describe, send } from "./api.ts";
import { Notice } from "./ui.tsx";

const PLAN_NAMES: Record<string, string> = {
  free: "Free",
  premium: "Premium",
  family: "Family",
  teams: "Teams",
};

/** Where to manage a plan bought with each provider. */
const WHERE: Record<PlanProvider, string> = {
  stripe: "on the web, from PWM's Settings on a computer",
  app_store: "in the App Store, on your iPhone or iPad",
  play: "in Google Play, on your Android device",
};

export function planName(plan: string): string {
  return PLAN_NAMES[plan] ?? plan;
}

function day(ms: number): string {
  return new Date(ms).toLocaleDateString(undefined, {
    year: "numeric",
    month: "long",
    day: "numeric",
  });
}

export function PlanSetting(): JSX.Element {
  const [plan, setPlan] = useState<PlanView | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [checkingOut, setCheckingOut] = useState(false);

  const load = async (): Promise<void> => {
    setError(null);
    try {
      setPlan(await send({ kind: "plan" }));
    } catch (err) {
      setError(describe(err));
    }
  };

  useEffect(() => {
    void load();
  }, []);

  const act = async (work: () => Promise<void>): Promise<void> => {
    setBusy(true);
    setError(null);
    try {
      await work();
    } catch (err) {
      setError(describe(err));
    } finally {
      setBusy(false);
    }
  };

  const [refunded, setRefunded] = useState(false);

  const checkout = (): Promise<void> =>
    act(async () => {
      const url = await send({ kind: "checkout", plan: "premium" });
      // The desktop opens it in the default browser, the extension in a tab.
      window.open(url, "_blank", "noopener");
      setCheckingOut(true);
    });

  if (plan === null) {
    return (
      <section class="stack" id="plan">
        <span class="label">Plan</span>
        {error ? (
          <Notice kind="danger">{error}</Notice>
        ) : (
          <span class="muted">Loading…</span>
        )}
      </section>
    );
  }

  const sub = plan.subscription;
  return (
    <section class="stack" id="plan">
      <span class="label">Plan</span>
      <strong id="plan-name">{planName(plan.plan)}</strong>

      {plan.sells.length === 0 && (
        <span class="caption muted">Your server includes every feature.</span>
      )}

      {sub !== null && (
        <span class="caption muted" id="plan-period">
          {sub.canceling
            ? `Ends on ${day(sub.untilMs)}. You keep everything until then.`
            : `Renews on ${day(sub.untilMs)}.`}
          {sub.provider !== "stripe" && ` Managed ${WHERE[sub.provider]}.`}
        </span>
      )}

      {sub?.provider === "stripe" &&
        (sub.canceling ? (
          <button
            id="resume-plan"
            disabled={busy}
            onClick={() =>
              void act(async () => {
                setPlan(await send({ kind: "resumePlan" }));
              })
            }
          >
            Keep my plan
          </button>
        ) : (
          <button
            id="cancel-plan"
            disabled={busy}
            onClick={() =>
              void act(async () => {
                setPlan(await send({ kind: "cancelPlan" }));
              })
            }
          >
            Cancel plan
          </button>
        ))}

      {sub !== null &&
        sub.refundableUntilMs !== null &&
        (sub.provider === "stripe" || sub.provider === "play") && (
          <>
            <button
              id="refund-plan"
              disabled={busy}
              onClick={() =>
                void act(async () => {
                  setPlan(await send({ kind: "refundPlan" }));
                  setRefunded(true);
                })
              }
            >
              Request a refund
            </button>
            <span class="caption muted">
              Ends your plan now and pays back the days you haven't used. Until{" "}
              {day(sub.refundableUntilMs)}.
            </span>
          </>
        )}

      {sub?.provider === "app_store" && (
        // Plain text: the extensions ship no remote URLs (D40).
        <span class="caption muted">
          To ask for a refund, use Apple's Report a Problem page at
          reportaproblem.apple.com.
        </span>
      )}

      {refunded && sub === null && (
        <Notice kind="info">Your plan ended. The refund is on its way.</Notice>
      )}

      {sub !== null && sub.also.length > 0 && (
        <Notice kind="info">
          You also pay for PWM {sub.also.map((p) => WHERE[p]).join(" and ")}.
          Cancel it there so you're not charged twice.
        </Notice>
      )}

      {sub === null &&
        plan.plan === "free" &&
        plan.sells.includes("stripe") && (
          <>
            <button
              id="get-premium"
              disabled={busy}
              onClick={() => void checkout()}
            >
              Get Premium
            </button>
            {checkingOut && (
              <span class="caption muted">
                Finish in your browser, then{" "}
                <button class="link" onClick={() => void load()}>
                  check again
                </button>
                .
              </span>
            )}
          </>
        )}

      {error && <Notice kind="danger">{error}</Notice>}
    </section>
  );
}
