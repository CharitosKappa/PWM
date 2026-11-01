//! Weak, reused and breached (HIBP k-anonymity) password checks
//! (decision D22). Covers the password of every login item that is not
//! trashed and not empty. The core does no networking: platforms fetch HIBP
//! ranges through [`RangeFetcher`].
#![forbid(unsafe_code)]

mod breach;
mod error;
mod local;

pub use breach::{
    ADD_PADDING_HEADER, BreachCheck, BreachedPassword, HIBP_RANGE_URL, RangeFetcher, check_breaches,
};
pub use error::HealthError;
pub use local::{LocalReport, WeakPassword, check_local};

use pwm_vault::{Item, ItemId, ItemKind, Login};

/// Login items the report covers, in input order.
fn candidates<'a>(
    items: &'a [(ItemId, &'a Item)],
) -> impl Iterator<Item = (ItemId, &'a Item, &'a Login)> {
    items.iter().filter_map(|(id, item)| match &item.kind {
        ItemKind::Login(login) if !item.is_trashed() && !login.password.is_empty() => {
            Some((*id, *item, login))
        }
        _ => None,
    })
}
