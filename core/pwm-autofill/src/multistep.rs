//! Multi-step logins (D33 §8): per tab, the site, the item chosen and the
//! step reached. Never values. Forgotten after 5 minutes, on a site change
//! or when the tab closes.

use std::collections::HashMap;

use url::Url;

use crate::AutofillError;
use crate::matching::site_of;

const TTL_MS: i64 = 5 * 60 * 1000;
/// Tabs remembered at most; the oldest go first.
const MAX_TABS: usize = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepDone {
    Username,
    Password,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NextStep {
    Password,
    Totp,
}

struct TabState {
    site: String,
    item: String,
    done: StepDone,
    at_ms: i64,
}

#[derive(Default)]
pub struct MultiStep {
    tabs: HashMap<String, TabState>,
}

fn web_site(page_url: &str) -> Result<String, AutofillError> {
    let url = Url::parse(page_url).map_err(|_| AutofillError::InvalidUrl)?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(AutofillError::InvalidUrl);
    }
    site_of(&url).ok_or(AutofillError::InvalidUrl)
}

impl MultiStep {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    /// Notes that `item` filled step `done` on this tab's page.
    pub fn record(
        &mut self,
        tab: &str,
        page_url: &str,
        item: &str,
        done: StepDone,
        now_ms: i64,
    ) -> Result<(), AutofillError> {
        let site = web_site(page_url)?;
        self.tabs.insert(
            tab.to_owned(),
            TabState {
                site,
                item: item.to_owned(),
                done,
                at_ms: now_ms,
            },
        );
        if self.tabs.len() > MAX_TABS
            && let Some(oldest) = self
                .tabs
                .iter()
                .min_by_key(|(_, s)| s.at_ms)
                .map(|(k, _)| k.clone())
        {
            self.tabs.remove(&oldest);
        }
        Ok(())
    }

    /// The item and step to offer next on this tab's page, if a login is in
    /// progress here. Expired or other-site state is dropped.
    pub fn next(&mut self, tab: &str, page_url: &str, now_ms: i64) -> Option<(String, NextStep)> {
        let site = web_site(page_url).ok();
        let state = self.tabs.get(tab)?;
        if now_ms - state.at_ms >= TTL_MS || site.as_deref() != Some(state.site.as_str()) {
            self.tabs.remove(tab);
            return None;
        }
        let next = match state.done {
            StepDone::Username => NextStep::Password,
            StepDone::Password => NextStep::Totp,
        };
        Some((state.item.clone(), next))
    }

    /// The tab closed.
    pub fn forget(&mut self, tab: &str) {
        self.tabs.remove(tab);
    }
}
