//! Loading collections and environments from JSON files.

use std::path::Path;

use anyhow::{anyhow, Context, Result};
use rustman_core::importers::{import_openapi, import_postman};
use rustman_core::{Collection, Environment, Variable};
use serde_json::Value;
use uuid::Uuid;

/// Loads a Postman v2.1, OpenAPI 3.x or internal Rustman collection JSON.
pub fn load_collection(path: &Path) -> Result<Collection> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("cannot read {}", path.display()))?;
    parse_collection(&text).with_context(|| format!("cannot parse {}", path.display()))
}

pub fn parse_collection(text: &str) -> Result<Collection> {
    let v: Value = serde_json::from_str(text)?;
    if v.get("openapi").is_some() {
        Ok(import_openapi(text)?)
    } else if v.get("info").is_some() {
        Ok(import_postman(text)?)
    } else {
        serde_json::from_value(v).map_err(|e| anyhow!("not a Postman/OpenAPI/Rustman collection: {e}"))
    }
}

/// Loads a Postman environment (`values`) or an internal Rustman environment.
pub fn load_environment(path: &Path) -> Result<Environment> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("cannot read {}", path.display()))?;
    let v: Value = serde_json::from_str(&text)
        .with_context(|| format!("invalid JSON in {}", path.display()))?;
    if let Some(values) = v.get("values").and_then(Value::as_array) {
        let variables = values
            .iter()
            .filter_map(|e| {
                let key = e.get("key")?.as_str()?.to_string();
                let value = match e.get("value") {
                    Some(Value::String(s)) => s.clone(),
                    Some(Value::Null) | None => String::new(),
                    Some(o) => o.to_string(),
                };
                Some(Variable {
                    key,
                    value,
                    enabled: e.get("enabled").and_then(Value::as_bool).unwrap_or(true),
                    secret: e.get("type").and_then(Value::as_str) == Some("secret"),
                })
            })
            .collect();
        Ok(Environment {
            id: Uuid::new_v4(),
            name: v.get("name").and_then(Value::as_str).unwrap_or("environment").into(),
            variables,
        })
    } else {
        serde_json::from_value(v).map_err(|e| anyhow!("not a Postman/Rustman environment: {e}"))
    }
}
