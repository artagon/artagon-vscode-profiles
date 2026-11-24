<!-- BEGIN RUST-DOCS-POLICY -->
## Documentation Policy
Before changing Rust APIs:
1. inspect hover docs
2. inspect trait/type docs
3. inspect signatures
4. inspect generated rustdoc
5. inspect OpenSpec docs
6. inspect local architecture docs
Public API changes require:
- rustdoc updates
- architecture doc updates if behavior changed
- benchmark doc updates if performance changed
Required verification:
```bash
cargo doc --workspace --all-features --no-deps
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps
```
<!-- END RUST-DOCS-POLICY -->
