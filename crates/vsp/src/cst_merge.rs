//! Lossless JSONC Concrete Syntax Tree (CST) merger backed by `jsonc-parser`'s CST.
//!
//! Preserves all leading, trailing, inline, and interior `//` and `/* */` comments verbatim
//! on untouched keys and user-added keys, merges nested `[language]` blocks in place without
//! clobbering user sub-keys, merges set-like union arrays without losing comments, and retires
//! obsolete VSP-managed keys cleanly. Permanently retires `--allow-comment-loss`.

use std::collections::{BTreeMap, BTreeSet};

use jsonc_parser::cst::{CstArray, CstInputValue, CstNode, CstObject, CstObjectProp, CstRootNode};
use serde_json::{Map, Value};

use crate::exit_codes::{VspError, VspResult};
use crate::jsonc::{default_parse_options, parse_jsonc_value};
use crate::model::{WorkspaceExtensionsFile, COMPILER_UNION_KEYS};
use crate::resolver::to_canonical_json_string;

/// Options controlling a lossless JSONC CST object merge.
#[derive(Debug, Clone)]
pub struct CstMergeOptions {
    /// Top-level keys to remove when they are not present in `desired`
    /// (for example retired VSP-managed keys, UX keys on `--ux default`, or AI keys).
    pub delete_keys: Vec<String>,
    /// Nested object key paths to remove when not present in `desired`
    /// (for example `["[typescript]", "editor.codeActionsOnSave", "source.fixAll.eslint"]`).
    pub delete_paths: Vec<Vec<String>>,
    /// Top-level array keys merged via order-preserving set union instead of replacement.
    pub union_array_keys: BTreeSet<String>,
}

impl Default for CstMergeOptions {
    fn default() -> Self {
        Self {
            delete_keys: Vec::new(),
            delete_paths: Vec::new(),
            union_array_keys: COMPILER_UNION_KEYS
                .iter()
                .map(|k| (*k).to_string())
                .collect(),
        }
    }
}

/// Losslessly merges `desired` settings into an existing JSONC document `existing_text`.
///
/// - When `existing_text` is empty or whitespace-only, emits canonical formatted JSON.
/// - Otherwise parses `existing_text` with [`CstRootNode::parse`], indexes existing CST
///   properties once per object (`O(K + P)`), updates changed values in place, collapses
///   duplicate keys to the effective last occurrence, recursively merges nested JSON objects
///   (such as `[rust]` / `[typescript]` blocks) without touching user-owned keys or comments,
///   unions set-like arrays, and deletes retired keys and nested paths.
/// - Re-parses the emitted JSONC before returning to guarantee syntactic validity and
///   post-merge value effectiveness.
///
/// # Errors
///
/// Returns [`VspError::Operational`] if `existing_text` is not valid JSONC with an object root
/// or if output verification fails.
pub fn merge_jsonc_object_str(
    existing_text: &str,
    desired: &Map<String, Value>,
    opts: &CstMergeOptions,
) -> VspResult<String> {
    if existing_text.trim().is_empty() {
        return to_canonical_json_string(desired);
    }

    let parse_opts = default_parse_options();
    let root = CstRootNode::parse(existing_text, &parse_opts)
        .map_err(|err| VspError::Operational(format!("JSONC CST parse error: {err}")))?;

    let root_obj = root.object_value().ok_or_else(|| {
        VspError::Operational("expected a JSONC object at document root".to_string())
    })?;

    // 1. Merge desired keys recursively into the CST object using an O(K + P) property index.
    let mut remaining_root_props =
        merge_into_cst_object(&root_obj, desired, &opts.union_array_keys, true)?;

    // 2. Delete retired nested paths that are not re-declared in `desired`.
    for path in &opts.delete_paths {
        if !path.is_empty() && !map_contains_nested_path(desired, path) {
            if let Some((first, rest)) = path.split_first() {
                if rest.is_empty() {
                    if let Some(props) = remaining_root_props.remove(first) {
                        for prop in props {
                            prop.remove();
                        }
                    }
                } else if let Some(props) = remaining_root_props.get(first) {
                    for prop in props {
                        if let Some(sub_obj) = prop.object_value() {
                            remove_nested_path(&sub_obj, rest);
                        }
                    }
                } else {
                    remove_nested_path(&root_obj, path);
                }
            }
        }
    }

    // 3. Delete retired top-level keys that are not re-declared in `desired`.
    let mut del_keys: BTreeSet<&str> = opts.delete_keys.iter().map(String::as_str).collect();
    for k in desired.keys() {
        del_keys.remove(k.as_str());
    }
    for key in del_keys {
        if let Some(props) = remaining_root_props.remove(key) {
            for prop in props {
                prop.remove();
            }
        }
    }

    let mut out = root.to_string();
    if !out.ends_with('\n') {
        out.push('\n');
    }

    // Self-verify syntactic validity so we never emit broken JSONC.
    let verified = parse_jsonc_value(&out, "cst_merge_result")?;
    let Value::Object(verified_obj) = &verified else {
        return Err(VspError::Operational(
            "internal CST merge error: resulting document is not a JSON object".to_string(),
        ));
    };

    // Self-verify the values too: each desired leaf must be the effective value in the
    // emitted document. A duplicate key that survived the merge would fail here.
    if let Err(path) = verify_desired_applied(desired, verified_obj, &opts.union_array_keys, true) {
        return Err(VspError::Operational(format!(
            "internal CST merge error: desired setting '{path}' is not effective in the merged document"
        )));
    }

    Ok(out)
}

/// Checks that `output` contains each desired value with the merge semantics of
/// [`merge_jsonc_object_str`]: objects are merged (subset check), top-level union arrays
/// contain each desired item, and every other value is equal. Returns the dotted path of
/// the first mismatch.
fn verify_desired_applied(
    desired: &Map<String, Value>,
    output: &Map<String, Value>,
    union_keys: &BTreeSet<String>,
    is_top_level: bool,
) -> Result<(), String> {
    for (key, desired_val) in desired {
        let Some(actual) = output.get(key) else {
            return Err(key.clone());
        };
        match (desired_val, actual) {
            (Value::Object(desired_sub), Value::Object(actual_sub)) => {
                verify_desired_applied(desired_sub, actual_sub, union_keys, false)
                    .map_err(|inner| format!("{key}.{inner}"))?;
            }
            (Value::Array(desired_arr), Value::Array(actual_arr))
                if is_top_level && union_keys.contains(key) =>
            {
                if !desired_arr.iter().all(|item| actual_arr.contains(item)) {
                    return Err(key.clone());
                }
            }
            _ if desired_val == actual => {}
            _ => return Err(key.clone()),
        }
    }
    Ok(())
}

/// Indexes all properties of `cst_obj` once by decoded property name, preserving document
/// order per key so duplicate occurrences can be collapsed to the effective last entry in
/// `O(K + P)` time instead of rescanning the object for every desired key.
fn index_cst_object_props(cst_obj: &CstObject) -> BTreeMap<String, Vec<CstObjectProp>> {
    let mut index: BTreeMap<String, Vec<CstObjectProp>> = BTreeMap::new();
    for prop in cst_obj.properties() {
        if let Some(name) = prop.name().and_then(|n| n.decoded_value().ok()) {
            index.entry(name.to_string()).or_default().push(prop);
        }
    }
    index
}

/// Losslessly merges workspace `.vscode/extensions.json` preserving comments and user-owned
/// recommendations while retiring previously VSP-managed recommendations.
///
/// Returns `(merged_jsonc_text, new_managed_recommendations)`.
///
/// # Errors
///
/// Returns [`VspError::Operational`] if `existing_text` is malformed JSONC or not an object.
pub fn merge_workspace_extensions_jsonc(
    existing_text: &str,
    desired: &WorkspaceExtensionsFile,
    prev_managed_recs: &[String],
) -> VspResult<(String, Vec<String>)> {
    let base_text = if existing_text.trim().is_empty() {
        "{}\n"
    } else {
        existing_text
    };

    let existing_val = parse_jsonc_value(base_text, ".vscode/extensions.json")?;
    let existing_obj = existing_val.as_object().ok_or_else(|| {
        VspError::Operational(
            "existing .vscode/extensions.json is not a single valid JSON object".to_string(),
        )
    })?;

    let existing_recs = string_array_field(existing_obj, "recommendations");
    let existing_unwanted = string_array_field(existing_obj, "unwantedRecommendations");

    let prev_lc: BTreeSet<String> = prev_managed_recs
        .iter()
        .map(|s| s.to_ascii_lowercase())
        .collect();
    let incoming_recs_lc: BTreeSet<String> = desired
        .recommendations
        .iter()
        .map(|s| s.to_ascii_lowercase())
        .collect();

    // Never subtract from unwantedRecommendations; union case-insensitively.
    let mut final_unwanted = Vec::new();
    let mut unwanted_lc = BTreeSet::new();
    for id in existing_unwanted
        .iter()
        .chain(desired.unwanted_recommendations.iter())
    {
        let lc = id.to_ascii_lowercase();
        if unwanted_lc.insert(lc) {
            final_unwanted.push(id.clone());
        }
    }

    // Keep user-owned recommendations (not in prev_managed) + still-desired recommendations.
    let mut final_recs = Vec::new();
    let mut rec_seen = BTreeSet::new();
    for id in &existing_recs {
        let lc = id.to_ascii_lowercase();
        let keep = !prev_lc.contains(&lc) || incoming_recs_lc.contains(&lc);
        if keep && !unwanted_lc.contains(&lc) && rec_seen.insert(lc) {
            final_recs.push(id.clone());
        }
    }
    for id in &desired.recommendations {
        let lc = id.to_ascii_lowercase();
        if !unwanted_lc.contains(&lc) && rec_seen.insert(lc) {
            final_recs.push(id.clone());
        }
    }

    // Track which recommendations are VSP-managed (excluding pre-existing user-owned entries).
    let existing_recs_lc: BTreeSet<String> = existing_recs
        .iter()
        .map(|s| s.to_ascii_lowercase())
        .collect();
    let new_managed_recs: Vec<String> = desired
        .recommendations
        .iter()
        .filter(|id| {
            let lc = id.to_ascii_lowercase();
            !existing_recs_lc.contains(&lc) || prev_lc.contains(&lc)
        })
        .cloned()
        .collect();

    if existing_text.trim().is_empty() {
        let mut out_map = Map::new();
        out_map.insert(
            "recommendations".to_string(),
            Value::Array(final_recs.into_iter().map(Value::String).collect()),
        );
        if !final_unwanted.is_empty() {
            out_map.insert(
                "unwantedRecommendations".to_string(),
                Value::Array(final_unwanted.into_iter().map(Value::String).collect()),
            );
        }
        return Ok((to_canonical_json_string(&out_map)?, new_managed_recs));
    }

    let parse_opts = default_parse_options();
    let root = CstRootNode::parse(existing_text, &parse_opts)
        .map_err(|err| VspError::Operational(format!("extensions.json CST parse error: {err}")))?;
    let root_obj = root.object_value().ok_or_else(|| {
        VspError::Operational("extensions.json root is not a JSON object".to_string())
    })?;

    sync_cst_string_array_prop(&root_obj, "recommendations", &final_recs)?;
    if !final_unwanted.is_empty() || root_obj.get("unwantedRecommendations").is_some() {
        sync_cst_string_array_prop(&root_obj, "unwantedRecommendations", &final_unwanted)?;
    }

    let mut out = root.to_string();
    if !out.ends_with('\n') {
        out.push('\n');
    }
    let _ = parse_jsonc_value(&out, ".vscode/extensions.json")?;
    Ok((out, new_managed_recs))
}

/// Losslessly merges workspace `.vscode/tasks.json` by task `.label`.
///
/// Preserves existing comments and user tasks with unrelated labels, updates managed tasks
/// whose `.label` matches in place, and appends new managed tasks cleanly.
///
/// # Errors
///
/// Returns [`VspError::Operational`] if `existing_text` is malformed JSONC or not an object.
pub fn merge_workspace_tasks_jsonc(existing_text: &str, desired_doc: &Value) -> VspResult<String> {
    if existing_text.trim().is_empty() {
        return to_canonical_json_string(desired_doc);
    }

    let parse_opts = default_parse_options();
    let root = CstRootNode::parse(existing_text, &parse_opts)
        .map_err(|err| VspError::Operational(format!("tasks.json CST parse error: {err}")))?;
    let root_obj = root
        .object_value()
        .ok_or_else(|| VspError::Operational("tasks.json root is not a JSON object".to_string()))?;

    if root_obj.get("version").is_none() {
        let ver = desired_doc
            .get("version")
            .cloned()
            .unwrap_or_else(|| Value::String("2.0.0".to_string()));
        append_prop_safe(&root_obj, "version", &ver)?;
    }

    if let Some(desired_tasks) = desired_doc.get("tasks").and_then(Value::as_array) {
        let tasks_arr = if let Some(arr) = root_obj.array_value("tasks") {
            arr
        } else {
            if let Some(prop) = root_obj.get("tasks") {
                set_prop_value_safe(&prop, &Value::Array(Vec::new()))?;
            } else {
                append_prop_safe(&root_obj, "tasks", &Value::Array(Vec::new()))?;
            }
            root_obj.array_value("tasks").ok_or_else(|| {
                VspError::Operational("failed to create tasks array in CST".to_string())
            })?
        };

        // Snapshot pre-existing CST elements once before appending new nodes: newly appended
        // synthetic CST nodes must not be re-parsed via `cst_node_to_value` before root
        // serialization.
        let mut existing_tasks: Vec<(Option<String>, Value, Option<CstNode>)> = Vec::new();
        for elem in tasks_arr.elements() {
            let elem_val = cst_node_to_value(&elem)?;
            let label = elem_val
                .get("label")
                .and_then(Value::as_str)
                .map(ToString::to_string);
            existing_tasks.push((label, elem_val, Some(elem)));
        }

        // Collapse duplicate labels within `desired_tasks` (last occurrence wins, preserving
        // first-seen order) so each existing CST node is replaced at most once and each new
        // labeled task is appended at most once.
        let mut collapsed_desired: Vec<&Value> = Vec::with_capacity(desired_tasks.len());
        let mut seen_desired_labels: BTreeMap<&str, usize> = BTreeMap::new();
        for desired_task in desired_tasks {
            if let Some(label) = desired_task.get("label").and_then(Value::as_str) {
                if let Some(&idx) = seen_desired_labels.get(label) {
                    collapsed_desired[idx] = desired_task;
                } else {
                    seen_desired_labels.insert(label, collapsed_desired.len());
                    collapsed_desired.push(desired_task);
                }
            } else {
                collapsed_desired.push(desired_task);
            }
        }

        for desired_task in collapsed_desired {
            let desired_label = desired_task.get("label").and_then(Value::as_str);
            let mut updated = false;
            if let Some(label) = desired_label {
                for (existing_label, existing_val, existing_node) in &mut existing_tasks {
                    if existing_label.as_deref() == Some(label) {
                        if *existing_val != *desired_task {
                            if let Some(node) = existing_node.take() {
                                replace_cst_node_safe(node, desired_task)?;
                            }
                        }
                        updated = true;
                        break;
                    }
                }
            }
            if !updated {
                append_value_to_cst_array(&tasks_arr, desired_task)?;
            }
        }
    }

    let mut out = root.to_string();
    if !out.ends_with('\n') {
        out.push('\n');
    }
    let _ = parse_jsonc_value(&out, ".vscode/tasks.json")?;
    Ok(out)
}

fn merge_into_cst_object(
    cst_obj: &CstObject,
    desired: &Map<String, Value>,
    union_keys: &BTreeSet<String>,
    is_top_level: bool,
) -> VspResult<BTreeMap<String, Vec<CstObjectProp>>> {
    let mut prop_index = index_cst_object_props(cst_obj);

    for (key, desired_val) in desired {
        if let Some(mut matches) = prop_index.remove(key) {
            // Keep only the effective (last) occurrence of a duplicate key; remove earlier copies.
            if let Some(prop) = matches.pop() {
                for duplicate in matches {
                    duplicate.remove();
                }

                if let (Value::Object(desired_sub), Some(existing_sub)) =
                    (desired_val, prop.object_value())
                {
                    let _ = merge_into_cst_object(&existing_sub, desired_sub, union_keys, false)?;
                    continue;
                }

                if is_top_level && union_keys.contains(key) {
                    if let (Value::Array(desired_arr), Some(existing_arr)) =
                        (desired_val, prop.array_value())
                    {
                        let mut existing_vals = Vec::new();
                        for elem in existing_arr.elements() {
                            existing_vals.push(cst_node_to_value(&elem)?);
                        }
                        for item in desired_arr {
                            if !existing_vals.contains(item) {
                                append_value_to_cst_array(&existing_arr, item)?;
                                existing_vals.push(item.clone());
                            }
                        }
                        continue;
                    }
                }

                let existing_val = match prop.value() {
                    Some(v) => Some(cst_node_to_value(&v)?),
                    None => None,
                };
                if existing_val.as_ref() == Some(desired_val) {
                    continue;
                }
                set_prop_value_safe(&prop, desired_val)?;
                continue;
            }
        }
        append_prop_safe(cst_obj, key, desired_val)?;
    }
    Ok(prop_index)
}

fn sync_cst_string_array_prop(
    obj: &CstObject,
    key: &str,
    desired_items: &[String],
) -> VspResult<()> {
    let desired_lc: BTreeSet<String> = desired_items
        .iter()
        .map(|s| s.to_ascii_lowercase())
        .collect();

    if let Some(arr) = obj.array_value(key) {
        // Remove elements no longer in desired_items (or duplicates).
        let mut seen = BTreeSet::new();
        for elem in arr.elements() {
            let val = cst_node_to_value(&elem)?;
            let keep = val.as_str().is_some_and(|s| {
                let lc = s.to_ascii_lowercase();
                desired_lc.contains(&lc) && seen.insert(lc)
            });
            if !keep {
                elem.remove();
            }
        }
        // Append any missing desired items cleanly.
        for item in desired_items {
            let lc = item.to_ascii_lowercase();
            if seen.insert(lc) {
                append_value_to_cst_array(&arr, &Value::String(item.clone()))?;
            }
        }
        return Ok(());
    }

    let val = Value::Array(desired_items.iter().cloned().map(Value::String).collect());
    if let Some(prop) = obj.get(key) {
        set_prop_value_safe(&prop, &val)?;
    } else {
        append_prop_safe(obj, key, &val)?;
    }
    Ok(())
}

fn map_contains_nested_path(map: &Map<String, Value>, path: &[String]) -> bool {
    let Some((first, rest)) = path.split_first() else {
        return false;
    };
    let Some(val) = map.get(first) else {
        return false;
    };
    if rest.is_empty() {
        return true;
    }
    match val {
        Value::Object(sub) => map_contains_nested_path(sub, rest),
        _ => false,
    }
}

fn remove_nested_path(obj: &CstObject, path: &[String]) {
    let Some((first, rest)) = path.split_first() else {
        return;
    };
    if rest.is_empty() {
        while let Some(prop) = obj.get(first) {
            prop.remove();
        }
        return;
    }
    if let Some(sub_obj) = obj.object_value(first) {
        remove_nested_path(&sub_obj, rest);
    }
}

fn string_array_field(obj: &Map<String, Value>, key: &str) -> Vec<String> {
    obj.get(key)
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn cst_node_to_value(node: &CstNode) -> VspResult<Value> {
    let snippet = node.to_string();
    parse_jsonc_value(&snippet, "cst_node")
}

fn value_to_cst_input(value: &Value) -> CstInputValue {
    match value {
        Value::Null => CstInputValue::Null,
        Value::Bool(b) => CstInputValue::Bool(*b),
        Value::Number(n) => CstInputValue::Number(n.to_string()),
        Value::String(s) => CstInputValue::String(s.clone()),
        Value::Array(items) => CstInputValue::Array(items.iter().map(value_to_cst_input).collect()),
        Value::Object(map) => CstInputValue::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), value_to_cst_input(v)))
                .collect(),
        ),
    }
}

/// Repairs JSON string escaping on newly created CST nodes because `jsonc-parser 0.26`'s
/// `CstStringLit::new_escaped` only escapes `"` and does not escape backslashes or control
/// characters (`\`, `\n`, `\t`, etc.).
fn fix_cst_strings(node: &CstNode, value: &Value) -> VspResult<()> {
    match value {
        Value::String(s) => {
            if let Some(lit) = node.as_string_lit() {
                let escaped = serde_json::to_string(s).map_err(|err| {
                    VspError::Operational(format!("JSON string escaping error: {err}"))
                })?;
                lit.set_raw_value(escaped);
            }
        }
        Value::Array(items) => {
            if let Some(arr) = node.as_array() {
                for (elem, item_val) in arr.elements().iter().zip(items.iter()) {
                    fix_cst_strings(elem, item_val)?;
                }
            }
        }
        Value::Object(map) => {
            if let Some(obj) = node.as_object() {
                for (prop, (k, v)) in obj.properties().iter().zip(map.iter()) {
                    fix_prop_name(prop, k)?;
                    if let Some(val_node) = prop.value() {
                        fix_cst_strings(&val_node, v)?;
                    }
                }
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
    Ok(())
}

fn fix_prop_name(prop: &CstObjectProp, key: &str) -> VspResult<()> {
    if let Some(str_lit) = prop.name().and_then(|n| n.as_string_lit()) {
        let escaped = serde_json::to_string(key)
            .map_err(|err| VspError::Operational(format!("JSON key escaping error: {err}")))?;
        str_lit.set_raw_value(escaped);
    }
    Ok(())
}

fn set_prop_value_safe(prop: &CstObjectProp, value: &Value) -> VspResult<()> {
    prop.set_value(value_to_cst_input(value));
    if let Some(val_node) = prop.value() {
        fix_cst_strings(&val_node, value)?;
    }
    Ok(())
}

fn append_prop_safe(obj: &CstObject, key: &str, value: &Value) -> VspResult<()> {
    let prop = obj.append(key, value_to_cst_input(value));
    fix_prop_name(&prop, key)?;
    if let Some(val_node) = prop.value() {
        fix_cst_strings(&val_node, value)?;
    }
    Ok(())
}

fn append_value_to_cst_array(arr: &CstArray, value: &Value) -> VspResult<()> {
    let node = arr.append(value_to_cst_input(value));
    fix_cst_strings(&node, value)?;
    Ok(())
}

fn replace_cst_node_safe(node: CstNode, value: &Value) -> VspResult<()> {
    let replaced = match node {
        CstNode::Container(c) => match c {
            jsonc_parser::cst::CstContainerNode::Object(o) => {
                o.replace_with(value_to_cst_input(value))
            }
            jsonc_parser::cst::CstContainerNode::Array(a) => {
                a.replace_with(value_to_cst_input(value))
            }
            _ => None,
        },
        CstNode::Leaf(l) => match l {
            jsonc_parser::cst::CstLeafNode::StringLit(s) => {
                s.replace_with(value_to_cst_input(value))
            }
            jsonc_parser::cst::CstLeafNode::NumberLit(n) => {
                n.replace_with(value_to_cst_input(value))
            }
            jsonc_parser::cst::CstLeafNode::BooleanLit(b) => {
                b.replace_with(value_to_cst_input(value))
            }
            jsonc_parser::cst::CstLeafNode::NullKeyword(n) => {
                n.replace_with(value_to_cst_input(value))
            }
            jsonc_parser::cst::CstLeafNode::WordLit(w) => w.replace_with(value_to_cst_input(value)),
            _ => None,
        },
    };
    if let Some(new_node) = replaced {
        fix_cst_strings(&new_node, value)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn preserves_all_comments_and_user_keys_across_updates_and_deletions() {
        let input = r#"// Top file banner comment
{
  /* Block header comment */
  "user.customKey": "keep-me", // inline user comment
  // Managed font size comment
  "editor.fontSize": 13, // inline managed comment
  "[typescript]": {
    // User sub-key comment inside [typescript]
    "editor.tabSize": 4, // keep user sub-key
    "editor.defaultFormatter": "esbenp.prettier-vscode",
    "editor.codeActionsOnSave": {
      "source.fixAll.eslint": "explicit"
    }
  },
  "retired.managed.key": true
}
"#;
        let desired_val = json!({
            "editor.fontSize": 14,
            "[typescript]": {
                "editor.defaultFormatter": "biomejs.biome",
                "editor.codeActionsOnSave": {
                    "source.fixAll.biome": "explicit"
                }
            },
            "new.escaped": "path\\with\\backslashes \"and\" quotes\nline2"
        });
        let desired = desired_val.as_object().expect("obj");
        let opts = CstMergeOptions {
            delete_keys: vec!["retired.managed.key".to_string()],
            delete_paths: vec![vec![
                "[typescript]".to_string(),
                "editor.codeActionsOnSave".to_string(),
                "source.fixAll.eslint".to_string(),
            ]],
            ..Default::default()
        };

        let merged = merge_jsonc_object_str(input, desired, &opts).expect("merge");
        assert!(merged.contains("// Top file banner comment"));
        assert!(merged.contains("/* Block header comment */"));
        assert!(merged.contains("// inline user comment"));
        assert!(merged.contains("// Managed font size comment"));
        assert!(merged.contains("// inline managed comment"));
        assert!(merged.contains("// User sub-key comment inside [typescript]"));
        assert!(merged.contains("// keep user sub-key"));

        let parsed = parse_jsonc_value(&merged, "merged").expect("valid jsonc");
        assert_eq!(parsed["user.customKey"], "keep-me");
        assert_eq!(parsed["editor.fontSize"], 14);
        assert_eq!(parsed["[typescript]"]["editor.tabSize"], 4);
        assert_eq!(
            parsed["[typescript]"]["editor.defaultFormatter"],
            "biomejs.biome"
        );
        assert_eq!(
            parsed["[typescript]"]["editor.codeActionsOnSave"]["source.fixAll.biome"],
            "explicit"
        );
        assert!(parsed["[typescript]"]["editor.codeActionsOnSave"]
            .get("source.fixAll.eslint")
            .is_none());
        assert!(parsed.get("retired.managed.key").is_none());
        assert_eq!(
            parsed["new.escaped"],
            "path\\with\\backslashes \"and\" quotes\nline2"
        );
    }

    /// Opus M3: a duplicate key cannot keep an unmanaged value effective.
    #[test]
    fn duplicate_keys_are_collapsed_so_the_managed_value_is_effective() {
        let existing = r#"{
  // first occurrence
  "security.workspace.trust.untrustedFiles": "open",
  "my.key": 1, // user comment
  "editor.quickSuggestions": { "other": "off" },
  /* second occurrence: the one VS Code reads */
  "security.workspace.trust.untrustedFiles": "open",
  "editor.quickSuggestions": { "comments": "on", "other": "off", "other": "inline" },
  "stale.key": true,
  "stale.key": false
}
"#;
        let mut desired = Map::new();
        desired.insert(
            "security.workspace.trust.untrustedFiles".to_string(),
            Value::String("prompt".to_string()),
        );
        desired.insert(
            "editor.quickSuggestions".to_string(),
            serde_json::json!({ "other": "on" }),
        );
        let merged = merge_jsonc_object_str(
            existing,
            &desired,
            &CstMergeOptions {
                delete_keys: vec!["stale.key".to_string()],
                ..Default::default()
            },
        )
        .expect("merge");

        assert_eq!(
            merged
                .matches("security.workspace.trust.untrustedFiles")
                .count(),
            1,
            "{merged}"
        );
        assert_eq!(
            merged.matches("\"editor.quickSuggestions\"").count(),
            1,
            "{merged}"
        );
        assert_eq!(merged.matches("\"other\"").count(), 1, "{merged}");
        assert!(!merged.contains("stale.key"), "{merged}");
        assert!(!merged.contains("\"open\""), "{merged}");
        assert!(merged.contains("// user comment"), "{merged}");

        let parsed = parse_jsonc_value(&merged, "merged").expect("valid JSONC");
        assert_eq!(parsed["security.workspace.trust.untrustedFiles"], "prompt");
        assert_eq!(parsed["editor.quickSuggestions"]["other"], "on");
        // The user's member of the effective (last) object is kept.
        assert_eq!(parsed["editor.quickSuggestions"]["comments"], "on");
        assert_eq!(parsed["my.key"], 1);
    }

    #[test]
    fn post_merge_verification_reports_a_value_that_is_not_effective() {
        let desired: Map<String, Value> =
            serde_json::from_str(r#"{"a.b": 1, "obj": {"x": {"y": 2}}, "arr": [1, 2]}"#)
                .expect("desired");
        let union: BTreeSet<String> = ["arr".to_string()].into_iter().collect();
        let ok: Map<String, Value> = serde_json::from_str(
            r#"{"a.b": 1, "obj": {"x": {"y": 2, "z": 3}}, "arr": [0, 1, 2], "user": true}"#,
        )
        .expect("ok");
        assert!(verify_desired_applied(&desired, &ok, &union, true).is_ok());

        let wrong_leaf: Map<String, Value> =
            serde_json::from_str(r#"{"a.b": 1, "obj": {"x": {"y": 9}}, "arr": [1, 2]}"#)
                .expect("wrong");
        assert_eq!(
            verify_desired_applied(&desired, &wrong_leaf, &union, true),
            Err("obj.x.y".to_string())
        );
        let missing: Map<String, Value> =
            serde_json::from_str(r#"{"obj": {"x": {"y": 2}}, "arr": [1, 2]}"#).expect("missing");
        assert_eq!(
            verify_desired_applied(&desired, &missing, &union, true),
            Err("a.b".to_string())
        );
        let short_union: Map<String, Value> =
            serde_json::from_str(r#"{"a.b": 1, "obj": {"x": {"y": 2}}, "arr": [1]}"#)
                .expect("short");
        assert_eq!(
            verify_desired_applied(&desired, &short_union, &union, true),
            Err("arr".to_string())
        );
    }

    #[test]
    fn merges_union_arrays_and_tasks_preserving_comments() {
        let input = r#"{
  // Keep shellcheck comment
  "shellcheck.exclude": [
    "SC1090" // existing rule
  ]
}
"#;
        let desired = json!({
            "shellcheck.exclude": ["SC1090", "SC2034"]
        });
        let merged = merge_jsonc_object_str(
            input,
            desired.as_object().expect("obj"),
            &CstMergeOptions::default(),
        )
        .expect("merge");
        assert!(merged.contains("// Keep shellcheck comment"));
        assert!(merged.contains("// existing rule"));
        let parsed = parse_jsonc_value(&merged, "merged").expect("valid");
        assert_eq!(parsed["shellcheck.exclude"], json!(["SC1090", "SC2034"]));

        let tasks_in = r#"{
  // Custom workspace tasks
  "version": "2.0.0",
  "tasks": [
    {
      // User custom task
      "label": "user: lint",
      "type": "shell",
      "command": "echo hi"
    },
    {
      "label": "rust: doc",
      "type": "shell",
      "command": "old-cmd"
    }
  ]
}
"#;
        let tasks_desired = json!({
            "version": "2.0.0",
            "tasks": [
                { "label": "rust: doc", "type": "shell", "command": "cargo doc" },
                { "label": "rust: doc strict", "type": "shell", "command": "env RUSTDOCFLAGS=-Dwarnings cargo doc" }
            ]
        });
        let tasks_out = merge_workspace_tasks_jsonc(tasks_in, &tasks_desired).expect("tasks merge");
        assert!(tasks_out.contains("// Custom workspace tasks"));
        assert!(tasks_out.contains("// User custom task"));
        let tp = parse_jsonc_value(&tasks_out, "tasks").expect("valid tasks");
        let arr = tp["tasks"].as_array().expect("tasks array");
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0]["label"], "user: lint");
        assert_eq!(arr[1]["command"], "cargo doc");
        assert_eq!(arr[2]["label"], "rust: doc strict");
    }

    #[test]
    fn appends_multiple_new_tasks_to_existing_tasks_jsonc() {
        let tasks_in = r#"{
  // Keep user workspace banner
  "version": "2.0.0",
  "tasks": [
    {
      // User custom task
      "label": "user: build",
      "type": "shell",
      "command": "make build"
    }
  ]
}
"#;
        let tasks_desired = json!({
            "version": "2.0.0",
            "tasks": [
                { "label": "rust: doc", "type": "shell", "command": "cargo doc" },
                { "label": "rust: doc strict", "type": "shell", "command": "cargo doc --no-deps" },
                { "label": "rust: test", "type": "shell", "command": "cargo test" },
                { "label": "rust: doc", "type": "shell", "command": "cargo doc --workspace" }
            ]
        });
        let tasks_out =
            merge_workspace_tasks_jsonc(tasks_in, &tasks_desired).expect("multi-task append");
        assert!(tasks_out.contains("// Keep user workspace banner"));
        assert!(tasks_out.contains("// User custom task"));
        let tp = parse_jsonc_value(&tasks_out, "tasks").expect("valid merged tasks");
        let arr = tp["tasks"].as_array().expect("tasks array");
        assert_eq!(arr.len(), 4);
        assert_eq!(arr[0]["label"], "user: build");
        assert_eq!(arr[1]["label"], "rust: doc");
        assert_eq!(arr[1]["command"], "cargo doc --workspace");
        assert_eq!(arr[2]["label"], "rust: doc strict");
        assert_eq!(arr[3]["label"], "rust: test");
    }
}
