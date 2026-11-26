//! Neutral JSONC and strict JSON parsing helpers shared across v2.2 and legacy loaders,
//! lossless CST merging, validation, and CLI commands.

use std::fs;
use std::path::Path;

use jsonc_parser::ParseOptions;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::exit_codes::{VspError, VspResult};

/// Returns the canonical `jsonc-parser` options used across `vsp`:
/// allows `//` and `/* */` comments and trailing commas while rejecting unquoted property names.
#[must_use]
pub const fn default_parse_options() -> ParseOptions {
    ParseOptions {
        allow_comments: true,
        allow_trailing_commas: true,
        allow_loose_object_property_names: false,
    }
}

/// Parses a JSONC string strictly as a single JSON value.
///
/// Allows `//` and `/* */` comments and trailing commas while rejecting empty
/// documents, trailing tokens, and syntax errors.
///
/// # Errors
///
/// Returns [`VspError::Operational`] if the input fails JSONC parsing or contains no value.
pub fn parse_jsonc_value(input: &str, label: &str) -> VspResult<Value> {
    let options = default_parse_options();
    let parsed = jsonc_parser::parse_to_serde_value(input, &options)
        .map_err(|err| VspError::Operational(format!("{label}: JSONC parse error: {err}")))?;
    parsed.ok_or_else(|| {
        VspError::Operational(format!("{label}: empty JSONC document (expected a value)"))
    })
}

/// Parses a UTF-8 JSONC byte slice into a strongly typed `T`.
///
/// # Errors
///
/// Returns [`VspError::Operational`] on invalid UTF-8 or JSONC syntax errors, or
/// [`VspError::UsageOrConfig`] if the parsed document fails schema deserialization into `T`.
pub fn parse_jsonc_bytes<T: DeserializeOwned>(bytes: &[u8], label: &str) -> VspResult<T> {
    let text = std::str::from_utf8(bytes)
        .map_err(|err| VspError::Operational(format!("{label}: invalid UTF-8: {err}")))?;
    let val = parse_jsonc_value(text, label)?;
    serde_json::from_value(val)
        .map_err(|err| VspError::UsageOrConfig(format!("{label}: schema error: {err}")))
}

/// Parses a JSONC file strictly as a single JSON object and strips `$schema` and `@extends`.
///
/// Matches `compiler_parse_jsonc_object` in `scripts/lib/compiler.sh`.
///
/// # Errors
///
/// Returns [`VspError::Operational`] if the file cannot be read, fails JSONC parsing,
/// or does not have an object root.
pub fn parse_jsonc_settings_object(path: &Path) -> VspResult<Map<String, Value>> {
    let raw = fs::read_to_string(path).map_err(|err| {
        VspError::Operational(format!("failed to read {}: {err}", path.display()))
    })?;
    let Value::Object(mut map) = parse_jsonc_value(&raw, &path.display().to_string())? else {
        return Err(VspError::Operational(format!(
            "{}: expected a single JSON object",
            path.display()
        )));
    };
    map.remove("$schema");
    map.remove("@extends");
    Ok(map)
}

/// Parses a strict `.json` file (comments and trailing commas disallowed) into a typed value.
///
/// # Errors
///
/// Returns [`VspError::Operational`] if reading or JSON deserialization fails.
pub fn read_strict_json<T: DeserializeOwned>(path: &Path) -> VspResult<T> {
    let raw = fs::read_to_string(path).map_err(|err| {
        VspError::Operational(format!("failed to read {}: {err}", path.display()))
    })?;
    serde_json::from_str(&raw).map_err(|err| {
        VspError::Operational(format!(
            "failed to parse strict JSON {}: {err}",
            path.display()
        ))
    })
}

/// Parses a strict `.json` file into a raw [`Value`], verifying the expected root type.
///
/// # Errors
///
/// Returns [`VspError::Operational`] if reading fails, JSON parsing fails, or the root
/// type does not match `expect_array`.
pub fn read_strict_json_value(path: &Path, expect_array: bool) -> VspResult<Value> {
    let raw = fs::read_to_string(path).map_err(|err| {
        VspError::Operational(format!("failed to read {}: {err}", path.display()))
    })?;
    let val: Value = serde_json::from_str(&raw).map_err(|err| {
        VspError::Operational(format!(
            "failed to parse strict JSON {}: {err}",
            path.display()
        ))
    })?;
    if expect_array && !val.is_array() {
        return Err(VspError::Operational(format!(
            "{}: expected single JSON array",
            path.display()
        )));
    }
    if !expect_array && !val.is_object() {
        return Err(VspError::Operational(format!(
            "{}: expected single JSON object",
            path.display()
        )));
    }
    Ok(val)
}
