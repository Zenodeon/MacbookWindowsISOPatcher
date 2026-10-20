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
This version is pinned for the v17 HDMI boot recipe; the app does not choose
Intel's newest release automatically.

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
Recognized Boot Camp driver EXEs are extracted for their INF packages. Apple
device drivers go inside installed Windows, excluding `IntelHDGraphics64` and
`IntelIrisSetup`; the separately injected Intel driver supplies graphics instead.
The Boot Camp software installer is retained unmodified. Only the exact source allowlist is placed
on the Setup boot path. Thunderbolt, CS4206, and old Intel graphics are excluded
from that path; the missing Thunderbolt MSI is not needed for HDMI.

## Downloads for selected models

Pressing Start download & continue to Windows ISO downloads the selected model's
archives without extracting or installing them. Selecting a model alone does not
start any downloads.
The mappings were checked against Apple's Software Update catalog and English
Boot Camp distributions on 2026-10-20. Every listed model can build after its
download completes. Unverified models use their own Apple bundle, including its
graphics drivers; the separate Intel replacement and HDMI startup sequence remain
specific to MacBookPro13,2. Their WinPE paths contain only available allowlisted
Apple driver folders, while the installed Windows image receives the full bundle.

| Mac identifier | Boot Camp product | Additional package |
| --- | --- | --- |
| MacBookPro13,2 | 041-88734 | Intel 15.45.34.5174 |
| MacBookPro13,1 | 041-88734 | Included Apple bundle only |
| MacBookPro13,3 | 041-88737 | Included Apple bundle only |
| MacBookPro14,2 | 041-89042 | Included Apple bundle only |

- 041-88737: 572,663,165 bytes; SHA-256
  `782b00a1dbaf5950605909620f6f70d0d12375da8ce914d73d33ab57a9d1072c`.
  URL: https://swcdn.apple.com/content/downloads/25/38/041-88737-A_IXJZSM8OLV/fq3xy3duoq3xqp9o2sa8dkjtz5wvjaojhq/BootCampESD.pkg
- 041-89042: 409,720,220 bytes; SHA-256
  `6e1213a2a8abbc7815f342010f4e9c14aaaab27c2086d1d3fc1a5f1b41859bc9`.
  URL: https://swcdn.apple.com/content/downloads/32/09/041-89042-A_XVZ2U8XKG2/06ub1qfep6wv3g8bb68smwt3ac25xyng83/BootCampESD.pkg

These two pins were computed from full HTTPS CDN downloads on 2026-10-20, with
the archives' internal XAR payload checksums also verified. They are developer
pins, not separately published Apple SHA-256 values. All archives, including
shared packages, must match their SHA-256 pin on every reuse.

Catalog: https://swscan.apple.com/content/catalogs/others/index-11-10.15-10.14-10.13-10.12-10.11-10.10-10.9-mountainlion-lion-snowleopard-leopard.merged-1.sucatalog

## Build tools

7-Zip has an official installation link. The ADK button downloads Microsoft's
signed installer and installs Deployment Tools after UAC approval. Neither tool
comes from third-party mirrors. Helper EXEs are compiled from source in this repo.

Downloaded archives are cached and hashed on every reuse. Partial downloads use
unique filenames and are deleted after failure. The app never silently accepts a
different package under an existing version label.
