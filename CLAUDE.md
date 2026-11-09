# Modern Monitor Switcher

Saves monitor arrangements as named profiles and switches between them from the tray, a hotkey,
or `msw.exe`. Windows 10 and 11, Rust with a Tauri settings window, installed per-user and
self-updating. `README.md` covers use, install and how profile matching works.

Follows the workflow standard in the user CLAUDE.md: squash merges, `/ship` to land a PR,
`/release` to cut one, bugs fixed when found. Deviations are listed at the end.

## Commands

```bash
cargo test --workspace            # unit tests, no hardware needed
cargo run                         # tray app against REAL settings; hands over to an installed copy
cargo build --profile fast        # release-like build without LTO, in target/fast
cargo fmt --all -- --check        # CI checks this
cargo clippy -p msw-core -p msw-cli --all-targets -- -D warnings
powershell -ExecutionPolicy Bypass -File tools/run-sandbox.ps1    # debug build, throwaway settings (-Keep carries them over)
powershell -ExecutionPolicy Bypass -File tools/install-local.ps1  # build the installer and install it
powershell -ExecutionPolicy Bypass -File tools/build-release.ps1  # build the installer only
```

Work against the debug build (`target/debug/ModernMonitorSwitcher.exe`, about 5 s a rebuild).
Build the installer only to test installing or updating. Run cargo from PowerShell.

| Crate | Holds |
| --- | --- |
| `msw-core` | The CCD API, profile storage, the matching strategies. No UI, and no dependency on either front end |
| `msw-cli` | `msw.exe` |
| `msw-app` | The tray app and settings window (Tauri); `msw-app/ui` is the page |

## Rules that hold across changes

| Rule | Why |
| --- | --- |
| The updater signing key never enters the repo. `*.key` stays gitignored | Installed copies trust only the key they shipped with. Lose it and every user reinstalls by hand. The private half is the `TAURI_SIGNING_PRIVATE_KEY` Actions secret; the public half is in `tauri.conf.json` |
| `tools/build-release.ps1` passes `--ci` to `tauri build` | Without it the build stops at a password prompt even for a key with none, and PowerShell drops an env var set to `""` |
| Every candidate topology goes through `SDC_VALIDATE` before `SetDisplayConfig` | A strategy Windows would reject never reaches the screen |
| Match a monitor by identity only when unambiguous | Two monitors of one model share a friendly name; matching both to the first collapses two displays into one |
| Adapter LUIDs are never stored as identity | Windows reassigns them on boot and driver updates |
| The install stays per-user, in `%LOCALAPPDATA%` | Updates install without a UAC prompt |
| `msw-core` never depends on `msw-app` or `msw-cli` | A future network front end stays an addition |
| New UI keeps the existing system: native Windows 11 chrome (Mica, nav rail), phosphor teal (`#0B7A70` light, `#5ED3C4` dark) for lit screens and actions, standby amber only for monitors connected but off. The desk diagram is the one bold element | No extra accent colours |

## Verify a change

`cargo test --workspace`, fmt and clippy, then `tools/run-sandbox.ps1` for anything in the app.
Switching a profile changes the real displays even in the sandbox.

Do not drive Jacob's desktop: no synthetic clicks, no screenshot loops. For a GUI check, give him a
short numbered list of what to click, then verify from state you can read (profile JSON under
`%APPDATA%\ModernMonitorSwitcher`, the registry, `msw.exe` output, logs). To see a UI change,
render `msw-app/ui` in headless Edge with a mocked `window.__TAURI__` and look at the PNG.
`docs/manual-test-plan.md` lists the hardware checks a release needs.

## CI

`ci.yml` runs `Test and lint` (fmt, clippy, tests for `msw-core` and `msw-cli`) on every PR; it is
the required check. `pages.yml` deploys `site/` on a push to `main` that touches it.

## Cutting a release

Run `/release`. Version files: `Cargo.toml` (`[workspace.package] version`, what `msw --version`
prints) and `msw-app/tauri.conf.json`; `release.yml` fails a tag that disagrees with either. The tag
push builds the installer, signs the update package, publishes the release with the updater
manifest, and attaches the standalone `msw.exe`. `workflow_dispatch` with a tag rebuilds an existing
release. Afterwards, check the release has the installer, its `.sig`, `latest.json` and `msw.exe`,
and that an installed copy offers the update.

## Deviations from the standard

| Deviation | Why |
| --- | --- |
| No wiki yet; `README.md` and `docs/manual-test-plan.md` hold the documentation | Small enough that a README covers it. Start a wiki per the vault's Wiki Conventions when that stops being true |
| `site/` holds the GitHub Pages project page | A published page, not documentation |
