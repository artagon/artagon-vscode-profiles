# VS Code Extension Compatibility Guide

This guide explains how to detect and handle incompatible VS Code extensions in your profiles.

> **CLI integration**: `vspcli apply --target=workspace` (and legacy `vspcli --detect --target=workspace`) runs this checker automatically before writing `.vscode/extensions.json`. Default mode is `--check-compat=warn` (stderr notice; install proceeds), while default `--dry-run` skips the implicit Marketplace query unless `--check-compat=warn|block` is passed explicitly. Set `--check-compat=block` to abort on any incompatibility, or `--check-compat=off` to skip the check entirely. The standalone invocation below is preserved for ad-hoc audits.

## Quick Start

### Check All Profiles

```bash
bash scripts/check-extension-compatibility.sh --all
```

### Check Specific Profile

```bash
bash scripts/check-extension-compatibility.sh github-workflows
```

### Check Multiple Profiles

```bash
bash scripts/check-extension-compatibility.sh java-gradle astro
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

```text
Usage: check-extension-compatibility.sh [options] [profile...]
Options:
  --all                      Audit all profiles under profiles/
  --extensions-file <path>   Audit extension IDs from a file (newline-separated or JSON array)
  --marketplace-only         Query Marketplace engines without requiring installed local extensions
  --vscode-version <ver>     Override detected local VS Code version (e.g. 1.147.0)
  --json                     Emit structured JSON report on stdout
  -h, --help                 Show help
```

### Examples

#### 1. Check a Single Profile

```bash
bash scripts/check-extension-compatibility.sh github-workflows
```

**Output:**
```text
VS Code version: 1.147.0 | total=29 compatible=29 incompatible=0 not_found=0
```

#### 2. Check All Profiles (Marketplace Only)

```bash
bash scripts/check-extension-compatibility.sh --all --marketplace-only
```

Queries Marketplace engine metadata without requiring extensions to be installed locally.

#### 3. JSON Output for Automation

```bash
bash scripts/check-extension-compatibility.sh --json github-workflows | jq '.summary'
```

**Output:**
```json
{
  "total": 29,
  "compatible": 29,
  "incompatible": 0,
  "not_found": 0
}
```

#### 4. Find Incompatible Extensions

```bash
bash scripts/check-extension-compatibility.sh --json --all | \
  jq '.extensions[] | select(.status == "incompatible")'
```

#### 5. Audit an Extensions File with an Explicit VS Code Version

```bash
bash scripts/check-extension-compatibility.sh \
  --extensions-file profiles/rust/extensions.json \
  --vscode-version 1.147.0 \
  --marketplace-only --json
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

## Live Marketplace Queries

The script queries the VS Code Marketplace API directly in batches of up to 40 extensions with a 30-second timeout per batch (`--max-time 30`). In headless or CI environments without a local `code` CLI, pass `--vscode-version <ver>` to specify the target VS Code version explicitly:

```bash
bash scripts/check-extension-compatibility.sh --all --marketplace-only --vscode-version 1.147.0 --json
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
# Audit a single profile to narrow down timeouts
bash scripts/check-extension-compatibility.sh --marketplace-only rust
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
  "vscode_version": "1.147.0",
  "summary": {
    "total": 29,
    "compatible": 27,
    "incompatible": 2,
    "not_found": 0
  },
  "extensions": [
    {
      "id": "github.copilot-chat",
      "status": "compatible",
      "version": "0.26.0",
      "engine": "^1.99.0",
      "prerelease": false
    }
  ]
}
```

**Status values:**
- `compatible` - Extension works with current VS Code
- `incompatible` - Extension requires a different VS Code version
- `NOT_FOUND` - Extension was not returned by the Marketplace API

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
