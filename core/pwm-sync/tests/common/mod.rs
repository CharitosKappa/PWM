//! An in-memory server that follows the D27 rules, fault injection, and a
//! driver that runs the sans-IO engine against it (D28).
#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use pwm_crypto::{AccountKeyset, Envelope, Id, Secret, SymmetricKey};
use pwm_store::{ItemRecord, SqliteStore, Store, SyncState};
use pwm_sync::{
    HttpRequest, HttpResponse, Method, Step, SyncEngine, SyncOutcome, SyncReport, Tokens,
};
use pwm_vault::{Item, ItemId, ItemKind};
use serde_json::{Value, json};

pub type Engine = SyncEngine<SqliteStore>;

pub fn b64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

fn unb64(text: &str) -> Option<Vec<u8>> {
    URL_SAFE_NO_PAD.decode(text).ok()
}

/// One vault as the server stores it: `(revision, seq, blob)` per item and
/// `(revision, seq)` per tombstone.
#[derive(Clone, Default)]
pub struct FakeVault {
    pub seq: u64,
    pub wrapped_key: Vec<u8>,
    pub items: BTreeMap<Id, (u64, u64, Vec<u8>)>,
    pub tombstones: BTreeMap<Id, (u64, u64)>,
    /// The highest compacted tombstone's seq (D76 §2).
    pub purged_seq: u64,
}

pub enum Fault {
    /// The request never reaches the server.
    Network,
    /// The server applies the request, then the response is lost.
    LoseResponse,
    /// The server answers with this status (and `Retry-After`) without
    /// applying the request.
    Status(u16, Option<&'static str>),
    /// The access token expired just before this request.
    ExpireAccess,
}

#[derive(Default)]
pub struct FakeServer {
    pub vaults: BTreeMap<Id, FakeVault>,
    access: BTreeSet<String>,
    refresh: BTreeSet<String>,
    revoked: BTreeSet<String>,
    /// Refresh tokens of a deleted account's sessions (D74 §5).
    deleted: BTreeSet<String>,
    issued: u64,
    /// `(prefix of "METHOD /path", fault)`: the first match fires once.
    faults: Vec<(String, Fault)>,
    /// Every request that reached the server: "METHOD /path [precondition]".
    pub log: Vec<String>,
    /// Sent as `pwm-epoch` with the vault list (D30).
    pub epoch: Id,
    /// Sent as `pwm-plan` and `pwm-plan-until` with the list (D73); none
    /// when `None`, as from a server older than D73.
    pub plan: Option<(String, Option<i64>)>,
}

fn json_response(status: u16, body: Value) -> HttpResponse {
    HttpResponse::new(status, Vec::new(), body.to_string().into_bytes())
}

fn error(status: u16, code: &str) -> HttpResponse {
    json_response(status, json!({ "error": code }))
}

fn parse_id(text: &str) -> Option<Id> {
    ItemId::from_str(text).ok().map(|id| *id.as_bytes())
}

fn id_string(id: &Id) -> String {
    ItemId::from_bytes(*id).to_string()
}

fn counter(text: &str) -> Option<u64> {
    let canonical = !text.is_empty()
        && text.bytes().all(|b| b.is_ascii_digit())
        && (text == "0" || !text.starts_with('0'));
    if canonical { text.parse().ok() } else { None }
}

impl FakeServer {
    pub fn new() -> Self {
        Self {
            epoch: *ItemId::generate().unwrap().as_bytes(),
            ..Self::default()
        }
    }

    /// What `pwm-server mark-restored` does after a restore.
    pub fn mark_restored(&mut self) {
        self.epoch = *ItemId::generate().unwrap().as_bytes();
    }

    /// Tombstone compaction (D76 §2): every tombstone of `vault` goes.
    pub fn compact(&mut self, vault: &Id) {
        let v = self.vaults.get_mut(vault).unwrap();
        if let Some(top) = v.tombstones.values().map(|(_, seq)| *seq).max() {
            v.purged_seq = v.purged_seq.max(top);
        }
        v.tombstones.clear();
    }

    /// A token pair for a new device.
    pub fn new_device(&mut self) -> Tokens {
        self.issue()
    }

    fn issue(&mut self) -> Tokens {
        self.issued += 1;
        let access = format!("pwma_{}", self.issued);
        let refresh = format!("pwmr_{}", self.issued);
        self.access.insert(access.clone());
        self.refresh.insert(refresh.clone());
        Tokens::new(access, refresh)
    }

    /// Ends every session (the user signed out everywhere).
    pub fn end_sessions(&mut self) {
        self.access.clear();
        self.refresh.clear();
    }

    /// Deletes the account: access tokens find nothing, and refresh tokens
    /// are answered `account_deleted` (D74 §5).
    pub fn delete_account(&mut self) {
        self.access.clear();
        self.deleted.append(&mut self.refresh);
    }

    pub fn revoke(&mut self, tokens: &Tokens) {
        self.revoked.insert(tokens.access().to_owned());
        self.revoked.insert(tokens.refresh().to_owned());
    }

    pub fn fault(&mut self, prefix: &str, fault: Fault) {
        self.faults.push((prefix.to_owned(), fault));
    }

    pub fn vault(&self, id: &Id) -> &FakeVault {
        &self.vaults[id]
    }

    /// `None` is a network failure.
    pub fn handle(&mut self, request: &HttpRequest) -> Option<HttpResponse> {
        let line = format!("{} {}", request.method.as_str(), request.path);
        let fault = self
            .faults
            .iter()
            .position(|(prefix, _)| line.starts_with(prefix.as_str()))
            .map(|i| self.faults.remove(i).1);
        let precondition = ["if-match", "if-none-match"]
            .iter()
            .filter_map(|name| request.header(name).map(|v| format!(" {name}={v}")))
            .collect::<String>();
        match fault {
            Some(Fault::Network) => return None,
            Some(Fault::Status(status, retry_after)) => {
                let headers = retry_after
                    .map(|s| vec![("retry-after".to_owned(), s.to_owned())])
                    .unwrap_or_default();
                return Some(HttpResponse::new(status, headers, b"{}".to_vec()));
            }
            Some(Fault::ExpireAccess) => {
                if let Some(token) = request
                    .header("authorization")
                    .and_then(|v| v.strip_prefix("Bearer "))
                {
                    self.access.remove(token);
                }
            }
            Some(Fault::LoseResponse) | None => {}
        }
        self.log.push(format!("{line}{precondition}"));
        let response = self.route(request);
        match fault {
            Some(Fault::LoseResponse) => None,
            _ => Some(response),
        }
    }

    fn route(&mut self, request: &HttpRequest) -> HttpResponse {
        let body: Value = request
            .body
            .as_deref()
            .and_then(|b| serde_json::from_slice(b).ok())
            .unwrap_or(Value::Null);
        if request.path == "/v1/auth/refresh" {
            let Some(token) = body["refresh_token"].as_str() else {
                return error(400, "invalid_request");
            };
            if self.revoked.contains(token) {
                return error(403, "device_revoked");
            }
            if self.deleted.contains(token) {
                return error(403, "account_deleted");
            }
            if !self.refresh.remove(token) {
                return error(401, "unauthorized");
            }
            let tokens = self.issue();
            return json_response(
                200,
                json!({ "access_token": tokens.access(), "refresh_token": tokens.refresh(),
                        "access_expires_in": 900, "refresh_expires_in": 7_776_000 }),
            );
        }
        let token = request
            .header("authorization")
            .and_then(|v| v.strip_prefix("Bearer "))
            .unwrap_or("");
        if self.revoked.contains(token) {
            return error(403, "device_revoked");
        }
        if !self.access.contains(token) {
            return error(401, "unauthorized");
        }
        let (path, query) = request.path.split_once('?').unwrap_or((&request.path, ""));
        let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
        match (request.method, parts.as_slice()) {
            (Method::Get, ["v1", "vaults"]) => self.list_vaults(),
            (Method::Post, ["v1", "vaults"]) => self.create_vault(&body),
            (Method::Get, ["v1", "vaults", vault, "items"]) => match parse_id(vault) {
                Some(vault) => self.changes(&vault, query),
                None => error(400, "invalid_request"),
            },
            (Method::Put | Method::Delete, ["v1", "vaults", vault, "items", item]) => {
                match (parse_id(vault), parse_id(item)) {
                    (Some(vault), Some(item)) => self.write(request, &vault, &item, &body),
                    _ => error(400, "invalid_request"),
                }
            }
            _ => error(404, "not_found"),
        }
    }

    fn list_vaults(&self) -> HttpResponse {
        let list: Vec<Value> = self
            .vaults
            .iter()
            .map(|(id, v)| {
                json!({ "id": id_string(id), "role": "owner", "wrap": "own",
                        "wrapped_key": b64(&v.wrapped_key), "shared_by": null,
                        "seq": v.seq.to_string() })
            })
            .collect();
        let mut response = json_response(200, Value::Array(list));
        response
            .headers
            .push(("pwm-epoch".to_owned(), id_string(&self.epoch)));
        if let Some((plan, until)) = &self.plan {
            response.headers.push(("pwm-plan".to_owned(), plan.clone()));
            if let Some(until) = until {
                response
                    .headers
                    .push(("pwm-plan-until".to_owned(), until.to_string()));
            }
        }
        response
    }

    fn create_vault(&mut self, body: &Value) -> HttpResponse {
        let (Some(id), Some(key)) = (
            body["id"].as_str().and_then(parse_id),
            body["wrapped_key"].as_str().and_then(unb64),
        ) else {
            return error(400, "invalid_request");
        };
        if self.vaults.contains_key(&id) {
            return error(409, "vault_exists");
        }
        self.vaults.insert(
            id,
            FakeVault {
                wrapped_key: key.clone(),
                ..FakeVault::default()
            },
        );
        json_response(
            201,
            json!({ "id": id_string(&id), "role": "owner", "wrap": "own", "wrapped_key": b64(&key),
                    "shared_by": null, "seq": "0" }),
        )
    }

    fn changes(&self, vault: &Id, query: &str) -> HttpResponse {
        let Some(v) = self.vaults.get(vault) else {
            return error(404, "not_found");
        };
        let mut since = 0;
        let mut limit = 500;
        for pair in query.split('&').filter(|p| !p.is_empty()) {
            let (key, value) = pair.split_once('=').unwrap();
            let value = counter(value).unwrap();
            match key {
                "since" => since = value,
                "limit" => limit = value as usize,
                _ => {}
            }
        }
        if since > 0 && since < v.purged_seq {
            return error(410, "resync_required");
        }
        let mut all: Vec<(u64, Value)> = v
            .items
            .iter()
            .filter(|(_, (_, seq, _))| *seq > since)
            .map(|(id, (rev, seq, blob))| {
                (*seq, json!({ "id": id_string(id), "revision": rev.to_string(), "seq": seq.to_string(), "blob": b64(blob) }))
            })
            .chain(v.tombstones.iter().filter(|(_, (_, seq))| *seq > since).map(|(id, (rev, seq))| {
                (*seq, json!({ "id": id_string(id), "revision": rev.to_string(), "seq": seq.to_string(), "deleted": true }))
            }))
            .collect();
        all.sort_by_key(|(seq, _)| *seq);
        let more = all.len() > limit;
        all.truncate(limit);
        let next = match all.last() {
            Some((seq, _)) if more => *seq,
            _ => v.seq,
        };
        json_response(
            200,
            json!({ "changes": all.into_iter().map(|(_, c)| c).collect::<Vec<_>>(),
                    "next": next.to_string(), "more": more }),
        )
    }

    fn write(
        &mut self,
        request: &HttpRequest,
        vault: &Id,
        item: &Id,
        body: &Value,
    ) -> HttpResponse {
        let if_match = request.header("if-match");
        let if_none_match = request.header("if-none-match");
        let Some(v) = self.vaults.get_mut(vault) else {
            return error(404, "not_found");
        };
        let expected = match (if_match, if_none_match) {
            (None, None) => return error(428, "precondition_required"),
            (Some(etag), None) => match etag
                .strip_prefix('"')
                .and_then(|e| e.strip_suffix('"'))
                .and_then(counter)
            {
                Some(rev) if rev >= 1 => Some(rev),
                _ => return error(400, "invalid_request"),
            },
            (None, Some("*")) if request.method == Method::Put => None,
            _ => return error(400, "invalid_request"),
        };
        let conflict = |current: &(u64, u64, Vec<u8>)| {
            json_response(
                412,
                json!({ "error": "revision_conflict", "current": {
                    "revision": current.0.to_string(), "seq": current.1.to_string(), "blob": b64(&current.2) } }),
            )
        };
        if v.tombstones.contains_key(item) {
            return error(409, "item_deleted");
        }
        let current = v.items.get(item).cloned();
        if request.method == Method::Delete {
            let expected = expected.unwrap();
            return match current {
                None => error(404, "not_found"),
                Some(c) if c.0 != expected => conflict(&c),
                Some(_) => {
                    v.seq += 1;
                    v.items.remove(item);
                    v.tombstones.insert(*item, (expected, v.seq));
                    json_response(
                        200,
                        json!({ "revision": expected.to_string(), "seq": v.seq.to_string() }),
                    )
                }
            };
        }
        let (Some(revision), Some(blob)) = (
            body["revision"].as_str().and_then(counter),
            body["blob"].as_str().and_then(unb64),
        ) else {
            return error(400, "invalid_request");
        };
        if revision != expected.map_or(1, |r| r + 1) || Envelope::from_bytes(&blob).is_err() {
            return error(400, "invalid_request");
        }
        let status = match (expected, &current) {
            (None, None) => 201,
            (Some(_), None) => return error(404, "not_found"),
            (Some(e), Some(c)) if c.0 == e => 200,
            (_, Some(c)) => return conflict(c),
        };
        v.seq += 1;
        v.items.insert(*item, (revision, v.seq, blob));
        HttpResponse::new(
            status,
            vec![("etag".to_owned(), format!("\"{revision}\""))],
            json!({ "revision": revision.to_string(), "seq": v.seq.to_string() })
                .to_string()
                .into_bytes(),
        )
    }
}

/// Where a run stopped.
#[derive(Debug)]
pub enum Ran {
    Done(SyncOutcome),
    Wait(u64),
}

/// Runs `step` and the steps after it against `server`.
pub fn drive(engine: &mut Engine, server: &mut FakeServer, mut step: Step, now: i64) -> Ran {
    loop {
        step = match step {
            Step::Send(request) => match server.handle(&request) {
                Some(response) => engine.receive(response, now).unwrap(),
                None => engine.network_failed().unwrap(),
            },
            Step::Wait { delay_ms } => return Ran::Wait(delay_ms),
            Step::Done(outcome) => return Ran::Done(outcome),
        };
    }
}

/// One sync run that must finish.
pub fn sync(engine: &mut Engine, server: &mut FakeServer, now: i64) -> SyncReport {
    let step = engine.start_sync().unwrap();
    match drive(engine, server, step, now) {
        Ran::Done(SyncOutcome::Synced(report)) => report,
        other => panic!("sync did not finish: {other:?}"),
    }
}

/// Steps until the engine wants to send a request matching `prefix`, and
/// returns that request without sending it.
pub fn run_until(
    engine: &mut Engine,
    server: &mut FakeServer,
    prefix: &str,
    now: i64,
) -> HttpRequest {
    let mut step = engine.start_sync().unwrap();
    loop {
        step = match step {
            Step::Send(request) => {
                if format!("{} {}", request.method.as_str(), request.path).starts_with(prefix) {
                    return request;
                }
                let response = server.handle(&request).expect("no fault expected");
                engine.receive(response, now).unwrap()
            }
            other => panic!("never sent {prefix}: {other:?}"),
        };
    }
}

/// The account all devices in a test share.
pub struct Account {
    auk: SymmetricKey,
    id: Id,
    sealed: Envelope,
}

impl Account {
    pub fn new() -> Self {
        let auk = SymmetricKey::generate().unwrap();
        let id = *ItemId::generate().unwrap().as_bytes();
        let sealed = AccountKeyset::generate().unwrap().seal(&auk, &id).unwrap();
        Self { auk, id, sealed }
    }

    pub fn keyset(&self) -> AccountKeyset {
        AccountKeyset::open(&self.sealed, &self.auk, &self.id).unwrap()
    }

    /// A signed-in device with an empty cache.
    pub fn device(&self, server: &mut FakeServer) -> Engine {
        let tokens = server.new_device();
        SyncEngine::new(
            SqliteStore::open_in_memory().unwrap(),
            self.keyset(),
            tokens,
        )
        .unwrap()
    }
}

/// Creates a vault online; returns its id.
pub fn create_vault(engine: &mut Engine, server: &mut FakeServer) -> Id {
    let (id, step) = engine.start_create_vault().unwrap();
    match drive(engine, server, step, 0) {
        Ran::Done(SyncOutcome::Synced(_)) => id,
        other => panic!("vault not created: {other:?}"),
    }
}

pub fn note(title: &str, now: i64) -> Item {
    Item::new(ItemKind::Note, title, now)
}

/// Saves a new note; returns its id.
pub fn add(engine: &mut Engine, vault: &Id, title: &str, now: i64) -> ItemId {
    let id = ItemId::generate().unwrap();
    engine.save_item(vault, &id, &note(title, now)).unwrap();
    id
}

/// Edits an item's title the way the UI does (previous version to history).
pub fn edit(engine: &mut Engine, vault: &Id, id: &ItemId, title: &str, now: i64) {
    let mut item = engine.item(vault, id).unwrap().expect("item exists");
    let mut edited = item.clone();
    edited.title = title.to_owned();
    item.apply_edit(edited, now).unwrap();
    engine.save_item(vault, id, &item).unwrap();
}

pub fn record(engine: &Engine, vault: &Id, id: &ItemId) -> Option<ItemRecord> {
    engine.store().item(vault, id).unwrap()
}

pub fn state(engine: &Engine, vault: &Id, id: &ItemId) -> Option<(u64, SyncState)> {
    record(engine, vault, id).map(|r| (r.revision, r.state))
}

/// Titles of the current items, sorted.
pub fn titles(engine: &Engine, vault: &Id) -> Vec<String> {
    let mut titles: Vec<String> = engine
        .items(vault)
        .unwrap()
        .into_iter()
        .map(|(_, item)| item.title.clone())
        .collect();
    titles.sort();
    titles
}

pub fn history_titles(item: &Item) -> Vec<String> {
    item.history()
        .iter()
        .map(|v| v.item.title.clone())
        .collect()
}

/// A token that will never be accepted, for tests that build tokens by hand.
pub fn secret(text: &str) -> Secret<String> {
    Secret::new(text.to_owned())
}
