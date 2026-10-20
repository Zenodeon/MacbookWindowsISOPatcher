# MacBook ISO Patcher

A native Rust desktop utility for preparing Windows installation media for Intel
MacBooks. No JavaScript, TypeScript, browser runtime, or payment verification.

The first profile targets **MacBook Pro 13-inch 2016 A1706 / MacBookPro13,2**, with
Intel Iris Graphics 550 and T1. It implements the source project's v17 external
HDMI startup sequence for a Mac whose internal display is disconnected.

## Use

1. Run `macbook-iso-patcher.exe` on x64 Windows 10 or 11.
2. Drop an original Microsoft Windows 10 ISO onto the app, or click the drop zone
   to browse. Dropped and browsed files are scanned automatically; pasted paths
   can be scanned with **Scan ISO**.
3. Select A1706 and click the rounded edition rows to select or deselect them.
   Pro has a green **Hardware Verified** label based on the original project's
   A1706 result; other editions have an orange **Unverified** label. This app's
   generated ISOs still need hardware validation.
4. Choose a new output filename and a working folder with enough free disk space.
5. Install missing prerequisites using **Missing downloads**, highlighted above
   **Build setup**. **Download & install ADK tools** downloads Microsoft's installer,
   verifies its signature, requests UAC, and installs Deployment Tools only. Its
   progress appears in the panel; the ISO tool is detected automatically afterward.
   A manual Microsoft download link is also available. Once both tools are found,
   the panel stays visible as a green **Build tools ready** section. **Build setup**
   appears above **Output & workspace**.
6. Start the build. Multiple editions show an optional donation page linking to
   [Ko-fi](https://ko-fi.com/zenodeon); **Continue without donating** always works.
7. Approve UAC when the separate image-servicing worker starts.

The GUI downloads and verifies Intel and Apple drivers before elevation. Build
logs and `job.json` stay in the chosen working folder. Existing output files and
the original ISO are never overwritten. Cancellation waits for an active DISM
operation to reach a safe boundary. Retrying with the same working folder
recovers identifiable app-owned mounts from interrupted builds.

Use **Dismount ISO** to eject the selected source ISO from Windows without
changing its file. Confirm in the popup to dismount and return to the first page.
It is unavailable during scans, builds, ADK installation, and another dismount.

The initial content size is 1240 × 700 logical pixels. On smaller monitors it
fits proportionally with room for window borders and the taskbar. The window
remains resizable; compact spacing, wrapping and scrolling keep controls usable
at smaller sizes. The aspect ratio is a starting proportion, not a resize lock.

The sidebar includes a persistent Ko-fi button, the GitHub repository link, and
**by zenodeon** linking to the creator's website. Choose **System**, **Light**, or
**Dark** under Appearance; the choice is saved between runs. Atomed, Exo 2 and
Space Mono from zenodeon.com are embedded for offline use.

## Prerequisites

- Windows x64. The UI/core are portable, but other OS servicing backends are not implemented.
- [7-Zip](https://www.7-zip.org/download.html), installed or selected as `7z.exe`.
- [Windows ADK Deployment Tools](https://learn.microsoft.com/en-us/windows-hardware/get-started/adk-install)
  for `oscdimg.exe`. The WinPE add-on is not needed. The app checks its Microsoft signature.
- Internet for the first driver download; pinned archives are cached and verified on reuse.
- A writable local working folder. The app calculates a conservative space allowance
  from the selected images; servicing several editions can need substantial disk space.

Supported inputs: x64 English (en-US) Windows 10 client images, builds 19041–19045,
in `install.wim` or `install.esd`, with compatible Setup index 2. The app reads
actual image indices/edition IDs. Windows 11, ARM, x86, other languages, custom
answer files, custom boot-path driver trees, and split SWM media are rejected.

**Validation status:** Pro was successfully installed using the source project's
v17 ISO. This app's output requires its own end-to-end hardware validation.
Other compatible editions are allowed and explicitly labeled unverified.

## Install on the Mac

1. Use macOS Boot Camp Assistant to create a **BOOTCAMP** partition first. Do not
   give it the patched ISO; it can reintroduce the old Apple graphics driver.
2. Copy the output ISO to Ventoy and use **WIMBOOT** (press **w**).
3. Keep the USB connected. In Setup, select and format only BOOTCAMP.
4. On the Windows desktop, confirm Iris 550 driver **21.20.16.5174**.
5. Run `C:\BootCamp\Setup.exe` as administrator and reboot for Touch Bar and full
   trackpad support. Do not run the older Intel graphics installer.

The app does not partition disks, write USB sticks, change the host's drivers, or
disable signature enforcement. The generated answer file only accepts the EULA
and provides driver paths. The Windows installation license remains your own.

## Build from source

Install Rust stable MSVC and Visual Studio C++ Build Tools with a Windows SDK.
Run from a developer PowerShell, or ensure the C++ toolchain is installed where
the Rust `cc` crate can find it:

```powershell
cargo test --all-targets
cargo build --release
.\target\release\macbook-iso-patcher.exe
```

`build.rs` compiles four statically linked x64 WinPE helpers and embeds them in
the executable. No C compiler is required on an end user's computer. Runtime
PowerShell calls are fixed Windows OS integration commands, with paths passed
through environment variables rather than inserted into script source.

## Structure

- `src/ui.rs`: egui workflow, edition picker, donation page, progress and logs.
- `src/domain.rs`, `wim.rs`: model constraints, build requests, read-only WIM metadata.
- `src/download.rs`: pinned downloads, cache verification, archive extraction.
- `src/platform.rs`: Windows integration and subprocess handling.
- `src/engine.rs`: image export/servicing, validation, recovery, ISO creation.
- `src/ipc.rs`: per-build authenticated loopback connection to the UAC worker.
- `payloads/a1706`: source-derived v17 script and reproducible WinPE helpers.

No ISOs or driver packages are committed. See [driver provenance](docs/driver-sources.md)
and [validation checklist](docs/validation.md).
