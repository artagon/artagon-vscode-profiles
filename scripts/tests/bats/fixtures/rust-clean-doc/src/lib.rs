//! Clean fixture crate — rustdoc compiles cleanly under
//! `RUSTDOCFLAGS="-D warnings"`. Used by rust-docs-strict-gate.bats.

/// Adds two integers.
///
/// See also [`subtract`].
///
/// # Examples
///
/// ```
/// use rust_clean_doc_fixture::add;
/// assert_eq!(add(2, 3), 5);
/// ```
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

/// Subtracts two integers.
///
/// See also [`add`].
pub fn subtract(a: i32, b: i32) -> i32 {
    a - b
}
