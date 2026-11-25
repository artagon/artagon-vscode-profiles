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
/// - [`VspError::TargetMissing`] (exit `2`): `target_path` does not exist or is not a
///   directory.
/// - [`VspError::IncompleteObservation`] (exit `7`): the target, a directory in it, or a
///   source file that a rule must read exists but cannot be read (for example permission
///   denied). The scan is incomplete, so no "no match" result is returned.
pub fn detect_workspace(
    catalog: &WorkspaceCatalog,
    target_path: &Path,
) -> VspResult<DetectionResult> {
    // A missing target is exit 2. A target that exists but cannot be examined (for example
    // permission denied on a parent directory) is an incomplete scan (exit 7), not "missing".
    match fs::metadata(target_path) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => {
            return Err(VspError::TargetMissing(format!(
                "path does not exist or is not a directory: {}",
                target_path.display()
            )));
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(VspError::TargetMissing(format!(
                "path does not exist or is not a directory: {}",
                target_path.display()
            )));
        }
        Err(err) => return Err(scan_incomplete(target_path, &err)),
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
                // Read bytes, not UTF-8 text: a source file in another encoding can still
                // contain the pattern. A file that vanished is skipped. Every other read
                // failure means the scan is incomplete, so "no match" cannot be reported.
                let content = match fs::read(&cand) {
                    Ok(bytes) => bytes,
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(err) => return Err(scan_incomplete(&cand, &err)),
                };
                if contains_bytes(&content, pattern.as_bytes()) {
                    let rel = cand
                        .strip_prefix(ws_root)
                        .unwrap_or(&cand)
                        .to_string_lossy()
                        .replace('\\', "/");
                    return Ok(Some(format!("{found_manifest}:{rel}")));
                }
            }
            Ok(None)
        }
    }
}

/// Builds the error for a scan that could not examine `path`. The detector then reports an
/// incomplete observation (exit `7`) and never a complete "no match" (exit `3`).
fn scan_incomplete(path: &Path, err: &std::io::Error) -> VspError {
    VspError::IncompleteObservation(format!(
        "detect: scan incomplete, cannot read {}: {err}",
        path.display()
    ))
}

/// Returns `true` when `haystack` contains the byte sequence `needle` (not empty).
fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|window| window == needle)
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
    // A directory that vanished during the scan is absent. Every other failure (permission
    // denied, I/O error) is an incomplete scan and is propagated: it is never dropped.
    let read_dir = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(scan_incomplete(dir, &err)),
    };
    let mut entries = Vec::new();
    for entry in read_dir {
        entries.push(entry.map_err(|err| scan_incomplete(dir, &err))?);
    }
    entries.sort_by_key(std::fs::DirEntry::file_name);

    for entry in entries {
        let path = entry.path();
        let ft = match entry.file_type() {
            Ok(ft) => ft,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => return Err(scan_incomplete(&path, &err)),
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

    /// Astra A12: an unreadable directory or source file is an incomplete scan (exit 7),
    /// and a source file that is not UTF-8 is still searched.
    #[cfg(unix)]
    #[test]
    fn unreadable_paths_are_incomplete_scans_and_non_utf8_sources_match() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = TempDir::new().expect("tmp");
        let ws = tmp.path();
        fs::write(ws.join("pom.xml"), "<project/>").expect("write pom");
        fs::create_dir_all(ws.join("src")).expect("mkdir src");

        // A Latin-1 byte makes the file invalid UTF-8. The pattern is still found.
        let mut latin1 = b"// caf\xe9\n@SpringBootApplication\n".to_vec();
        fs::write(ws.join("src/App.java"), &latin1).expect("write source");
        let grep = DetectRule::FileAndGrep {
            paths: vec!["pom.xml".to_string()],
            search_dir: ".".to_string(),
            glob: "*.java".to_string(),
            pattern: "@SpringBootApplication".to_string(),
        };
        assert_eq!(
            eval_rule(ws, &grep).expect("non-UTF-8 source"),
            Some("pom.xml:src/App.java".to_string())
        );
        latin1.truncate(8);
        fs::write(ws.join("src/App.java"), &latin1).expect("rewrite source");
        assert_eq!(eval_rule(ws, &grep).expect("no pattern"), None);

        // Unreadable source file.
        let secret = ws.join("src/Secret.java");
        fs::write(&secret, "@SpringBootApplication").expect("write secret");
        fs::set_permissions(&secret, fs::Permissions::from_mode(0o000)).expect("chmod file");
        if fs::read(&secret).is_ok() {
            // Running as root: permission bits do not restrict reads, so skip the rest.
            return;
        }
        let err = eval_rule(ws, &grep).expect_err("unreadable source file");
        assert_eq!(err.exit_code().as_i32(), 7, "{err}");
        fs::set_permissions(&secret, fs::Permissions::from_mode(0o644)).expect("restore file");
        fs::remove_file(&secret).expect("rm secret");

        // Unreadable directory during a glob scan.
        let locked = ws.join("locked");
        fs::create_dir_all(&locked).expect("mkdir locked");
        fs::write(locked.join("x.bats"), "").expect("write bats");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).expect("chmod dir");
        let glob = DetectRule::Glob {
            pattern: "*.bats".to_string(),
            dir: ".".to_string(),
            max_depth: 3,
        };
        let err = eval_rule(ws, &glob).expect_err("unreadable directory");
        assert_eq!(err.exit_code().as_i32(), 7, "{err}");

        // The whole-workspace detector propagates it: exit 7, not a "no match" result.
        let catalog = load_legacy_catalog(&repo_root()).expect("catalog");
        let err = detect_workspace(&catalog, ws).expect_err("incomplete detect");
        assert_eq!(err.exit_code().as_i32(), 7, "{err}");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).expect("restore dir");

        // A target that does not exist is still "missing" (exit 2).
        let err = detect_workspace(&catalog, &ws.join("nope")).expect_err("missing target");
        assert_eq!(err.exit_code().as_i32(), 2);
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
