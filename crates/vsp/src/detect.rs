//! Data-driven, strictly read-only workspace toolchain and capability detector.
//!
//! Mirrors `scripts/detect-toolchain.sh`: evaluates every module with a non-null
//! `detect` specification in ascending `(priority, fallback, id)` order, handles
//! primary vs fallback groups, applies `detect.supersedes[]`, and records full
//! rule explanations for `vsp detect --explain`.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::Serialize;

use crate::exit_codes::{VspError, VspResult};
use crate::model::{DetectMatchMode, DetectRule, WorkspaceCatalog};

/// Directories pruned during `glob` file searches.
const GLOB_PRUNE_DIRS: &[&str] = &[".git", "node_modules", "target", "build", ".venv", ".cache"];

/// Directories pruned during `file_and_grep` searches.
const GREP_PRUNE_DIRS: &[&str] = &[".git", "node_modules", "target", "build", ".venv"];

/// Explanation record for a single superseded module.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SupersededMatch {
    /// Module ID that matched but was superseded.
    pub module: String,
    /// Higher-precedence module ID that superseded it.
    pub superseded_by: String,
    /// Signals that originally triggered the superseded module.
    pub signals: Vec<String>,
}

/// Complete read-only detection output.
#[derive(Debug, Clone, Serialize)]
pub struct DetectionResult {
    /// Final active detected module IDs in priority order.
    pub toolchains: Vec<String>,
    /// Matched signal strings per active module ID.
    pub signals: BTreeMap<String, Vec<String>>,
    /// Whether this result came from an explicit `--toolchain` override.
    #[serde(rename = "override")]
    pub is_override: bool,
    /// Modules that matched rules but were superseded by another matched module.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub superseded: Vec<SupersededMatch>,
}

/// Evaluates all catalog detection rules against `target_path` without modifying any files.
///
/// # Errors
///
/// Returns [`VspError::TargetMissing`] (exit `2`) if `target_path` does not exist or is
/// not a directory, or [`VspError::Operational`] on unreadable workspace traversal errors.
pub fn detect_workspace(
    catalog: &WorkspaceCatalog,
    target_path: &Path,
) -> VspResult<DetectionResult> {
    if !target_path.is_dir() {
        return Err(VspError::TargetMissing(format!(
            "path does not exist or is not a directory: {}",
            target_path.display()
        )));
    }
    let ws_root = fs::canonicalize(target_path).map_err(|err| {
        VspError::TargetMissing(format!(
            "cannot resolve target path {}: {err}",
            target_path.display()
        ))
    })?;

    // Collect all modules with non-null `detect`, sorted by (priority, fallback, id).
    let mut candidates: Vec<(u32, bool, &str)> = catalog
        .modules
        .values()
        .filter_map(|def| {
            def.manifest
                .detect
                .as_ref()
                .map(|det| (det.priority, det.fallback, def.manifest.id.as_str()))
        })
        .collect();
    candidates.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(b.2)));

    let mut primary_matches: Vec<String> = Vec::new();
    let mut fallback_matches: Vec<String> = Vec::new();
    let mut primary_signals: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut fallback_signals: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut primary_supersedes: Vec<(String, String)> = Vec::new();
    let mut fallback_supersedes: Vec<(String, String)> = Vec::new();

    for (_prio, is_fallback, mod_id) in candidates {
        let Some(def) = catalog.modules.get(mod_id) else {
            continue;
        };
        let Some(det) = &def.manifest.detect else {
            continue;
        };

        let mut mod_signals = Vec::new();
        let mut matched_any = false;
        let mut matched_all = true;

        for rule in &det.rules {
            match eval_rule(&ws_root, rule)? {
                Some(sig) => {
                    matched_any = true;
                    mod_signals.push(sig);
                }
                None => {
                    matched_all = false;
                }
            }
        }

        let is_match = !det.rules.is_empty()
            && match det.match_mode {
                DetectMatchMode::All => matched_all,
                DetectMatchMode::Any => matched_any,
            };

        if is_match {
            if is_fallback {
                fallback_matches.push(mod_id.to_string());
                fallback_signals.insert(mod_id.to_string(), mod_signals);
                for sup in &det.supersedes {
                    fallback_supersedes.push((mod_id.to_string(), sup.clone()));
                }
            } else {
                primary_matches.push(mod_id.to_string());
                primary_signals.insert(mod_id.to_string(), mod_signals);
                for sup in &det.supersedes {
                    primary_supersedes.push((mod_id.to_string(), sup.clone()));
                }
            }
        }
    }

    let (active, mut signals, supersedes_list) = if !primary_matches.is_empty() {
        (primary_matches, primary_signals, primary_supersedes)
    } else if !fallback_matches.is_empty() {
        (fallback_matches, fallback_signals, fallback_supersedes)
    } else {
        (Vec::new(), BTreeMap::new(), Vec::new())
    };

    let mut final_toolchains = Vec::new();
    let mut superseded = Vec::new();
    for mod_id in active {
        if let Some((winner, _)) = supersedes_list.iter().find(|(_, loser)| loser == &mod_id) {
            let removed_sigs = signals.remove(&mod_id).unwrap_or_default();
            superseded.push(SupersededMatch {
                module: mod_id,
                superseded_by: winner.clone(),
                signals: removed_sigs,
            });
            continue;
        }
        final_toolchains.push(mod_id);
    }

    Ok(DetectionResult {
        toolchains: final_toolchains,
        signals,
        is_override: false,
        superseded,
    })
}

fn eval_rule(ws_root: &Path, rule: &DetectRule) -> VspResult<Option<String>> {
    match rule {
        DetectRule::File { path } => {
            if !path.is_empty() && ws_root.join(path).is_file() {
                Ok(Some(path.clone()))
            } else {
                Ok(None)
            }
        }
        DetectRule::AnyFile { paths } => {
            for p in paths {
                if !p.is_empty() && ws_root.join(p).is_file() {
                    return Ok(Some(p.clone()));
                }
            }
            Ok(None)
        }
        DetectRule::AllFiles { paths, any_extra } => {
            let mut matched = Vec::new();
            for p in paths {
                if p.is_empty() || !ws_root.join(p).is_file() {
                    return Ok(None);
                }
                matched.push(p.clone());
            }
            if matched.is_empty() {
                return Ok(None);
            }
            if !any_extra.is_empty() {
                match any_extra.iter().find(|p| !p.is_empty() && ws_root.join(p).is_file()) {
                    Some(extra) => matched.push(extra.clone()),
                    None => return Ok(None),
                }
            }
            Ok(Some(matched.join(" ")))
        }
        DetectRule::Glob {
            pattern,
            dir,
            max_depth,
        } => {
            if pattern.is_empty() {
                return Ok(None);
            }
            let search_dir = if dir == "." {
                ws_root.to_path_buf()
            } else {
                ws_root.join(dir)
            };
            if !search_dir.is_dir() {
                return Ok(None);
            }
            if let Some(hit) =
                find_first_matching_file(&search_dir, pattern, *max_depth, GLOB_PRUNE_DIRS)?
            {
                let rel = hit
                    .strip_prefix(ws_root)
                    .unwrap_or(&hit)
                    .to_string_lossy()
                    .replace('\\', "/");
                return Ok(Some(rel));
            }
            Ok(None)
        }
        DetectRule::FileAndAbsent { present, absent } => {
            if !present.is_empty()
                && ws_root.join(present).is_file()
                && (absent.is_empty() || !ws_root.join(absent).exists())
            {
                Ok(Some(present.clone()))
            } else {
                Ok(None)
            }
        }
        DetectRule::FileAndGrep {
            paths,
            search_dir,
            glob,
            pattern,
        } => {
            let Some(found_manifest) = paths
                .iter()
                .find(|p| !p.is_empty() && ws_root.join(p).is_file())
            else {
                return Ok(None);
            };
            if pattern.is_empty() {
                return Ok(None);
            }
            let search_base = if search_dir == "." {
                ws_root.to_path_buf()
            } else {
                ws_root.join(search_dir)
            };
            if !search_base.is_dir() {
                return Ok(None);
            }
            let mut candidates = Vec::new();
            collect_matching_files(&search_base, glob, 8, GREP_PRUNE_DIRS, 1, &mut candidates)?;
            for cand in candidates {
                if let Ok(content) = fs::read_to_string(&cand) {
                    if content.contains(pattern.as_str()) {
                        let rel = cand
                            .strip_prefix(ws_root)
                            .unwrap_or(&cand)
                            .to_string_lossy()
                            .replace('\\', "/");
                        return Ok(Some(format!("{found_manifest}:{rel}")));
                    }
                }
            }
            Ok(None)
        }
    }
}

fn find_first_matching_file(
    base: &Path,
    pattern: &str,
    max_depth: usize,
    prune: &[&str],
) -> VspResult<Option<std::path::PathBuf>> {
    let mut matches = Vec::new();
    collect_matching_files(base, pattern, max_depth, prune, 1, &mut matches)?;
    Ok(matches.into_iter().next())
}

fn collect_matching_files(
    dir: &Path,
    pattern: &str,
    max_depth: usize,
    prune: &[&str],
    depth: usize,
    out: &mut Vec<std::path::PathBuf>,
) -> VspResult<()> {
    if depth > max_depth {
        return Ok(());
    }
    let read_dir = fs::read_dir(dir).map_err(|err| {
        VspError::TargetMissing(format!("cannot read directory {}: {err}", dir.display()))
    })?;
    let mut entries: Vec<_> = read_dir.filter_map(Result::ok).collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let path = entry.path();
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        if ft.is_dir() {
            if prune.contains(&name_str) {
                continue;
            }
            if depth < max_depth {
                collect_matching_files(&path, pattern, max_depth, prune, depth + 1, out)?;
            }
        } else if ft.is_file() && wildcard_match(pattern, name_str) {
            out.push(path);
        }
    }
    Ok(())
}

/// Simple deterministic filename glob matcher supporting `*` and `?`.
#[must_use]
pub fn wildcard_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star_pi, mut star_ti) = (None, 0usize);

    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star_pi = Some(pi);
            star_ti = ti;
            pi += 1;
        } else if let Some(sp) = star_pi {
            pi = sp + 1;
            star_ti += 1;
            ti = star_ti;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::legacy_loader::load_legacy_catalog;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("repo root")
            .to_path_buf()
    }

    #[test]
    fn wildcard_match_handles_globs() {
        assert!(wildcard_match("*.bats", "cli.bats"));
        assert!(!wildcard_match("*.bats", "cli.sh"));
        assert!(wildcard_match("*Application.java", "DemoApplication.java"));
        assert!(wildcard_match("?.yml", "a.yml"));
        assert!(!wildcard_match("?.yml", "ab.yml"));
        assert!(wildcard_match("*", "anything"));
    }

    #[test]
    fn detect_handles_supersedes_and_pruned_dirs() {
        let catalog = load_legacy_catalog(&repo_root()).expect("catalog");
        let tmp = TempDir::new().expect("tmp");

        // Astro + tsconfig.json -> astro supersedes ts-base.
        fs::write(tmp.path().join("package.json"), "{}").expect("pkg");
        fs::write(tmp.path().join("astro.config.mjs"), "").expect("astro");
        fs::write(tmp.path().join("tsconfig.json"), "{}").expect("ts");

        // CMakeLists.txt + .clangd -> cpp-clangd (not cpp-intellisense).
        fs::write(tmp.path().join("CMakeLists.txt"), "").expect("cmake");
        fs::write(tmp.path().join(".clangd"), "").expect("clangd");

        // .bats inside pruned node_modules must NOT trigger shell detector.
        fs::create_dir_all(tmp.path().join("node_modules/pkg")).expect("nm");
        fs::write(tmp.path().join("node_modules/pkg/ignored.bats"), "").expect("bats");

        let res = detect_workspace(&catalog, tmp.path()).expect("detect");
        assert!(res.toolchains.contains(&"astro".to_string()));
        assert!(!res.toolchains.contains(&"ts-base".to_string()));
        assert!(res
            .superseded
            .iter()
            .any(|s| s.module == "ts-base" && s.superseded_by == "astro"));
        assert!(res.toolchains.contains(&"cpp-clangd".to_string()));
        assert!(!res.toolchains.contains(&"cpp-intellisense".to_string()));
        assert!(!res.toolchains.contains(&"shell".to_string()));
    }

    #[test]
    fn detect_handles_file_and_grep_for_spring_boot() {
        let catalog = load_legacy_catalog(&repo_root()).expect("catalog");
        let tmp = TempDir::new().expect("tmp");
        fs::write(tmp.path().join("build.gradle.kts"), "").expect("gradle");
        fs::write(tmp.path().join("pom.xml"), "").expect("pom");
        fs::create_dir_all(tmp.path().join("src/main/java/com/example")).expect("java dir");
        fs::write(
            tmp.path()
                .join("src/main/java/com/example/DemoApplication.java"),
            "@SpringBootApplication\npublic class DemoApplication {}\n",
        )
        .expect("app");

        let res = detect_workspace(&catalog, tmp.path()).expect("detect");
        assert!(res.toolchains.contains(&"java-gradle".to_string()));
        assert!(res.toolchains.contains(&"java-spring".to_string()));
        assert!(!res.toolchains.contains(&"java-maven".to_string()));
        assert!(res
            .superseded
            .iter()
            .any(|s| s.module == "java-maven" && s.superseded_by == "java-gradle"));
    }
}
