//! JSON exports: parsed into a `serde_json::Value` that is wiped when
//! dropped, and small readers that tolerate missing or odd fields.
//! `serde_json` stops at 128 levels of nesting, which bounds hostile files.

use serde_json::{Map, Value};
use zeroize::Zeroize;

use crate::ImportError;

/// A parsed document; every string in it is zeroized on drop.
pub struct Doc(Value);

impl Doc {
    pub fn get(&self) -> &Value {
        &self.0
    }
}

impl Drop for Doc {
    fn drop(&mut self) {
        wipe(&mut self.0);
    }
}

fn wipe(value: &mut Value) {
    match value {
        Value::String(s) => s.zeroize(),
        Value::Array(items) => items.iter_mut().for_each(wipe),
        Value::Object(map) => {
            for (_, v) in map.iter_mut() {
                wipe(v);
            }
        }
        _ => {}
    }
}

pub fn parse(bytes: &[u8]) -> Result<Doc, ImportError> {
    let bytes = crate::csv::strip_bom(bytes);
    serde_json::from_slice(bytes)
        .map(Doc)
        .map_err(|_| ImportError::Unreadable)
}

/// A string field, trimmed; numbers and booleans as text; else "".
pub fn s(v: &Value, key: &str) -> String {
    text(v.get(key))
}

pub fn text(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.trim().to_owned(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        _ => String::new(),
    }
}

pub fn arr<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v.get(key)
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

pub fn obj<'a>(v: &'a Value, key: &str) -> Option<&'a Map<String, Value>> {
    v.get(key).and_then(Value::as_object)
}

pub fn int(v: &Value, key: &str) -> Option<i64> {
    match v.get(key)? {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

pub fn truthy(v: &Value, key: &str) -> bool {
    match v.get(key) {
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_i64().is_some_and(|n| n != 0),
        Some(Value::String(s)) => matches!(s.trim(), "1" | "true" | "True" | "TRUE" | "yes"),
        _ => false,
    }
}
