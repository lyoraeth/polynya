# Building and contributing

[Русский](docs/ru/contributing.md) · [简体中文](docs/zh/contributing.md) · [日本語](docs/ja/contributing.md)

Thanks for your interest in Polynya! This file describes how to set up the environment, build the core and modules, run the tests and propose your changes.

---

## 1. Tools and dependencies

The project is written in plain Rust without external UI frameworks (only the `windows` 0.62, `serde` / `serde_json` and `toml` crates, plus `embed-resource` at build time).

To build you need:
* **Windows 11** (x64 or ARM64);
* **Rust**: the version is pinned in `rust-toolchain.toml` and installed by `rustup` automatically. Either the GNU toolchain with **MinGW-w64** or MSVC works (the full Windows SDK is not required for regular builds); releases are built with MSVC.
* **Inno Setup 6.5+**: only to build the installer (`winget install JRSoftware.InnoSetup`).

---

## 2. Building

The whole project is a single Cargo workspace (the `polynya` core, the `proto` protocol crate and 4 built-in modules in `modules/{agents,notify,picker,start}`).

Build everything in the release profile:
```powershell
cargo build --release --workspace
```

After the build, `target/release/` contains:
* `polynya.exe`: the island core and the CLI client;
* `polynya-agents.exe`, `polynya-notify.exe`, `polynya-picker.exe`: the built-in module binaries;
* `polynya-start.exe`: the custom Start module. It lives in the repository (`modules/start`), like the Node.js `modules/devtools`, but neither is shipped in releases yet.

*Versioning note:* `build.rs` computes the version from `git` automatically (on a `vX.Y.Z` tag it is `X.Y.Z`, between tags `X.Y.Z+commits.hash`, before the first tag `1.0.0-dev.N+hash`) and embeds it into the executable's `VERSIONINFO`.

---

## 3. Local deployment for debugging

At runtime `polynya.exe` works from its own directory in `%LOCALAPPDATA%\Polynya\`. To test a fresh build locally:

1. Stop the running island with `polynya quit` (resident modules exit with it).
2. Copy the built `polynya.exe` into `%LOCALAPPDATA%\Polynya\`, and the module binaries (with their `module.toml`) into `%LOCALAPPDATA%\Polynya\modules\<id>\`.
3. **Important:** start `polynya.exe` from `explorer.exe` or a regular (non-elevated!) terminal. If your IDE or terminal runs as administrator, the island's child process inherits the elevated token, which breaks communication with Everything because of UIPI.

### Logging while debugging
The shared log of the core and all modules is written to `%LOCALAPPDATA%\Polynya\logs\polynya.log`. To enable verbose output (catalog walk timings, banner decisions, taskbar capture parameters), add this line to `%APPDATA%\Polynya\config.ini`:
```ini
log = debug
```

---

## 4. Testing

The project has 85 automated tests covering the notification queue and series logic, manifest parsing, `proto` serialization, APCA/OKLCH color math, AI agent config merging, the calculator parser and the CLI.

Run all unit tests:
```powershell
cargo test --release --workspace
```

To check integration with a real Windows environment (GSMTC media session thumbnails, system geolocation, etc.), there is a set of ignored tests:
```powershell
cargo test --release --workspace -- --ignored
```

Before sending a pull request, make sure all tests in `--workspace` pass.

### Code style
* **Formatting:** `cargo fmt --all` (settings in `rustfmt.toml`). `rustfmt` doesn't rewrap comments, so keep them within ~100 columns by hand.
* **Comments:** in English; they explain *why*, not retell the code. A module starts with `//!`, public items are documented with `///`.
* **`unsafe`:** every block has a `// SAFETY:` comment with a real justification (who owns the handle, how long the buffer lives). The `clippy::undocumented_unsafe_blocks` lint is enabled for the whole workspace: `cargo clippy --workspace` must not report new warnings.
* **File header:** every source file starts with two lines, `Copyright` and `SPDX-License-Identifier` (`GPL-3.0-or-later`, or `MIT` for the `proto` crate).

---

## 5. Helper scripts (`tools/`)

In regular development you don't need to rebuild assets (the ready files are already in the repository and embedded at compile time), but if you change the logo or the notification package manifest:
* **`tools/package.ps1`** rebuilds the unsigned sparse `.msix` package for the `notify` module (it downloads the lightweight `makeappx` from the `Microsoft.Windows.SDK.BuildTools` NuGet package by itself). **Important:** on any change to the package manifest, bump the version both in the manifest and in the `VERSION` constant in `src/package.rs`, so the core notices the update at startup.
* **`tools/release.ps1`** builds the release files for one architecture into `target/dist`: the installer (`installer/polynya.iss` through `installer/build.ps1`, which downloads the Everything setup and checks its SHA-256), the portable archive and the modules archive for it. The `Release` workflow runs it for both architectures on a `vX.Y.Z` tag and opens a draft release.
* **`tools/logo.py` and `tools/ascii.py`** generate the `.ico`, raster and vector logo variants with grain, and the colored ASCII layout `assets/logo/mark-ascii.json` from the `assets/logo/mark.svg` source.

---

## 6. Localization and text rules (`i18n`)

The interface supports 4 languages: Russian (`ru`), English (`en`), Chinese (`zh`) and Japanese (`ja`). Core strings live in `src/i18n.rs`, module strings in their code and `module.toml` files. When adding strings, follow the project's common rules:

1. **Case:** standalone titles and setting names start with a capital letter; compound "object + state" statuses are lowercase after the object (e.g. `"Caps Lock on"`, `"#ff0000 copied"`).
2. **Ellipsis:** ongoing processes and input placeholders use the single `…` character (`"Thinking…"`, `"Enter a city…"`); agent action strings with an object have no ellipsis (`"Editing main.rs"`).
3. **Tone and terminology:** strictly neutral, without "you" and without the first person. System commands use Windows' own exact wording in each language (*"Завершение работы"* / *"Shut down"* / *"关机"* / *"シャットダウン"*). Confirmation strings (`confirm`) are short questions only.

The documentation follows the same four languages: English is the primary version (`README.md`, `docs/*.md`), translations live in `README.<lang>.md` and `docs/<lang>/`. Legal texts (`LICENSE`, `TRADEMARKS.md`) exist in English only.

---

## 7. Contributor license terms

By sending a pull request to this repository, you agree that your code:
* as part of the core and built-in modules, is distributed under **[LICENSE](LICENSE)** (GNU GPLv3 + additional terms under Section 7);
* as part of the `proto` crate and the protocol documentation, is distributed under **[proto/LICENSE](proto/LICENSE)** (MIT License).
