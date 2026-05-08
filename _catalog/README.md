# `_catalog/`

Project catalog files — small, sparse, mostly-static data used by the
shell tooling. Not generated; edit by hand and commit.

## `extension-pins.json`

VSIX pin manifest. When an entry exists for an extension ID, the
install scripts (`scripts/import-profile.sh`,
`scripts/install-extensions.sh`) fetch the VSIX from the recorded
`vsix_url`, verify its `sha256`, and install offline. Defends
against marketplace publish-chain compromise for high-blast
extensions (themes ship in every bundle).

**Schema:**

```json
{
  "version": 1,
  "pins": {
    "<publisher>.<name>": {
      "version": "<x.y.z>",
      "sha256": "<lowercase hex>",
      "vsix_url": "https://<publisher>.gallery.vsassets.io/_apis/public/gallery/publisher/<publisher>/extension/<name>/<version>/assetbyname/Microsoft.VisualStudio.Services.VSIXPackage"
    }
  }
}
```

**Adding a pin** (manual; CI workflow is a follow-up):

```bash
# 1. Find the latest stable version on https://marketplace.visualstudio.com/items?itemName=<id>
# 2. Construct the VSIX URL using the template above.
# 3. Fetch and hash:
curl -fsSL "<url>" -o /tmp/ext.vsix && shasum -a 256 /tmp/ext.vsix
# 4. Add the entry to this file.
```

**Bypass:** `ARTAGON_VSCODE_BYPASS_PINS=1` skips pin lookup entirely
(every install hits the live marketplace). Emits a one-time stderr
notice per process.

**Failure mode is closed.** Hash mismatch or fetch failure refuses
the install — no live-marketplace fallback. The bypass env var is
the only escape hatch.
