# Validation

## Storage availability review — 2026-10-21

- 73 automated tests pass; formatting and Clippy with warnings denied pass.
- Artifact inventory tests cover workspace-only, ISO-only, both/neither present,
  missing temporary files/logs, permission errors, wrong file types, replaced
  outputs, retained history, and unique workspace/space counts.
- Confirmation tests cover a disappeared ISO, changed paths requiring renewed
  confirmation, fresh unchanged scans, and stale selections being removed.
- A separate interactive Windows integration test opens a disposable folder with
  spaces using an extended path, then reveals its file and checks the selection
  through Shell.Application. It passes and closes only its own test windows.
  This reproduced and fixed the incorrect System32 Explorer executable path and
  quoting of the /select argument.
- Native screenshots inspected with the actual configured history in light/dark
  themes at 1240 × 700, plus 800 × 520. Both real records are workspace-only;
  no nonexistent output/temporary ISO paths or Delete ISO buttons are shown.
  Actual-history confirmation lists only the existing workspace. Disposable
  fixtures additionally cover a present ISO, a changed output, empty and scanning
  states. Existing path controls remain separate from row selection/deletion.
- Debug preview only: no release build, package refresh, ZIP, push, full ISO
  build or deletion of the user's existing build/cache artifacts was performed.

## Verified on 2026-10-19

- Windows MSVC release build and compilation of all four WinPE helpers.
- 17 automated tests, formatting check, and Clippy with warnings denied.
- Renderer screenshot inspection of the native start screen in light and dark
  themes, with embedded website fonts, creator branding, and sidebar links.
- Native file-drop handling is enabled; drop validation tests cover a local ISO,
  uppercase extension, multiple files, wrong file type, browser-only drops,
  and nonexistent paths. Complete ISO scans still require real Windows media.
- Complete preparation from the actual pinned Intel/Apple downloads: archive
  hashing, Intel Burn cabinet and file-manifest reconstruction, valid catalog
  signature, Apple archive/DMG/driver EXE extraction, all 27 boot-path folders,
  and successful cleanup of the generated extraction directory.
- Complete ISO servicing/publication and A1706 hardware installation have **not**
  been exercised here: no original Windows ISO or ADK ISO tool was available.

## Automated

The current 54 tests, Clippy with warnings denied, and release build pass.
Loopback transport tests reproduce Windows error 10035 with an inherited
non-blocking accepted socket, then verify the explicit blocking-mode fix with
fragmented delayed events and cancellation. Authentication still times out, and
an actual worker disconnect remains an error. Windows error 10060 was reproduced:
after authentication the writer reported no read timeout while its cloned reader
still reported 3 seconds. Both handles are now explicitly reset and checked.
The production handoff helper and cancellation monitor pass a test with more
than 3 seconds of silence followed by a fragmented event and cancellation.
Connection diagnostics retain elapsed time, phase, socket settings and events
in a per-build workspace log without logging authentication tokens.
The compact log viewer was rendered at 1240 × 700 in both themes with long and
multiline entries and 2 px vertical spacing.
Coverage includes model/package mapping, stale download completion, cancelled
model switches, explicit download authorization for each selected model, retry
behavior, and excluding old Apple Intel graphics while
preserving the other device drivers. Cached package verification was exercised
for A1706 and the two additional Boot Camp packages. A real cancelled network
transfer left no incomplete archive in its test cache.
All listed models are accepted by build validation without changing hardware
verification labels. Unverified builds select their own Apple packages and compatible
Intel drivers on 13-inch models, and Apple Radeon for the 15-inch external output.
Tests cover automatic external-GPU package selection and rejecting OEM-only or
incorrect Intel hardware IDs. Boot-driver validation accepts available allowlisted folders and
rejects graphics or unknown folders. Their full ISO builds and hardware operation
remain untested. The A1706-only Iris/HDMI path remains separate.
The pinned A1708 and 2017 Intel packages completed real extraction, matching-INF
and catalog-signature validation, and temporary extraction cleanup. The 15-inch
Apple bundle was also extracted; its loose Radeon display/audio INF paths were
inspected. The revised model cards were rendered in both themes with automatic
graphics selection and consistent verification tags.
Path-display tests cover extended drive/UNC paths and preserving device namespaces;
byte-progress tests cover unknown/zero totals and bounds. Renderer fixtures cover
source-section ordering, footer download progress/errors/Retry, and the Build ISO
button's measurable and indeterminate states. Main scrolling resets on page changes.
The Action needed popup was rendered in both themes using the 51.6 GiB workspace
message, with Copy error, Close and Retry, plus the reported worker error with
Copy error and Close. The compact download-error footer includes Copy and Retry.
Copy actions send the complete error text to the clipboard. Retry routes
through the original build validation entry point and retains the donation flow.
The official ADK bootstrapper download and Microsoft signature verification were
exercised without installing it. The default window was inspected at 1240 × 700
content pixels; proportional fitting is covered by tests. Live ADK installation
still needs integration testing. Two opt-in Windows integration tests use generated
48 KiB data ISOs to verify explicit eject, owned/borrowed cleanup, failed scan
cleanup, and normal GUI closure with both owned and pre-existing mounts:
`cargo test --test iso_mount -- --ignored --test-threads=1`.
The edition rows, green tools-ready
panel, and dismount confirmation were rendered and visually checked in both themes
using debug-only screenshot fixtures, which are excluded from release builds.

## Storage settings verified on 2026-10-20

- Cache inventory/deletion tests cover allowlisted packages, UUID partial files,
  extraction and ADK setup folders, counts/sizes, unrelated files, original ISO
  containment, active locks and junction refusal.
- Build tests cover importing older jobs, missing files, changed/replaced ISO
  identities, original ISO and path traversal protection, active build locks,
  mounted ISO refusal, unknown WIM ownership and partial deletion failures.
- Driver state tests confirm cache cleanup revokes readiness and download
  authorization, rejects stale completion, and requires an explicit download.
- Settings was rendered at 1240 × 700 and 800 × 520 in light/dark themes, including
  empty, populated, scanning, deleting, confirmation and partial-error states.
  Errors and Copy error/Retry remain above the list; Close remains visible.
- A separate elevated integration test generated a disposable data ISO and WIM,
  mounted the WIM inside its recorded job, then removed its ISO, workspace and
  history while preserving the source file. It also verified no mount remained.
  A Windows partial-directory-unmount error required one bounded discard retry;
  failures still retain the build instead of deleting live mounted files.

Run the opt-in mount cleanup test from an administrator terminal:
`cargo test --lib cleanup::tests::windows_generated_iso_and_owned_wim_cleanup -- --exact --ignored`.
It needs ADK oscdimg but never services or installs the host OS. On failure its
own test fixture is retained for mount recovery; other mounted images are ignored.

## A1706 full-build test on 2026-10-20 — blocked by host servicing

The real Windows 10 22H2 English x64 ISO was tested with A1706 and Pro (source
index 6). Its SHA-256 matches Microsoft's published English x64 checksum:
`a6f470ca6d331eb353b815c043e327a347f594f37ff525f17764738fe812852e`.
The original ISO had the same hash after the test.

- Driver preparation, UAC handoff, media copy, Pro export and image mounting
  completed. The authenticated reader remained connected beyond the previous
  three-second failure point, with both read timeouts confirmed as `None`.
- Adding the signed Intel driver failed during DISM initialization with error 87,
  before driver installation. DISM recorded failure to determine the offline OS
  version (`0x80070057`). The app reported the failure and discarded its mount.
- Read-only checks with system DISM 10.0.26100.9549, ADK DISM 10.0.26100.9457
  and the original image's DISM 10.0.19041.844 all failed at initialization.
  All probe mounts were subsequently discarded.
- The exported SOFTWARE hive is byte-identical to the source Pro hive. A separate
  control saved an 8 KiB hive from the host's timezone key successfully, but Windows
  `reg load` rejected both that control and the source hive with “The filename or
  extension is too long.” The control also failed at `C:\zp.hiv` with a short
  registry key name. Temporary files and successful probe mappings were cleaned up.

No finished ISO was produced. Host offline-registry loading needs to work before
the full build can be accepted; changing DISM versions alone did not resolve it.
Retained local evidence is in `.cache/a1706-full-build.log`,
`.cache/dism-probe.log`, `.cache/registry-control.log`, and the per-build connection
log under `.work/a1706-test`. A Windows restart followed by another control test
is the next diagnostic step, not a verified repair. Hardware acceptance remains
pending even after ISO publication succeeds.

Run `cargo test --all-targets` and `cargo clippy --all-targets -- -D warnings`.
Tests cover metadata compatibility, real image indices, edition validation,
boot command composition, startup ordering, and absence of disk automation.

Driver extraction can be exercised against pinned packages independently of
servicing: `cargo run --example prepare_drivers -- <cache-folder> <7z.exe>`.
This downloads missing packages and extracts them without administrator access.
It removes its extraction directory on completion and retains the verified archives.

## Windows integration (requires original Windows media and ADK)

- Scan a multi-edition WIM ISO and an ESD ISO; compare names, indices, edition
  IDs, version, architecture and language against DISM.
- Build Pro alone, Home alone, and Pro+Home. Confirm every selected image has
  the signed Intel package, Drivers, BootCamp, and SetupComplete.cmd.
- Confirm multi-edition Setup offers the selected editions, and a single-edition
  output targets the selected edition. Check that no original PID file survives.
- Skip the donation page and confirm the same multi-edition job runs.
- Verify Setup index 2 contains the signed driver, scripts, helper binaries and
  DisplaySwitch. BasicDisplay Start=1; igfx Start=3.
- Confirm all and only allowlisted folders exist under `$WinPEDriver$`.
- Confirm the original ISO hash is unchanged; existing outputs cannot be replaced.
- Test Unicode/spaced paths, output on NTFS and exFAT, low disk space, missing
  build tools, a bad cached hash, interrupted downloads, and UAC refusal.
- Cancel during copying, downloading, image export, servicing and ISO creation.
  Confirm image cleanup. Kill the worker while mounted, then retry using the
  same workspace and confirm app-owned mount recovery without touching others.
- Mount the finished ISO and compare its selected image metadata with the plan.

## A1706 hardware acceptance — pending

- Boot via Ventoy WIMBOOT, internal display disconnected, HDMI dongle attached.
- Require `iris started` before `launching setup` in A1706Logs.
- Confirm visible Windows Setup, no boot-critical-driver dialog, successful
  installation to BOOTCAMP and an external picture at the first desktop boot.
- Require Iris 550 driver 21.20.16.5174 in Device Manager.
- Run C:\BootCamp\Setup.exe, reboot and verify right-click, scrolling and Touch Bar.
- Validate Home separately before advertising hardware support beyond Pro.

The source project's recorded Pro success is evidence for the recipe, not a
substitute for testing an ISO generated by this application.
