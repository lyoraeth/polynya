# Package manager distribution

## Automatic publication

`.github/workflows/distribute.yml` runs when a stable GitHub Release is published. Tag builds
still create a draft: inspect it and publish it manually. Prereleases are excluded. The workflow
and packaging scripts must be on the default branch before publishing the release.

The workflow downloads all six final assets from that release and generates manifests with their
actual hashes. Scoop and WinGet then run independently:

- **Scoop:** commit `bucket/polynya.json` to this repository's default branch with `GITHUB_TOKEN`.
  Repository/organization policy must permit `contents: write`, and branch rules must allow this
  bot push. No force push or branch-rule bypass is configured. The bucket is added with
  `scoop bucket add polynya https://github.com/lyoraeth/polynya` (also add `extras` for Everything).
- **WinGet:** `wingetcreate submit` validates the generated manifests and opens a PR in
  `microsoft/winget-pkgs`. Microsoft handles catalog validation and acceptance; submission does
  not guarantee immediate availability in WinGet.

### One-time token setup

1. Open GitHub **Settings → Developer settings → Personal access tokens → Tokens (classic)**
   and generate a token with **`public_repo`** only. Fine-grained tokens are not supported by
   winget-create. `repo`, `workflow` and `delete_repo` are not required. Choose an expiration and
   replace the secret when rotating the token.
2. In **lyoraeth/polynya → Settings → Secrets and variables → Actions → New repository secret**,
   set the name to **`WINGET_TOKEN`** and the value to that token.

The WinGet step passes the secret through `WINGET_CREATE_GITHUB_TOKEN`, never a command-line token
argument. The token's account owns the generated fork/PR. A missing secret fails the WinGet job
with setup instructions; the independent Scoop job can still succeed.

### Retry and failure handling

Use **Actions → Distribute packages → Run workflow**, on the default branch, with an already
published stable tag such as `vX.Y.Z`. Drafts, prereleases, invalid tags and missing assets are
rejected before downloading. A failed job can also be rerun through GitHub Actions.

An identical Scoop version is a no-op; a newer bucket version is never downgraded. Different
contents for the same Scoop version fail explicitly. An open or merged WinGet PR with the
workflow's exact title is detected on retries; closed, unmerged PRs may be resubmitted. Inspect
any manually submitted PR before retrying to avoid a duplicate with a different title.

Generated manifests are retained as a workflow artifact, including when publication fails. Do
not replace published release assets: changing bytes invalidates package hashes. The workflow
does not build, test-install, publish a GitHub Release or approve a WinGet PR.

## Manual generation

Generate manifests locally from the six final release artifacts (x64 and ARM64 installers,
portable archives and module archives). The generator does not build, upload or submit anything.
Run it from the repository root:

```powershell
$releaseVersion = '1.2.0'
./tools/package-manifests.ps1 -Version $releaseVersion -Dist target/dist
```

Set `$releaseVersion` to the version being packaged; `1.2.0` above is an example.
This README applies to all releases and does not need a per-release edit.

The output is `target/package-manifests/<version>`. Generation fails if an artifact is missing or the
version's output directory already exists. Every hash comes from the actual local artifact.
Do not replace release assets after generating the manifests; regenerate into a fresh output
directory if any bytes change. Only generate publication manifests from the final tagged build.

## WinGet

The `winget/manifests/l/lyoraeth/Polynya/<version>` folder contains the version, English default locale
and installer manifests. Both architectures use the existing per-user Inno installer and its
stable uninstall registry key. The package manager's silent switches come from `InstallerType:
inno`. Custom switches select the core, agents and picker, and leave autostart off on a fresh
installation. Upgrades preserve existing module choices and autostart.

WinGet declares `voidtools.Everything` as a required package dependency. Silent setup does not
launch Polynya or invoke its bundled Everything and notify installers; dependency installation
is handled by WinGet itself. Everything's own installer may need elevation. The notify package
still requires interactive activation.
Regular interactive setup keeps its existing behavior.

Before submitting:

1. Validate the generated folder with `winget validate <manifest-folder>`.
2. Test fresh silent installation, upgrade and silent uninstall in a disposable Windows Sandbox.
   Check that Polynya does not launch its UI or secondary installers, settings survive upgrade
   and uninstall by default, and the installed entry is detected as `lyoraeth.Polynya`.
   Check Everything's dependency installation and any elevation request separately.
3. Check x64 and ARM64 on their respective systems. An x64 test does not validate ARM64.
4. Publish the final release and ensure every manifest URL downloads the hashed bytes.
5. Submit the generated folder in a PR to `microsoft/winget-pkgs` and wait for validation/review.

## Scoop

The `scoop/bucket/polynya.json` file installs the portable core and bundled modules. The `portable`
marker prevents self-relocation to `%LOCALAPPDATA%`. `data`, `logs` and the module tree are
persisted. Fresh bundled module files replace only their own paths during install, preserving
third-party modules and their data. `extras/everything` is a required dependency, so the Extras
bucket must be available. Its portable package still needs Everything to be started/configured;
declaring a dependency does not install its indexing service or prove file search is ready.
The pre-install hook rejects Windows versions below `10.0.22000` (Windows 11). Scoop runs this
check after downloading/extracting the archives, before linking and persisting Polynya.

The lifecycle scripts in `packaging/scoop` are embedded into the manifest by the generator:

- Before update/uninstall, remember an autostart entry owned by this Scoop installation, then run
  Polynya's cleanup command without `--purge`. This stops the app and removes its integrations.
- After installation/update, restore remembered autostart through Scoop's stable `current` path.
  The app does not start automatically. Launch it to restore enabled agent integrations.
- Persisted user data remains on uninstall unless the user explicitly purges it with Scoop.

Use one Polynya installation at a time. Avoid mixing Scoop, WinGet and a separately launched
portable copy: the core is single-instance and integrations belong to that running instance.
Scoop global installation is rejected because Polynya and its integrations are per-user.

Test install, update, uninstall and reinstall in an isolated Scoop root/user profile before
publishing the manifest. Check autostart on and off, agent integrations enabled and disabled,
custom modules, an existing data directory, and paths containing spaces. Optional notify package
activation is interactive; verify its cleanup separately on a disposable Windows account.
The cleanup command also removes the experimental notify package during Scoop updates; users who
enabled it must activate it again after updating. This flow needs a runtime check before shipping
the Scoop manifest.

The distribution workflow publishes the JSON in this repository's `bucket` directory after the
release assets are public. `checkver` and `autoupdate` also describe manifest updates for Scoop
tools; this project uses the release publication event rather than a scheduled updater.

## Local checks

The generator/file lifecycle sanity check requires PowerShell 7.2+. It creates synthetic artifacts
only under `target` and never executes an installer or writes the registry:

```powershell
./tools/test-package-manifests.ps1 -Winget -ScoopSchema "$HOME/scoop/apps/scoop/current/schema.json"
./tools/test-release-manifests.ps1
```

The schema and WinGet arguments are optional. Fixture manifests use `9.9.9` and are not for
publication or installation. Runtime installation/upgrade checks still require a disposable VM
or Windows Sandbox; these checks do not replace them.

The release guard check mocks GitHub CLI responses and tests published/draft/prerelease states,
tag validation, missing/duplicate assets, download failures and hashes of downloaded fixture
bytes without making network requests. Both checks run in CI.

To additionally resolve the dependencies through the installed package managers without
installing them:

```powershell
./tools/test-package-manifests.ps1 -Winget -ScoopSchema "$HOME/scoop/apps/scoop/current/schema.json" `
    -ScoopScript "$HOME/scoop/apps/scoop/current/bin/scoop.ps1" -ResolveWingetDependency
```

Scoop resolution covers both architecture declarations. WinGet queries its catalog; this does
not execute the dependency installer or test its elevation behavior. Fresh installation with
Everything absent, an existing Everything installation and actual indexed file search remain
runtime acceptance checks for an isolated Windows VM.

## References

- [WinGet manifests](https://learn.microsoft.com/en-us/windows/package-manager/package/manifest)
- [WinGet validation and submission](https://learn.microsoft.com/en-us/windows/package-manager/package/repository)
- [winget-create token setup](https://github.com/microsoft/winget-create/blob/main/doc/token.md)
- [Scoop app manifests](https://github.com/ScoopInstaller/Scoop/wiki/App-Manifests)
- [Scoop persistence](https://github.com/ScoopInstaller/Scoop/wiki/Persistent-data)
- [Inno command-line switches](https://jrsoftware.org/ishelp/topic_setupcmdline.htm)
