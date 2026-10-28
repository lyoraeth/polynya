// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Exe resources: the icon (the mark itself comes from `tools/logo.py`) and the version, see
//! `tools/winres.rs`.

include!("tools/winres.rs");

fn main() {
    resources(std::path::Path::new(env!("CARGO_MANIFEST_DIR")), "Polynya", "polynya.exe");
}
