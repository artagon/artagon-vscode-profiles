# VS Code Extension Compatibility Guide

This guide explains how to detect and handle incompatible VS Code extensions in your profiles.

> **CLI integration**: as of the `workspace-toolchain-and-ux-layering` change, `vspcli --detect --target=workspace` runs this checker automatically before writing `.vscode/extensions.json`. Default mode is `--check-compat=warn` (stderr notice; install proceeds). Set `--check-compat=block` to abort on any incompatibility, or `--check-compat=off` to skip the check entirely. The standalone invocation below is preserved for ad-hoc audits.

## Quick Start

### Check All Profiles

```bash
bash scripts/check-extension-compatibility.sh --all
```

### Check Specific Profile

```bash
bash scripts/check-extension-compatibility.sh github-workflows-crisp
```

### Check Multiple Profiles

```bash
bash scripts/check-extension-compatibility.sh java-profile-crisp web-astro-crisp
```

## Understanding Extension Compatibility

VS Code extensions specify which VS Code versions they support using the `engines.vscode` field in their `package.json`. For example:

- `^1.95.0` - Requires VS Code 1.95.0 or higher
- `>=1.90.0` - Requires VS Code 1.90.0 or higher
- `*` - Works with any VS Code version

### Common Compatibility Issues

1. **Extension too new**: Extension requires a newer VS Code version than you have installed
2. **Extension abandoned**: Extension hasn't been updated in years and may not work with modern VS Code
3. **Breaking changes**: VS Code API changes that make old extensions incompatible

## Using the Compatibility Checker

### Command Options

```bash
scripts/check-extension-compatibility.sh [OPTIONS] [PROFILE...]

OPTIONS:
  --all                 Check all profiles
  --json                Output results as JSON
  --verbose, -v         Show detailed information
  --no-cache            Skip cache, fetch fresh data
  --marketplace-only    Only check Marketplace API (faster, skips checking installed extensions)
  --help, -h            Show help message
```

### Examples

#### 1. Basic Check with Verbose Output

```bash
bash scripts/check-extension-compatibility.sh --verbose github-workflows-crisp
```

**Output:**
```
VS Code version: 1.107.1 (1.107)

📦 Checking profile: github-workflows-crisp
────────────────────────────────────────────────────────────
✓ akamud.vscode-theme-onedark (2.3.0) - Requires: ^1.12.0
✓ catppuccin.catppuccin-vsc (3.18.1) - Requires: ^1.80.0
✓ github.copilot (1.388.0) - Requires: ^1.103.0
...

════════════════════════════════════════════════════════════
📊 SUMMARY
════════════════════════════════════════════════════════════
VS Code Version: 1.107.1
Total Extensions: 27
✓ Compatible: 27
Incompatible: 0
Unknown: 0
════════════════════════════════════════════════════════════

✅ All extensions are compatible with VS Code 1.107.1
```

#### 2. Check All Profiles (Marketplace Only)

```bash
bash scripts/check-extension-compatibility.sh --all --marketplace-only
```

This skips checking if extensions are installed (faster).

#### 3. JSON Output for Automation

```bash
bash scripts/check-extension-compatibility.sh --json github-workflows-crisp | jq '.summary'
```

**Output:**
```json
{
  "total": 27,
  "compatible": 27,
  "incompatible": 0,
  "unknown": 0
}
```

#### 4. Find Incompatible Extensions

```bash
bash scripts/check-extension-compatibility.sh --json --all | \
  jq '.extensions[] | select(.status == "incompatible")'
```

#### 5. Check Without Cache (Fresh Data)

```bash
bash scripts/check-extension-compatibility.sh --no-cache github-workflows-crisp
```

## Manual Compatibility Checking

### Method 1: Using VS Code CLI

Check if an extension can be installed:

```bash
code --install-extension github.copilot
```

If incompatible, you'll see an error message.

### Method 2: Check Marketplace Website

1. Visit the [VS Code Marketplace](https://marketplace.visualstudio.com/)
2. Search for the extension
3. Check the "More Info" section for:
   - Last updated date
   - Required VS Code version
   - Download statistics

### Method 3: Check Extension Manifest

For installed extensions, check the `package.json`:

```bash
cat ~/.vscode/extensions/github.copilot-*/package.json | jq '.engines.vscode'
```

## Handling Incompatible Extensions

### Option 1: Update VS Code

```bash
# Check current version
code --version

# Update via website or package manager
# macOS (Homebrew):
brew upgrade visual-studio-code
```

### Option 2: Find Alternatives

Search for alternative extensions that provide similar functionality:

```bash
# Example: Alternative code formatters
code --search "formatter"
```

### Option 3: Remove Extension from Profile

Edit the profile's `extensions.json`:

```bash
# Remove incompatible extension
jq 'del(.[] | select(.identifier.id == "incompatible.extension"))' \
  profiles/my-profile/extensions.json > temp.json && \
  mv temp.json profiles/my-profile/extensions.json
```

### Option 4: Wait for Update

Check the extension's repository for:
- Open issues about compatibility
- Planned updates
- Community forks with fixes

## Automated Compatibility Checks

### Add to CI/CD Pipeline

```yaml
# .github/workflows/check-extensions.yml
name: Check Extension Compatibility

on: [push, pull_request]

jobs:
  compatibility:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3

      - name: Install VS Code
        run: |
          wget -qO- https://packages.microsoft.com/keys/microsoft.asc | gpg --dearmor > packages.microsoft.gpg
          sudo install -o root -g root -m 644 packages.microsoft.gpg /etc/apt/trusted.gpg.d/
          sudo sh -c 'echo "deb [arch=amd64] https://packages.microsoft.com/repos/vscode stable main" > /etc/apt/sources.list.d/vscode.list'
          sudo apt-get update
          sudo apt-get install code

      - name: Install jq and curl
        run: sudo apt-get install jq curl

      - name: Check Extension Compatibility
        run: bash scripts/check-extension-compatibility.sh --all --marketplace-only
```

### Pre-commit Hook

Create `.git/hooks/pre-commit`:

```bash
#!/usr/bin/env bash

# Only check if extensions.json files were modified
if git diff --cached --name-only | grep -q 'extensions.json'; then
  echo "Checking extension compatibility..."
  bash scripts/check-extension-compatibility.sh --all --marketplace-only || {
    echo "⚠️  Extension compatibility check failed!"
    echo "Run: bash scripts/check-extension-compatibility.sh --all"
    exit 1
  }
fi
```

Make it executable:
```bash
chmod +x .git/hooks/pre-commit
```

## Caching

The script caches Marketplace API responses in:
```
~/.cache/vscode-extension-check/
```

**Cache TTL:** 1 hour

**Clear cache:**
```bash
rm -rf ~/.cache/vscode-extension-check/
```

## Troubleshooting

### Issue: "Extension not found in Marketplace"

**Cause:** Extension may have been:
- Removed from Marketplace
- Renamed
- Made private

**Solution:**
1. Check if extension is deprecated
2. Search for replacement
3. Remove from profile

### Issue: Script Hangs or Times Out

**Cause:** Network issues or rate limiting

**Solution:**
```bash
# Try with cache
bash scripts/check-extension-compatibility.sh --all

# Or increase timeout (edit script)
# Change: timeout=30  # to longer value
```

### Issue: False Positives

**Cause:** Some extensions use flexible version requirements

**Solution:**
- Manually verify by installing
- Check extension's GitHub issues
- Test in isolated profile

## Best Practices

### 1. Regular Checks

Check compatibility after:
- VS Code updates
- Adding new extensions
- Before deploying profiles

### 2. Pin VS Code Version

Document the VS Code version your profiles are tested with:

```json
// profiles/my-profile/metadata.json
{
  "name": "my-profile",
  "testedWith": "1.107.1",
  "minVersion": "1.95.0"
}
```

### 3. Monitor Extension Updates

Subscribe to extension repositories for:
- Release notifications
- Breaking change announcements
- Deprecation notices

### 4. Test Before Deploying

```bash
# Test workflow
1. Check compatibility
2. Import profile in clean VS Code instance
3. Verify all extensions load
4. Test critical functionality
5. Deploy to team
```

## JSON Output Schema

```json
{
  "vscodeVersion": "1.107.1",
  "summary": {
    "total": 27,
    "compatible": 25,
    "incompatible": 2,
    "unknown": 0
  },
  "extensions": [
    {
      "profile": "github-workflows-crisp",
      "extension": "github.copilot",
      "status": "compatible",
      "latestVersion": "1.388.0",
      "engine": "^1.103.0",
      "installedVersion": "1.388.0",
      "lastUpdated": "2025-12-20T10:00:00Z"
    }
  ]
}
```

**Status values:**
- `compatible` - Extension works with current VS Code
- `incompatible` - Extension requires newer VS Code
- `unknown` - Cannot determine compatibility

## Further Reading

- [VS Code Extension API](https://code.visualstudio.com/api)
- [Extension Manifest Reference](https://code.visualstudio.com/api/references/extension-manifest)
- [Marketplace API Documentation](https://github.com/Microsoft/vscode-extension-samples)

## Support

If you find incompatible extensions:

1. Check extension's GitHub repository
2. Look for existing compatibility issues
3. Consider filing an issue with:
   - Your VS Code version
   - Extension version
   - Error messages
   - Steps to reproduce
