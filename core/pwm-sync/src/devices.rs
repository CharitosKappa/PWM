//! The account's devices (D26, D43 §6): list, rename and revoke, run by the
//! engine so they share its token refresh and revoked-device handling.

use std::fmt;

use pwm_crypto::{Envelope, Id};
use serde::{Deserialize, Serialize};

use crate::http::{HttpRequest, Method};
use crate::wire::{b64, id_text, parse_id, unb64};

/// A device request.
pub enum DeviceTask {
    List,
    /// `name_sealed` is the new name sealed for this device (D26 format).
    Rename {
        device_id: Id,
        name_sealed: Envelope,
    },
    Revoke {
        device_id: Id,
    },
}

impl DeviceTask {
    pub(crate) fn request(&self) -> HttpRequest {
        match self {
            Self::List => HttpRequest::new(Method::Get, "/v1/devices".to_owned()),
            Self::Rename {
                device_id,
                name_sealed,
            } => {
                let body = NameJson {
                    name_sealed: b64(&name_sealed.to_bytes()),
                };
                HttpRequest::new(Method::Patch, path(device_id))
                    .json(serde_json::to_vec(&body).expect("a string field always serializes"))
            }
            Self::Revoke { device_id } => HttpRequest::new(Method::Delete, path(device_id)),
        }
    }
}

fn path(device_id: &Id) -> String {
    format!("/v1/devices/{}", id_text(device_id))
}

/// One of the account's devices as the server lists it; the name is still
/// sealed. Times are Unix milliseconds.
#[derive(Clone)]
pub struct DeviceRecord {
    pub id: Id,
    pub platform: String,
    /// `None` if the device has no name, or the server's value is not an envelope.
    pub name_sealed: Option<Envelope>,
    pub created_at: i64,
    pub last_seen_at: i64,
    pub revoked_at: Option<i64>,
    /// The device making the request.
    pub current: bool,
}

impl fmt::Debug for DeviceRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeviceRecord")
            .field("platform", &self.platform)
            .field("current", &self.current)
            .finish_non_exhaustive()
    }
}

#[derive(Serialize)]
struct NameJson {
    name_sealed: String,
}

#[derive(Deserialize)]
struct DeviceJson {
    id: String,
    platform: String,
    #[serde(default)]
    name_sealed: Option<String>,
    created_at: i64,
    last_seen_at: i64,
    #[serde(default)]
    revoked_at: Option<i64>,
    current: bool,
}

/// `GET /v1/devices`, in the server's order.
pub(crate) fn device_list(body: &[u8]) -> Option<Vec<DeviceRecord>> {
    let list: Vec<DeviceJson> = serde_json::from_slice(body).ok()?;
    list.into_iter()
        .map(|d| {
            Some(DeviceRecord {
                id: *parse_id(&d.id)?.as_bytes(),
                platform: d.platform,
                name_sealed: d
                    .name_sealed
                    .as_deref()
                    .and_then(unb64)
                    .and_then(|bytes| Envelope::from_bytes(&bytes).ok()),
                created_at: d.created_at,
                last_seen_at: d.last_seen_at,
                revoked_at: d.revoked_at,
                current: d.current,
            })
        })
        .collect()
}
