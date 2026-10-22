# MacBook ISO Patcher

A native Rust desktop utility for preparing Windows installation media for Intel
MacBooks. No JavaScript, TypeScript, browser runtime, or payment verification.

The first profile targets **MacBook Pro 13-inch 2016 A1706 / MacBookPro13,2**, with
Intel Iris Graphics 550 and T1. It implements the source project's v17 external
HDMI startup sequence for a Mac whose internal display is disconnected.

## Use

1. Run `macbook-iso-patcher.exe` on x64 Windows 10 or 11. Open **Settings** to
   move the driver cache before continuing past model selection. **WorkingSpace**
   can be changed from any idle setup screen before starting an ISO build.
2. Choose a model, then press **Start download & continue to Windows ISO**.
   Every card lists its graphics hardware and verification status. The graphics
   driver is selected automatically for that model's external display output.
   Selecting a model alone does not download anything. The button starts downloading
   that model's files in the background and opens the Windows ISO page.
   Download bytes, progress, live speed in **MB/s**, completion and Retry stay in the center between the
   footer buttons. Completion turns green. Switching models cancels the previous transfer cleanly;
   press the start button again to download the newly selected model's files.
   Verified shared packages are reused from the cache; failed downloads offer Retry.
   All listed models can build after their downloads finish. The **2016 A1706 /
   MacBookPro13,2** uses the hardware-verified recipe; the other models remain
   **Unverified** and use their own Apple Boot Camp packages.
3. Drop an original Microsoft Windows 10 ISO onto the app, or click the drop zone
   to browse. Dropped and browsed files are scanned automatically; pasted paths
   can be scanned with **Scan ISO**.
4. Click the rounded edition rows to select or deselect them.
   Pro has a green **Hardware Verified** label based on the original project's
   A1706 result; other editions have an orange **Unverified** label. This app's
   generated ISOs still need hardware validation.
5. Choose a new output filename. **WorkingSpace** must have enough free space.
   Use **Change WorkingSpace** in Review or **Move WorkingSpace folder** in
   Settings to move it before building. Wait for downloads and other operations
   to finish first; your ISO, edition selections and downloaded driver readiness
   are retained.
6. Install missing prerequisites using **Missing downloads**, highlighted above
   **Build setup**. **Download & install ADK tools** downloads Microsoft's installer,
   verifies its signature, requests UAC, and installs Deployment Tools only. Its
   progress appears in the panel; the ISO tool is detected automatically afterward.
   A manual Microsoft download link is also available. Once both tools are found,
   the panel stays visible as a green **Build tools ready** section. **Build setup**
   appears above **Output & WorkingSpace**.
7. The button says **Downloading**, **Getting ready**, or **Waiting for drivers**
   while prerequisites are pending. It becomes **Build ISO** once ready, and
   **Building ISO** during image building. Press **Build ISO** to start. Its progress
   bar shows download progress, then an animated indicator for build stages that
   do not report a measurable percentage. The note beside
   the button links to GitHub issues: report success or problems, including your
   Mac model and Windows edition. Multiple editions show an optional donation page linking to
   [Ko-fi](https://ko-fi.com/zenodeon); **Continue without donating** always works.
8. Approve UAC when the separate image-servicing worker starts.

Errors open an **Action needed** popup. Build-check failures, including insufficient
WorkingSpace space, offer **Retry** to check the requirements again and continue when
ready. **Close** dismisses the popup so you can change the build settings.
**Copy error** copies the full message for troubleshooting or a GitHub issue.
Download failures also have a **Copy** link beside **Retry** in the footer.

Open **Settings** above Appearance to manage storage:

- **Move cache folder** and **Move WorkingSpace folder** move existing contents to
  an empty destination, including between drives. Settings shows both locations
  and their free space. Cache moves require idle model selection. WorkingSpace
  moves are available on every idle setup screen, including before continuing
  from the optional donation page. Both moves wait for downloads and other
  operations to finish, and neither can run during a build. Copies are
  verified before the app saves the new location and removes the original.
  Retained image mounts must be cleaned up before moving WorkingSpace.
- **Cache** shows reclaimable file count and size. **Clear cache** removes the
  patcher's downloaded driver packages, partial downloads, extraction folders
  and cached ADK setup downloads. Installed tools and unrelated files are kept.
  Drivers must be downloaded again explicitly before building.
- **Builds** lists generated ISOs and their workspaces. Click the rounded rows
  to select them, then **Delete selected** to remove their recorded ISO files,
  temporary media, workspaces and logs after confirmation. Failed or interrupted
  jobs can be cleaned even without a generated ISO. Older jobs with an unverified
  output are labeled workspace-only, and that output is kept.
- Original source ISOs, changed/replaced outputs, linked folders and files in use
  are protected. Build cleanup requests Windows administrator permission when
  needed to discard app-owned WIM mounts. A failed unmount retains the build.

Cleanup runs in the background, reports partial failures with **Copy error** and
**Retry**, and never starts a new build. Build history is stored separately from
settings and remains available when WorkingSpace changes.
Folder choices and preferences are saved in
`%LOCALAPPDATA%\Zenodeon\MacBookISOPatcher\settings.json`; existing Roaming
settings are imported automatically when this file is absent. The folders stay
at your chosen locations. Disk-space errors identify the checked folder and
drive, so cache requirements can be distinguished from WorkingSpace requirements.

Worker connection failures include the connection phase, elapsed time and the
retained `worker-connection-<id>.log` path in the workspace. This log includes
socket timeout settings and worker events, even before `build.log` is created.

The GUI downloads and verifies Intel and Apple drivers before elevation; it does
not install those drivers on the host. Building waits for the selected model's
downloads to finish. The 2016 13-inch models use Intel **21.20.16.5174**; the 2017
Iris Plus 650 model uses **27.20.100.9316** from February 2021. The 15-inch A1707
uses Apple's bundled Radeon Pro drivers for its external output, with no separate
Intel replacement. Its original Radeon INF packages are staged and added to each
selected Windows image. Other Apple hardware drivers remain included.
Old `IntelHDGraphics64` and `IntelIrisSetup` folders are excluded from staged Apple
drivers, including the post-install pass. Other models use available allowlisted
Apple boot drivers, without the A1706-only Iris/HDMI startup changes.
The original signed driver files and Boot Camp software installer stay unmodified. Build
logs and `job.json` stay in the chosen working folder. Existing output files and
the original ISO are never overwritten. Cancellation waits for an active DISM
operation to reach a safe boundary. Retrying with the same working folder
recovers identifiable app-owned mounts from interrupted builds.

After a successful scan, the source ISO stays mounted for the current session.
The source page puts **Selected Mac** above **Source file**, with compatibility
details inside Source file. File fields display normal drive or UNC paths rather
than Windows' internal extended-path prefix. Each page opens scrolled to the top.
**Dismount ISO** replaces the return/back action and appears only once an ISO has
been scanned. Confirm in the popup to eject it and return to **Windows ISO**;
the ISO file stays unchanged. Use **Edit editions** on Review to change editions.
Dismount is unavailable during scans, builds, ADK installation, and another dismount.
Normal app closure releases a source mount created by the app. Closing during a
driver download cancels it and removes its incomplete archive before exit. Closing
during a scan waits for its result; closing during a build first requests safe cancellation
and waits for worker cleanup. Pre-existing user mounts are preserved on closure.
An explicit Dismount action ejects the selected ISO even if it was mounted before
the app opened. Forced process termination cannot guarantee cleanup.

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

1. Prepare a Windows partition using Boot Camp Assistant or another method.
2. Set up a USB drive with [Ventoy](https://www.ventoy.net/),
   then copy the generated ISO onto it.
3. Boot the USB, select the ISO in Ventoy, and choose **WIMBOOT**. Keep the USB connected.
4. Complete Windows Setup normally using the Windows partition you prepared.
5. In Windows, run `C:\BootCamp\Setup.exe` as administrator, then restart.
6. Search Windows for **Apple Software Update**. If missing, install
   [Apple's desktop iTunes installer](https://www.apple.com/itunes/download/win64/),
   then check again. See [Apple's update guidance](https://support.apple.com/en-us/118406).
7. Install the applicable Apple Software Update updates and restart. Check your
   MacBook's features and [report any failures](https://github.com/Zenodeon/MacbookWindowsISOPatcher/issues)
   with your model number/identifier, Windows edition/build, and saved build logs.
   For A1706 boot/setup failures, also include the USB's **A1706Logs** folder when available.

Support varies by model and Windows edition. On A1706, keep the included signed
Iris 550 driver **21.20.16.5174**; do not run the old IntelHDGraphics64 installer.

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
