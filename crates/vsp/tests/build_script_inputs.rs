//! Astra A14: the build script rejects symbolic links for the required top-level snapshot
//! inputs and for the `packs` root.
//!
//! Cargo watches the resolved path of an input. If an input were a symbolic link, the link
//! could be retargeted while the old target stays unchanged, and an incremental build would
//! embed stale bytes. The build script is included here as a module so that its input check
//! runs against a temporary tree.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::symlink;

use tempfile::TempDir;

#[allow(dead_code)]
#[path = "../build.rs"]
mod build_script;

use build_script::reject_symlink_components;

#[test]
fn a14_build_script_rejects_symlinked_required_inputs_and_packs_root() {
    let tmp = TempDir::new().expect("tmp");
    let root = tmp.path().join("repo");
    let elsewhere = tmp.path().join("elsewhere");
    fs::create_dir_all(root.join("catalog")).expect("mkdir catalog");
    fs::create_dir_all(root.join("packs")).expect("mkdir packs");
    fs::create_dir_all(elsewhere.join("packs-v1")).expect("mkdir elsewhere");
    fs::write(root.join("catalog/extensions.jsonc"), "{}").expect("write extensions");
    fs::write(root.join("presets.jsonc"), "{}").expect("write presets");
    fs::write(elsewhere.join("lock-v1.json"), "{}").expect("write lock v1");

    // Real files and directories are accepted. A missing input is not this check's error.
    for rel in [
        "catalog/extensions.jsonc",
        "presets.jsonc",
        "packs",
        "vsp.lock.json",
    ] {
        assert!(reject_symlink_components(&root, rel).is_ok(), "{rel}");
    }

    // Required file that is a symbolic link (also to a file inside the repository).
    symlink(elsewhere.join("lock-v1.json"), root.join("vsp.lock.json")).expect("link lock");
    let err = reject_symlink_components(&root, "vsp.lock.json").expect_err("linked file");
    assert!(err.contains("symlinks are forbidden"), "{err}");
    fs::write(root.join("presets-real.jsonc"), "{}").expect("write real presets");
    fs::remove_file(root.join("presets.jsonc")).expect("rm presets");
    symlink(root.join("presets-real.jsonc"), root.join("presets.jsonc")).expect("link presets");
    assert!(reject_symlink_components(&root, "presets.jsonc").is_err());

    // Symbolic link at a parent directory of a required file.
    fs::remove_dir_all(root.join("catalog")).expect("rm catalog");
    fs::create_dir_all(elsewhere.join("catalog")).expect("mkdir linked catalog");
    fs::write(elsewhere.join("catalog/extensions.jsonc"), "{}").expect("write linked file");
    symlink(elsewhere.join("catalog"), root.join("catalog")).expect("link catalog dir");
    assert!(reject_symlink_components(&root, "catalog/extensions.jsonc").is_err());

    // Symbolic link at the packs root.
    fs::remove_dir_all(root.join("packs")).expect("rm packs");
    symlink(elsewhere.join("packs-v1"), root.join("packs")).expect("link packs");
    assert!(reject_symlink_components(&root, "packs").is_err());

    // Path traversal components are rejected.
    assert!(reject_symlink_components(&root, "../elsewhere/lock-v1.json").is_err());
}

/// The real repository inputs pass the same check that the build script runs.
#[test]
fn a14_repository_inputs_are_real_files() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("repo root")
        .to_path_buf();
    let canon = fs::canonicalize(root).expect("canonical repo root");
    for rel in [
        "catalog/extensions.jsonc",
        "catalog/policy.jsonc",
        "presets.jsonc",
        "vsp.lock.json",
        "packs",
    ] {
        assert!(reject_symlink_components(&canon, rel).is_ok(), "{rel}");
    }
}
