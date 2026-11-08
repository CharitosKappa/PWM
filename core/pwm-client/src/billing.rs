//! Plans and billing (D6): the account's plan and where it was bought,
//! Stripe's Checkout, cancel and resume, and handing a store purchase to
//! the server. Each is one authenticated call; the server reads the
//! subscription from its provider and answers the plan.

use pwm_store::Store;
use pwm_sync::Method;
use serde::Deserialize;

use crate::client::{Running, Step};
use crate::{Client, ClientError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BillingCall {
    Status,
    Checkout,
    Cancel,
    Resume,
    Refund,
    StorePurchase,
}

/// The account's plan, as the server has it (D6 §7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BillingInfo {
    /// `free`, `premium`, `family` or `teams`.
    pub plan: String,
    /// When the plan stops counting if nothing renews (Unix ms, D73 §2);
    /// `None` when it has no end.
    pub plan_until_ms: Option<i64>,
    /// The subscription that gives the plan, if any.
    pub subscription: Option<SubscriptionInfo>,
    /// The providers that sell plans on this server (`stripe`,
    /// `app_store`, `play`). Empty on a server without billing, where every
    /// account is Premium.
    pub sells: Vec<String>,
    /// The account's billing token, for a store purchase: Apple's
    /// `appAccountToken`, Play's `obfuscatedAccountId`. Only from
    /// [`Client::start_billing_status`].
    pub token: Option<String>,
}

/// Times are Unix milliseconds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubscriptionInfo {
    /// `stripe`, `app_store` or `play`: where to manage it.
    pub provider: String,
    pub plan: String,
    pub status: String,
    /// The end of the paid period.
    pub until_ms: i64,
    /// It ends at `until_ms` instead of renewing.
    pub canceling: bool,
    pub price_locked_until_ms: Option<i64>,
    /// Until when [`Client::start_refund`] works (Stripe and Play, 30 days
    /// after the last charge, D72 §1); `None` when it doesn't.
    pub refundable_until_ms: Option<i64>,
    /// Other subscriptions that count too, bought by mistake: where to
    /// cancel them.
    pub also: Vec<String>,
}

#[derive(Deserialize)]
struct SummaryJson {
    plan: String,
    #[serde(default)]
    plan_until: Option<i64>,
    subscription: Option<SubscriptionJson>,
    billing: Vec<String>,
    #[serde(default)]
    token: Option<String>,
}

#[derive(Deserialize)]
struct SubscriptionJson {
    provider: String,
    plan: String,
    status: String,
    until: i64,
    canceling: bool,
    price_locked_until: Option<i64>,
    #[serde(default)]
    refundable_until: Option<i64>,
    also: Vec<String>,
}

#[derive(Deserialize)]
struct CheckoutJson {
    url: String,
}

#[derive(Deserialize)]
struct ErrorJson {
    error: String,
}

fn read_summary(body: &[u8]) -> Result<BillingInfo, ClientError> {
    let s: SummaryJson = serde_json::from_slice(body).map_err(|_| ClientError::Protocol)?;
    Ok(BillingInfo {
        plan: s.plan,
        plan_until_ms: s.plan_until,
        subscription: s.subscription.map(|j| SubscriptionInfo {
            provider: j.provider,
            plan: j.plan,
            status: j.status,
            until_ms: j.until,
            canceling: j.canceling,
            price_locked_until_ms: j.price_locked_until,
            refundable_until_ms: j.refundable_until,
            also: j.also,
        }),
        sells: s.billing,
        token: s.token,
    })
}

/// The server's error codes for billing, kept as they are (D6 §7).
const CODES: &[&str] = &[
    "billing_unavailable",
    "purchase_not_yours",
    "purchase_pending",
    "already_subscribed",
    "managed_elsewhere",
    "refund_window_passed",
];

fn billing_error(status: u16, body: &[u8]) -> ClientError {
    let code = serde_json::from_slice::<ErrorJson>(body)
        .ok()
        .and_then(|e| CODES.iter().find(|c| **c == e.error).copied());
    match (status, code) {
        (_, Some(code)) => ClientError::Billing(code),
        (404, None) => ClientError::Billing("no_subscription"),
        (status, None) => ClientError::Server(status),
    }
}

impl<S: Store> Client<S> {
    fn start_billing_call(
        &mut self,
        call: BillingCall,
        method: Method,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Result<Step, ClientError> {
        self.idle()?;
        let step = self.engine()?.start_call(
            method,
            path.to_owned(),
            body.map(|b| b.to_string().into_bytes()),
        )?;
        self.running = Running::Billing(call);
        self.map_engine_step(step)
    }

    /// The plan, its subscription, what this server sells and the billing
    /// token; ends with `Billing`.
    pub fn start_billing_status(&mut self) -> Result<Step, ClientError> {
        self.start_billing_call(BillingCall::Status, Method::Get, "/v1/billing", None)
    }

    /// A Stripe Checkout page for `plan` (`premium` or `family`), for the
    /// app to open in the browser; ends with `CheckoutUrl`. The plan shows
    /// once Stripe tells the server it was paid.
    pub fn start_checkout(&mut self, plan: &str) -> Result<Step, ClientError> {
        self.start_billing_call(
            BillingCall::Checkout,
            Method::Post,
            "/v1/billing/stripe/checkout",
            Some(serde_json::json!({ "plan": plan })),
        )
    }

    /// Cancels the Stripe subscription at the end of its period (one click,
    /// PRICING.md §4); ends with `Billing`. A store's answers
    /// `managed_elsewhere`.
    pub fn start_cancel_subscription(&mut self) -> Result<Step, ClientError> {
        self.start_billing_call(
            BillingCall::Cancel,
            Method::Post,
            "/v1/billing/stripe/cancel",
            None,
        )
    }

    /// Lets a canceling Stripe subscription renew again; ends with `Billing`.
    pub fn start_resume_subscription(&mut self) -> Result<Step, ClientError> {
        self.start_billing_call(
            BillingCall::Resume,
            Method::Post,
            "/v1/billing/stripe/resume",
            None,
        )
    }

    /// Refunds the unused part of the last charge and ends the plan now
    /// (PRICING.md §5, D72 §1), for a subscription bought with `provider`
    /// (`stripe` or `play`); ends with `Billing`. After 30 days,
    /// `refund_window_passed`. Only Apple refunds an App Store purchase.
    pub fn start_refund(&mut self, provider: &str) -> Result<Step, ClientError> {
        let path = match provider {
            "stripe" => "/v1/billing/stripe/refund",
            "play" => "/v1/billing/play/refund",
            _ => return Err(ClientError::Billing("managed_elsewhere")),
        };
        self.start_billing_call(BillingCall::Refund, Method::Post, path, None)
    }

    /// Hands an App Store purchase to the server right after StoreKit
    /// finished it; ends with `Billing`.
    pub fn start_apple_purchase(&mut self, transaction_id: &str) -> Result<Step, ClientError> {
        self.start_billing_call(
            BillingCall::StorePurchase,
            Method::Post,
            "/v1/billing/apple/transactions",
            Some(serde_json::json!({ "transaction_id": transaction_id })),
        )
    }

    /// Hands a Play purchase to the server right after Play Billing
    /// finished it; ends with `Billing`. The server acknowledges it.
    pub fn start_play_purchase(
        &mut self,
        product_id: &str,
        purchase_token: &str,
    ) -> Result<Step, ClientError> {
        self.start_billing_call(
            BillingCall::StorePurchase,
            Method::Post,
            "/v1/billing/play/purchases",
            Some(serde_json::json!({ "product_id": product_id, "purchase_token": purchase_token })),
        )
    }

    pub(crate) fn billing_answer(
        &mut self,
        call: BillingCall,
        status: u16,
        body: &[u8],
    ) -> Result<crate::Outcome, ClientError> {
        match (call, status) {
            (BillingCall::Checkout, 200) => {
                let checkout: CheckoutJson =
                    serde_json::from_slice(body).map_err(|_| ClientError::Protocol)?;
                if !checkout.url.starts_with("https://") && !checkout.url.starts_with("http://") {
                    return Err(ClientError::Protocol);
                }
                Ok(crate::Outcome::CheckoutUrl(checkout.url))
            }
            (_, 200) => {
                let info = read_summary(body)?;
                self.remember_plan(&info.plan, info.plan_until_ms);
                Ok(crate::Outcome::Billing(info))
            }
            (_, status) => Err(billing_error(status, body)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summaries_are_read() {
        let body = br#"{"plan":"premium","billing":["stripe"],"token":"t",
            "subscription":{"provider":"stripe","plan":"premium","status":"active","until":5,
            "canceling":true,"price_locked_until":null,"also":["app_store"]}}"#;
        let info = read_summary(body).unwrap();
        assert_eq!(info.plan, "premium");
        assert_eq!(info.sells, ["stripe"]);
        assert_eq!(info.token.as_deref(), Some("t"));
        let sub = info.subscription.unwrap();
        assert_eq!((sub.until_ms, sub.canceling), (5, true));
        assert_eq!(sub.also, ["app_store"]);
        assert_eq!(info.plan_until_ms, None, "a server older than D73");
        let with_end = br#"{"plan":"premium","plan_until":7,"billing":[],"subscription":null}"#;
        assert_eq!(read_summary(with_end).unwrap().plan_until_ms, Some(7));
        assert!(matches!(read_summary(b"{}"), Err(ClientError::Protocol)));
    }

    #[test]
    fn errors_keep_the_server_code() {
        let code = |status, body: &[u8]| billing_error(status, body).code();
        assert_eq!(
            code(409, br#"{"error":"managed_elsewhere"}"#),
            "managed_elsewhere"
        );
        assert_eq!(
            code(403, br#"{"error":"purchase_not_yours"}"#),
            "purchase_not_yours"
        );
        assert_eq!(code(404, br#"{"error":"not_found"}"#), "no_subscription");
        assert_eq!(code(409, br#"{"error":"something_new"}"#), "server");
    }
}
