## ADDED Requirements

### Requirement: Workspace Trust default
Shared editor base settings SHALL default `security.workspace.trust.untrustedFiles` to `"prompt"` so that opening an untrusted folder requires user confirmation before language servers, tasks, debug launches, and formatters execute.

#### Scenario: First open of an untrusted folder
- **WHEN** a user opens a folder that VS Code does not consider trusted under any managed profile composed from the shared bases
- **THEN** VS Code presents the Workspace Trust prompt before running workspace-defined automation

#### Scenario: Trusted folder
- **WHEN** the user has previously trusted the folder
- **THEN** the prompt is not shown and the workspace operates normally

### Requirement: Variable-font default
Shared editor base settings SHALL default `editor.fontVariations` to `true` so VS Code automatically translates `editor.fontWeight` into the corresponding variable-font axis when the resolved font is variable. VS Code's settings schema accepts both `boolean` and `string` (CSS `font-variation-settings` syntax) for this key; the boolean form `true` is the documented "automatic translation" mode and is the form chosen here.

#### Scenario: Variable font in font stack
- **WHEN** a user's resolved editor font is a variable font (for example Monaspace Neon) and no per-profile override changes the value
- **THEN** the editor renders the font using its variable axes rather than a default static cut

#### Scenario: Per-profile opt-out
- **WHEN** a contributor wants a managed profile to use a fixed-axis font cut
- **THEN** they add `editor.fontVariations` set to a CSS axis string (or `false`) in that profile's `_overrides/<name>.jsonc`, which wins over the shared base during the merge
