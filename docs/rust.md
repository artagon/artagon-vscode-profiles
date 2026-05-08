For Rust in VS Code, “automatic documentation visible in the IDE” has 4 separate layers:

1. rustdoc comments (///)
2. rust-analyzer hover/signature docs
3. generated local HTML docs (cargo doc)
4. external crate docs (docs.rs / stdlib)

You want all 4 enabled.

Add this section to your .md file.

---
# Rust API Documentation in VS Code
## Goal
Enable:
- hover documentation
- signature help
- inline API docs
- crate documentation lookup
- stdlib documentation lookup
- generated local docs
- docs visible to CLI agents through MCP/rust-analyzer
- automatic docs refresh during development
This should work for:
- your own crates
- workspace crates
- dependencies
- stdlib APIs
---
# 1. rustdoc Comments
Rust documentation comes from:
```rust
/// Documentation comment
```
Example:
```rust
/// Interns a symbol and returns a stable identifier.
///
/// # Errors
///
/// Returns an error if the symbol exceeds configured limits.
///
/// # Performance
///
/// Expected O(1) lookup after hashing.
///
/// # Safety
///
/// Caller must ensure UTF-8 validity if bypassing checked APIs.
pub fn intern(&self, symbol: &str) -> Result<SymbolId, InternError> {
    todo!()
}
```
This powers:
- hover docs
- signature help
- generated docs
- docs.rs style rendering
- IDE docs
- CLI semantic context
---
# 2. Enable rust-analyzer Hover Docs
Add to `.vscode/settings.json`
```json
{
  "rust-analyzer.hover.actions.enable": true,
  "rust-analyzer.hover.documentation.enable": true,
  "rust-analyzer.hover.links.enable": true,
  "rust-analyzer.signatureInfo.documentation.enable": true,
  "editor.hover.enabled": true,
  "editor.hover.delay": 300,
  "editor.parameterHints.enabled": true
}
```
This enables:
- hover docs
- function signatures
- trait docs
- type docs
- method docs
- links to definitions
Hover over:
```rust
Vec::push
HashMap
Arc
RwLock
```
and docs appear automatically.
---
# 3. Generate Local HTML Docs
Generate workspace docs:
```bash
cargo doc --workspace --all-features --no-deps
```
Generated docs:
```text
target/doc/
```
Open:
```text
target/doc/index.html
```
Or serve locally:
```bash
python3 -m http.server -d target/doc 8000
```
Open:
```text
http://localhost:8000
```
This gives:
- clickable crate docs
- trait hierarchy
- impl docs
- module docs
- intra-doc links
Like local docs.rs.
---
# 4. Strict Documentation Gate
Require docs to compile cleanly.
Run:
```bash
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps
```
This catches:
- broken intra-doc links
- doc warnings
- malformed docs
- invalid examples
Recommended for OpenSpec verification.
---
# 5. Intra-doc Links
Use proper Rust intra-doc links.
Example:
```rust
/// Uses [`SymbolInterner`] internally.
///
/// See also [`crate::matcher::Matcher`].
```
This creates clickable IDE/docs links.
---
# 6. Documentation for Workspace Crates
For monorepos:
```text
crates/*
```
workspace docs automatically include:
- parser crate
- runtime crate
- allocator crate
- matcher crate
when using:
```bash
cargo doc --workspace
```
---
# 7. Dependency Documentation
Open dependency docs via hover:
```rust
serde::Serialize
tokio::spawn
bytes::Bytes
```
rust-analyzer pulls docs from dependency metadata automatically.
---
# 8. Stdlib Documentation
Hover over:
```rust
Vec
Option
Result
Arc
RwLock
Iterator
```
Docs come from Rust stdlib source metadata.
---
# 9. Auto-open Docs in Browser
Optional VS Code extension:
```bash
code --install-extension rust-lang.rust-analyzer
```
rust-analyzer provides:
```text
Open Docs
```
actions for symbols.
Right-click symbol:
```text
Go to Definition
Peek Definition
Open Documentation
```
depending on symbol/source availability.
---
# 10. Documentation Search in Editor
Use:
```text
CMD+T
CTRL+T
```
to search symbols across workspace.
Hover reveals docs instantly.
This is much faster than browsing trees.
---
# 11. Documentation Tasks
Add to `.vscode/tasks.json`
```json
{
  "label": "rust: doc",
  "type": "shell",
  "command": "cargo doc --workspace --all-features --no-deps",
  "problemMatcher": "$rustc"
},
{
  "label": "rust: doc strict",
  "type": "shell",
  "command": "RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps",
  "problemMatcher": "$rustc"
}
```
> **CLI-emitted form**: when `vspcli --detect` writes these tasks, both `command` fields are prefixed with `rtk` (and the strict variant uses `rtk env RUSTDOCFLAGS=...`) per the workspace's RTK contract. The bare form above is the general-audience reference.
---
# 12. Docs Policy for AI Agents
Add to:
```text
AGENTS.md
CLAUDE.md
CODEX.md
GEMINI.md
```
```md
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
```
---
# 13. Recommended Documentation Workflow
```text
write rustdoc
 → hover docs update automatically
 → generate local docs
 → verify docs compile cleanly
 → expose docs to CLI agents through rust-analyzer/MCP
```
---
# 14. Recommended Final Documentation Stack
```text
rustdoc comments
rust-analyzer hover docs
signature help
local cargo doc HTML
OpenSpec docs
Foam wiki docs
MCP semantic context
```
---
# Reality Check / Adversarial Review
Verified:
- rustdoc comments power hover docs
- cargo doc workflow
- rust-analyzer hover/signature docs
- intra-doc links
- local HTML docs generation
- strict rustdoc verification
Explicitly NOT assumed:
- Smart Context MCP guaranteed hover-doc exposure
- perfect macro-generated hover docs
- docs.rs offline mirroring
- automatic browser opening across all platforms
Treat MCP documentation capabilities as runtime-discovered.
