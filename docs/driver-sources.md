# Pinned driver sources

## Intel

- Package: 15.45.34.5174; Windows driver 21.20.16.5174.
- URL: https://downloadmirror.intel.com/30195/a08/win64_15.45.5174.exe
- SHA-256: `aab51027dac8210bca98b6679472b2a00ab15a5d4f8a6b777259200de52e48e2`
- Expected length: 255,770,016 bytes.
- Pin supplied by the source reproduction guide.

7-Zip initially opens only the WiX Burn UX cabinet. The patcher locates the
attached cabinet by the size and SHA-1 digest in the embedded Burn manifest,
then restores every payload's original path and checks its manifest digest.
The complete EXE's pinned SHA-256 is checked before this extraction.

The original `igdlh64.inf` and `igdlh.cat` are kept unchanged. DISM adds this
signed package to Setup and every selected installed edition. No `/ForceUnsigned`.

## Apple

- Boot Camp 041-88734, product version 6.1.6660.
- URL: https://swcdn.apple.com/content/downloads/28/47/041-88734-A_Q0SKN07BL0/7knxzec1xmojr5x5t7xnu5a4u6lslmlf9v/BootCampESD.pkg
- SHA-256: `ace1cc6dd3bbd12e32fcf953b0f1d0ab74f4a77075fbd4733352468c5aa36f34`
- Length: 353,187,226 bytes.
- Pin calculated from the full HTTPS download from Apple's CDN on 2026-10-19.
  This is a developer-established integrity pin, not a separately published Apple hash.

Extraction: xar -> bzip2 Payload -> cpio -> WindowsSupport.dmg -> support tree.
Recognized Boot Camp driver EXEs are extracted for their INF packages. The full
tree goes inside installed Windows. Only the exact source allowlist is placed
on the Setup boot path. Thunderbolt, CS4206, and old Intel graphics are excluded
from that path; the missing Thunderbolt MSI is not needed for HDMI.

## Build tools

7-Zip and Microsoft ADK are user-installed prerequisites, not downloaded from
third-party mirrors. The UI detects their standard installation paths and offers
the official installation links. Helper EXEs are compiled from source in this repo.

Downloaded archives are cached and hashed on every reuse. Partial downloads use
unique filenames and are deleted after failure. The app never silently accepts a
different package under an existing version label.
