# Third-party notices

MahiMarkD++ is licensed under the GNU General Public License v3.0 (see `LICENSE` and `COPYRIGHT`).
It includes or links the components below. Their licenses are compatible with GPL-3.0.

## Bundled web assets

| Component | Version | License | Copyright |
|---|---|---|---|
| [highlight.js](https://github.com/highlightjs/highlight.js) | 11.12.0 | BSD-3-Clause | (c) 2006 Ivan Sagalaev, Josh Goebel and other contributors |
| [KaTeX](https://github.com/KaTeX/KaTeX) with its fonts | 0.19.1 | MIT (fonts: SIL OFL 1.1) | (c) 2013-2020 Khan Academy and other contributors |

## Native components

| Component | License |
|---|---|
| Microsoft Edge WebView2 Loader (static library, via `webview2-com-sys`) | BSD-3-Clause, (c) Microsoft Corporation |
| [webview2-com](https://github.com/wravery/webview2-rs), webview2-com-sys | MIT |
| [windows-rs](https://github.com/microsoft/windows-rs) (windows, windows-core and helper crates) | MIT OR Apache-2.0 |
| [pulldown-cmark](https://github.com/pulldown-cmark/pulldown-cmark) | MIT |
| [ammonia](https://github.com/rust-ammonia/ammonia), html5ever, markup5ever, tendril, string_cache, web_atoms | MIT OR Apache-2.0 |
| cssparser, dtoa-short | MPL-2.0 (source: crates.io) |
| url, idna, form_urlencoded, percent-encoding | MIT OR Apache-2.0 |
| ICU4X crates (icu_*, zerovec, yoke, zerofrom, zerotrie, tinystr, litemap, writeable, potential_utf) | Unicode-3.0 |
| serde, serde_json, itoa, zmij | MIT OR Apache-2.0 / MIT |
| flate2, miniz_oxide, adler2, crc32fast, simd-adler32 | MIT OR Apache-2.0 (miniz_oxide also Zlib) |
| imagesize | MIT |
| phf, phf_shared, siphasher, precomputed-hash, new_debug_unreachable, maplit | MIT (some also Apache-2.0) |
| parking_lot, lock_api, scopeguard, smallvec, bitflags, cfg-if, log, memchr, unicase, dtoa, stable_deref_trait, utf8_iter, utf_types | MIT OR Apache-2.0 (memchr: Unlicense OR MIT) |

The full list with exact versions is in `Cargo.lock`.

## BSD-3-Clause (highlight.js)

```
Copyright (c) 2006, Ivan Sagalaev.
All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

* Redistributions of source code must retain the above copyright notice, this
  list of conditions and the following disclaimer.

* Redistributions in binary form must reproduce the above copyright notice,
  this list of conditions and the following disclaimer in the documentation
  and/or other materials provided with the distribution.

* Neither the name of the copyright holder nor the names of its
  contributors may be used to endorse or promote products derived from
  this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

## MIT (KaTeX)

```
The MIT License (MIT)

Copyright (c) 2013-2020 Khan Academy and other contributors

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

The MIT and Apache-2.0 license texts of the Rust crates are available in each crate's source on crates.io.
