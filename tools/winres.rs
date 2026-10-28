// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

// Exe resources shared by the core and the module exes: the icon and the version. Pulled into
// each `build.rs` with `include!`, since a build script cannot depend on a workspace crate.
//
// The version comes from git: on tag `vX.Y.Z` it is `X.Y.Z`, after it `X.Y.Z+commits.hash`.
// Before the first tag it is a pre-release of the `Cargo.toml` version: `X.Y.Z-dev.commits+hash`.
// Without git (source archive) it is just the `Cargo.toml` version. Available in code as
// `env!("POLYNYA_VERSION")`.

use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Version string and four numbers for the resource: major, minor, patch, commits since the tag.
fn version() -> (String, [u32; 4]) {
    let pkg = env!("CARGO_PKG_VERSION");
    let nums = |base: &str, commits: u32| {
        let mut v = base.split('.').map(|p| p.parse().unwrap_or(0));
        [v.next().unwrap_or(0), v.next().unwrap_or(0), v.next().unwrap_or(0), commits.min(u16::MAX as u32)]
    };
    // v0.2.0-5-g1a2b3c4: tag, commits since it, hash.
    if let Some(d) = git(&["describe", "--tags", "--long", "--match", "v[0-9]*"]) {
        let mut parts = d.rsplitn(3, '-');
        let (hash, commits, tag) = (parts.next().unwrap_or(""), parts.next().unwrap_or("0"), parts.next().unwrap_or(pkg));
        let base = tag.trim_start_matches('v');
        let commits: u32 = commits.parse().unwrap_or(0);
        let text = if commits == 0 { base.to_string() } else { format!("{base}+{commits}.{}", hash.trim_start_matches('g')) };
        return (text, nums(base, commits));
    }
    match (git(&["rev-list", "--count", "HEAD"]), git(&["rev-parse", "--short", "HEAD"])) {
        (Some(count), Some(hash)) => {
            let commits = count.parse().unwrap_or(0);
            (format!("{pkg}-dev.{commits}+{hash}"), nums(pkg, commits))
        }
        _ => (pkg.to_string(), nums(pkg, 0)),
    }
}

/// Embeds the icon and the version resource. `root` is the workspace folder; `description` is
/// what Task Manager shows for the process.
fn resources(root: &std::path::Path, description: &str, exe: &str) {
    // A new commit or tag means a new version.
    for p in [".git/HEAD", ".git/refs", ".git/packed-refs", "assets/logo/polynya.ico"] {
        println!("cargo:rerun-if-changed={}", root.join(p).display());
    }
    let (text, [a, b, c, d]) = version();
    println!("cargo:rustc-env=POLYNYA_VERSION={text}");
    let internal = exe.trim_end_matches(".exe");
    let icon = root.join("assets/logo/polynya.ico");
    let rc = format!(
        r#"1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {a},{b},{c},{d}
PRODUCTVERSION {a},{b},{c},{d}
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "Polynya"
      VALUE "FileDescription", "{description}"
      VALUE "FileVersion", "{text}"
      VALUE "InternalName", "{internal}"
      VALUE "OriginalFilename", "{exe}"
      VALUE "ProductName", "Polynya"
      VALUE "ProductVersion", "{text}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        icon = icon.display().to_string().replace('\\', "/"),
    );
    let path = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("polynya.rc");
    std::fs::write(&path, rc).unwrap();
    embed_resource::compile(&path, embed_resource::NONE).manifest_optional().unwrap();
}
