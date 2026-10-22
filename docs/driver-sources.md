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
signed package to every selected installed edition for the 2016 13-inch models.
Only the verified A1706 also receives it in Setup with the HDMI startup payload.
No `/ForceUnsigned`.

### 2017 Iris Plus 650

The 2017 MacBookPro14,2 uses Intel DCH **27.20.100.9316**, dated February 18, 2021
in its original `iigd_dch.inf`, close to the February 5, 2021 release of 15.45.5174.
The old Intel CDN links no longer responded, so the pinned CAB is from Microsoft's
Update Catalog, update `03a02012-f79c-44bf-8dc7-fa769a1e7ecf`:

- Catalog: https://www.catalog.update.microsoft.com/ScopedViewInline.aspx?updateid=03a02012-f79c-44bf-8dc7-fa769a1e7ecf
- URL: https://catalog.s.download.windowsupdate.com/c/msdownload/update/driver/drvs/2021/05/8c3b1650-0aef-4e61-b7f3-eaff91e3482a_504a1ab0935cace6f2359d8f857182fb6e23fb1f.cab
- Size: 312,227,370 bytes.
- SHA-256: `bac61afa36281c4bc68c78cdacf4192b7f04fba7d3a7c5a1fa08828ccb49f974`.
- This developer pin was computed from the complete Microsoft HTTPS download.
  The extracted `igdlh.cat` has a valid Windows Authenticode signature.
- The unchanged INF has a generic `PCI\VEN_8086&DEV_5927` mapping, so it is not
  limited to the OEM hardware IDs displayed in the catalog listing.

The 2017 A1706 Setup recipe also imports this package into `boot.wim` index 2.
Its main FileRepository package is checked byte-for-byte against the pinned
extraction. The igfx demand-start service uses `%13%\igdkmd64.sys` from the
original INF, resolved to the actual installed DriverStore directory. It does
not use the 2016 service path. BasicDisplay and source BCD settings are preserved.
Apple Boot Camp 041-89042 provides allowlisted startup drivers; an AppleSSD64
INF is required, while unavailable optional input/audio/Touch Bar drivers are
logged and skipped. This recipe remains unverified on hardware.

Required DriverStore files follow the 5927 install section's CopyFiles
directives. SourceDisksFiles also lists unused OpROM updater inputs such as
`igsc.dll`; their absence from the installed store is valid for this profile.
Those source files are still required when validating the downloaded extraction.

Before servicing, validate the exact driver version, catalog signature, and the
selected GPU's generic INF mapping: 1927 (Iris 550), 1926 (Iris 540),
or 5927 (Iris Plus 650). Keep original INF and CAT files unchanged. These matches
establish package compatibility, not Mac hardware verification.

Hardware references: https://support.apple.com/en-us/111999,
https://support.apple.com/en-us/111975, https://support.apple.com/en-us/111972.
5174 release notes: https://downloadmirror.intel.com/30195/eng/releasenotes_gfx_15.45.34.5174.pdf

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
download completes. Graphics follow the external output automatically, without a
GPU toggle. The 13-inch models use the matching Intel replacement; the HDMI startup
sequence remains specific to MacBookPro13,2. MacBookPro13,3 uses its Apple Radeon
package without downloading or injecting a separate Intel package. Loose Radeon
display/audio INF packages under `BootCamp/Drivers/AMD/AMDGraphics/Packages/Drivers`
are copied into staged `Drivers/AMDGraphics` and imported into each selected Windows
image through signed DISM driver servicing. The Boot Camp installer is preserved.
Their WinPE paths contain only available allowlisted
Apple driver folders, while the installed Windows image receives the full bundle.

| Mac identifier | Boot Camp product | Additional package |
| --- | --- | --- |
| MacBookPro13,2 | 041-88734 | Intel 15.45.34.5174 |
| MacBookPro13,1 | 041-88734 | Intel 15.45.34.5174 |
| MacBookPro13,3 | 041-88737 | Apple Radeon Pro drivers for the external output |
| MacBookPro14,2 | 041-89042 | Intel 27.20.100.9316 |

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
