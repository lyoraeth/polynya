# Third-Party Code and Attributions

This file contains notices, attributions, and licensing details for third-party code, implementations, or algorithms that have been used, adapted, or referenced in the Polynya project.

All original third-party code remains the intellectual property of its respective authors and is subject to the terms of its original license (detailed below).

**Notice regarding derivative works and licensing:**
Any modifications, architectural layers, platform adaptations, and new abstractions introduced within the Polynya codebase based on this third-party code are licensed exclusively under the terms of the **GNU GPLv3 with Section 7 additional terms** (see the main LICENSE file). Incorporating adapted third-party code into Polynya does not grant permission to redistribute Polynya or its derivative modules under the terms of the original permissive licenses (such as MIT or Apache-2.0).

---

## Components and References

### 1. arboard (Clipboard Module)

* **Original repository:** https://github.com/1Password/arboard
* **Original authors:** 1Password, Artur Kovacs, and arboard contributors
* **Original license:** MIT License (dual-licensed under MIT / Apache-2.0; MIT selected)
* **Status in project:** Planned / Under integration

#### Description
Polynya's clipboard implementation adapts low-level Win32 routines derived from the `arboard` crate. Cross-platform abstractions (X11, Wayland, macOS), public APIs, and external CLI wrappers were stripped out. The underlying Win32 clipboard logic is refactored to integrate directly with Polynya's rendering engine, image pipeline, and asynchronous task model.

#### Original MIT License text:

```text
MIT License

Copyright (c) 2022 The Arboard contributors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

---

## Direct Dependencies and Notices

Polynya is built using external Rust libraries. The full list of direct and transitive dependencies alongside their bundled licenses is generated automatically during packaging and distributed in `NOTICE.txt` next to the binary releases.

Notable upstream licensing terms requiring explicit attribution include:

### AccessKit (Chromium BSD-3-Clause Notice)

Portions of the `accesskit` crate used for Windows accessibility (UI Automation) are derived from Chromium and are subject to the following BSD-style license:

```text
Copyright 2015 The Chromium Authors. All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are
met:

   * Redistributions of source code must retain the above copyright
notice, this list of conditions and the following disclaimer.
   * Redistributions in binary form must reproduce the above
copyright notice, this list of conditions and the following disclaimer
in the documentation and/or other materials provided with the
distribution.
   * Neither the name of Google Inc. nor the names of its
contributors may be used to endorse or promote products derived from
this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
"AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```
