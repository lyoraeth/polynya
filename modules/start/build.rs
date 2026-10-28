// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Exe resources: the icon and a description, so Task Manager shows the process as part of Polynya.

include!("../../tools/winres.rs");

fn main() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    resources(&root, "Polynya Start menu", "polynya-start.exe");
}
