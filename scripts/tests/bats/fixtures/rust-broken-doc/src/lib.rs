//! Broken fixture crate — has a deliberately broken intra-doc link
//! that fails under `RUSTDOCFLAGS="-D warnings"`. Used by
//! rust-docs-strict-gate.bats to assert the gate actually catches
//! malformed rustdoc.

/// Calls [`SymbolThatDoesNotExist`] internally.
///
/// This intra-doc link is intentionally broken so the strict-docs
/// gate fails on this fixture.
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}
