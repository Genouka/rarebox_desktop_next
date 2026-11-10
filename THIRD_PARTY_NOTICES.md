# Third-Party Notices

RareBox Desktop contains or links against the following third-party components.
Each component remains under its own license; the AGPL-3.0 license of this
project does not replace those terms.

| Component | Used for | License | Source / notice |
| --- | --- | --- | --- |
| scrcpy-server | Android remote display server | Apache-2.0 | pushed to the device at runtime; see upstream scrcpy |
| OpenH264 | H.264 decoding | BSD-2-Clause | [`licenses/BSD-2-Clause.txt`](licenses/BSD-2-Clause.txt) |
| GPUI / gpui-pre | UI framework | Apache-2.0 | upstream Zed Industries |
| material3-gpui | Material 3 components | Apache-2.0 | [`vendor/material3-gpui/`](vendor/material3-gpui/) |
| gpui-mobile | iOS / Android GPUI platform layer | GPL-3.0-or-later OR AGPL-3.0-or-later OR Apache-2.0 | [`vendor/gpui-mobile/`](vendor/gpui-mobile/) |
| hapsigner | HarmonyOS HAP/HSP signing | MIT | [`licenses/hapsigner-MIT.txt`](licenses/hapsigner-MIT.txt), [`vendor/hapsigner/`](vendor/hapsigner/) |
| hdc (developtools_hdc) | HarmonyOS device connector | see vendor license | [`vendor/hdc/LICENSE`](vendor/hdc/LICENSE) |
| libuv | hdc event loop | MIT | [`vendor/hdc/third_party/libuv/`](vendor/hdc/third_party/libuv/) |
| LZ4 | hdc compression | BSD-2-Clause | [`vendor/hdc/third_party/lz4/`](vendor/hdc/third_party/lz4/) |
| OpenSSL | TLS / crypto | Apache-2.0 | bundled with the Windows/Android hdc builds |
| libusb | USB access | LGPL-2.1-or-later | bundled with the Windows/macOS/Android hdc builds |
| Android SDK adb | Android debugging | Apache-2.0 / Android SDK terms | bundled for Windows and Android |
| proc-macro-error2 | build-time macro helper | MIT OR Apache-2.0 | [`vendor/proc-macro-error2/`](vendor/proc-macro-error2/) |
| Tabler Icons | line icons used by the quick-guide page | MIT | [`licenses/tabler-MIT.txt`](licenses/tabler-MIT.txt), [`assets/icons/tabler-*.svg`](assets/icons/) |

Additional license texts are in [`licenses/`](licenses/) and next to each
vendored component. If you redistribute a binary, keep the relevant notices
and license texts with it.
