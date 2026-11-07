//! The plan on this device (D73 §2): what the server last said, from the
//! headers of every sync and from billing replies, and what it allows now.
//! The gate is honest about what Premium includes; it isn't protection, as
//! the secrets are the user's own.

use pwm_store::{PlanRecord, Store};

use crate::{Client, ClientError};

impl<S: Store> Client<S> {
    /// The plan that counts now: the stored plan while its end is absent or
    /// ahead, `free` after that until a sync says otherwise. `None` when the
    /// server hasn't said yet (signed in before D73, or an older server),
    /// which the gates treat as Premium.
    pub fn plan_now(&self, now_ms: i64) -> Result<Option<String>, ClientError> {
        Ok(self.store().plan()?.map(|p| plan_at(&p, now_ms).to_owned()))
    }

    /// Whether the plan now includes one-time codes (Premium, Family,
    /// Teams, or not known yet).
    pub fn codes_included(&self, now_ms: i64) -> Result<bool, ClientError> {
        Ok(self.plan_now(now_ms)?.is_none_or(|p| p != "free"))
    }

    pub(crate) fn remember_plan(&mut self, plan: &str, until_ms: Option<i64>) {
        let record = PlanRecord {
            plan: plan.to_owned(),
            until_ms,
        };
        // A plan the cache can't keep only means the next sync tries again.
        if let Ok(engine) = self.engine() {
            let _ = engine.set_plan(&record);
        }
    }
}

fn plan_at(plan: &PlanRecord, now_ms: i64) -> &str {
    match plan.until_ms {
        Some(until) if until <= now_ms => "free",
        _ => &plan.plan,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plan_counts_until_its_end() {
        let premium = PlanRecord {
            plan: "premium".to_owned(),
            until_ms: Some(1_000),
        };
        assert_eq!(plan_at(&premium, 999), "premium");
        assert_eq!(plan_at(&premium, 1_000), "free");
        let forever = PlanRecord {
            plan: "family".to_owned(),
            until_ms: None,
        };
        assert_eq!(plan_at(&forever, i64::MAX), "family");
    }
}
