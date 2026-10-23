# Validation

## DriverStore validation and closing after a missing source — 2026-10-22

- A retained 2017 build log showed Intel DISM import succeeded before validation
  rejected `igsc.dll`. The checker incorrectly treated all SourceDisksFiles
  entries as required DriverStore files. The pinned 5927 install section does
  not reference the OpROM updater copy section containing that DLL.
- Source extraction still requires the complete declared download contents.
  DriverStore checks now follow the GPU's actual CopyFiles directives, require
  the graphics kernel, INF and catalog, and compare every present package file
  with the pinned extraction. Missing referenced files, altered binaries,
  undeclared files and unexpected install dependencies still fail validation.
  The real pinned INF resolves to 115 required store files, excluding the unused
  OpROM tools. Synthetic regressions cover those differences and malformed INFs.
- Missing-source dismounting now consults Windows' attached disk-image inventory.
  An absent mount succeeds without requiring the original file; a matching
  attached ISO is ejected, and unrelated images remain untouched. Inventory and
  eject failures remain errors. Close-time cleanup errors offer Retry cleanup,
  Cancel, Copy error and an explicit Close app action after active work settles.
- 156 normal automated tests passed, with formatting and Clippy passing. A
  separate Windows Storage test mounted a disposable data ISO, ejected it,
  deleted its backing file and verified cleanup succeeds. It also checked a
  missing parent directory. Script fixtures cover unrelated/ambiguous mounts,
  inventory errors, failed ejection and a mount remaining attached. UI tests
  verify the explicit quit action cannot bypass an active build.
- Debug screenshots at 1240 × 700 and 800 × 520, in both themes, are saved as
  `.cache/close-cleanup-{dark,light}-{normal,small}.png`.
- The separate copied boot/install-image servicing probes still fail host DISM
  provider initialization with Error 87 before driver import. Their mounts were
  discarded, and `.cache/igsc-final-mounts.log` confirms none remain. This limits
  real-image validation; these probes do not establish a new recipe failure or
  successful 2017 hardware compatibility. Retained logs are
  `.cache/igsc-disposable-image.log` and `.cache/igsc-install-integration.log`.
- Debug preview only. The published 0.1.0v EXE was not rebuilt or replaced.

## A1706 2017 external-display recipe — 2026-10-22

- Added a separate MacBookPro14,2/Iris Plus 650 Setup recipe. It uses pinned
  Intel 27.20.100.9316 and Apple Boot Camp 041-89042; the model remains Unverified.
  The 2016 startup script remains byte-for-byte unchanged and other models do
  not receive this startup recipe. Source BCD settings remain untouched.
- Production-path native tests cover immediate/delayed discovery, enumeration
  errors, absent/ambiguous devices, rejection of 2016 IDs, structured status,
  wrong driver/service binding and the 60-second readiness timeout. Script tests
  protect one restart, readiness before Setup and logging before wpeinit.
  A real CMD fixture verifies loading nested 2017 INF filenames with spaced paths.
- The real cached packages passed SHA-256, extraction, catalog-signature, Intel
  version/device mapping, declared package-file and Apple boot/storage checks.
  DriverStore fixtures reject missing, changed and unexpected package files.
  Registry fixtures verify the 2017 INF-defined DriverStore service path,
  demand start, enabled BasicDisplay and preservation of the 2016 defaults.
- Collector tests verify explicit profile validation and propagation to the
  background watcher, separate 2016/2017 USB session destinations and recovery
  context. Existing marked-USB, retention-limit and collection-failure tests pass.
  Manifest, guide, issue-draft and progress tests include the 2017 profile.
- Final checks: 152 tests passed, 6 integration tests ignored by the normal
  suite; formatting, Clippy with warnings denied and the debug build passed.
  The administrator servicing test was run separately and failed as detailed below.
- Actual debug-renderer screenshots cover Review, USB preparation and reporting
  in both themes at 1240 × 700 and 800 × 520. Files are
  `.cache/setup2017-review-dark.png`, `.cache/setup2017-complete-{dark,light}.png`,
  `.cache/setup2017-small-{dark,light}.png` and `.cache/setup2017-report-dark.png`.
  Screenshot USB paths are fixtures; no user USB was changed.
- **Disposable-image servicing remains blocked by the host.** The administrator
  integration test mounted a copied stock boot.wim index 2, but system DISM
  10.0.26100.9549 failed WinPE/OS-provider initialization with Error 87 before
  driver import. Retesting in a shorter workspace failed identically. An
  independent freshly saved registry hive also failed `reg load` with
  "The filename or extension is too long." This reproduces the existing host
  offline-registry problem; a restart is a diagnostic next step, not a confirmed
  repair. Retained logs: `.cache/setup2017-elevated{2,3}.log` and
  `.cache/setup2017-hive-control.log`. The control confirmed no mounted images
  remain. No successful real-image service configuration/commit is claimed.
- The ignored administrator regression test
  `engine::tests::a1706_2017_boot_recipe_persists_driverstore_service_and_payload`
  is retained for rerunning after host servicing works. It requires explicit
  stock-WIM/CAB paths and a fresh `.cache/s17-*` fixture root. It checks import,
  service configuration, payload placement, commit/remount and source integrity.
- No full ISO rebuild, release build, ZIP, push or closure of the running app.
  Hardware acceptance still requires the intended external-display connection
  on the actual 2017 Mac, Windows installation and post-install device checks.

## Completion-page USB selection and reporting — 2026-10-22

- The single **Share logs / report an issue** action is now under **Something
  did not work?**, preserving the issue draft and manual-attachment tooltip.
- USB selection appears immediately in a wrapped, selectable rounded path area.
  Preparing, prepared and failed states are distinct. Failed preparation keeps
  its selected path and the existing copyable error popup. Cancellation preserves
  the previous selection; a new build or workflow reset clears session state.
  Request IDs prevent late preparation results restoring a cleared selection.
- All 41 binary/UI tests pass, including four new state-lifecycle tests.
  Formatting check, Clippy with warnings denied and the debug build pass.
  USB validation/marker writing and GitHub draft generation are unchanged.
- Actual renderer screenshots cover both themes at 1240 × 700 and 800 × 520,
  long paths, preparing/failed states and the reporting section. They are saved
  as `.cache/usb-selected-*.png`, `.cache/usb-preparing-dark.png`,
  `.cache/usb-failed-light.png` and `.cache/completion-report-dark.png`.
  Selected destinations in screenshots are simulated; no user USB was modified.
- Debug preview only. No release build, ISO rebuild, ZIP, push or closure of
  the user's running app.

## Output choices and workspace-only Settings — 2026-10-22

- Existing destinations return a typed `engine::OutputConflict`. The app checks
  publication history in the background, presents file details and provides
  replace, increment, rename and cancel actions. Replacement confirms permanent
  deletion in the same dialog. Copy error remains available after checking.
- Replacement checks source metadata, driver readiness, tools, retained worker
  locks and disk space before requesting the existing authenticated ISO-only
  worker. Volume identities limit freed-space credit to the ISO's own volume.
  The worker rechecks recorded identity, source protection, activity locks and
  mount state. It deletes only the output; normal preflight then starts a fresh
  build. A subsequent preflight failure explicitly says the old ISO was deleted.
- Filename tests cover occupied alternatives, numeric suffix continuation and
  nonnumeric parentheses. Rename cancellation leaves the output and popup intact.
  UI state tests cover typed versus generic failures, failed/cancelled cleanup,
  successful cleanup continuation and follow-up preflight failure. Publication
  collision tests confirm an existing destination is never overwritten.
- Settings emits only `CleanupRequest::Workspaces` for build deletion. Workspace
  scans/counts/totals exclude external temporary files and final ISOs, hide
  ISO-only history, and keep publication history. Tests cover changed outputs,
  hard links, unknown/missing files, reused output names, active locks and failed
  unmounts. Generated ISOs inside either their own or another recorded workspace
  are excluded from sizes and prevent cleanup with a move-first explanation.
- The reference warning was removed from Review, Complete and its former helper.
  Revision parsing, backward-compatible image metadata, support manifests and
  supported-source checks remain intact. Driver and Setup patching are unchanged.
- Actual debug renderer captures were inspected at 1240 × 700 and 800 × 520 in
  light/dark themes: output choices, confirmation, busy state, workspace list and
  workspace confirmation. Disposable files supplied conflict/list fixtures; a
  separate Settings summary capture scanned the current saved history. Captures
  are in `.cache/output-*.png`, `.cache/workspaces-*.png`,
  `.cache/workspace-confirm-small-light.png` and
  `.cache/workspace-summary-live-light.png`.
- `cargo fmt --check`, full `cargo test --quiet`,
  `cargo clippy --all-targets --quiet -- -D warnings`, and a debug build pass.
  140 tests pass; five existing administrator/live-media integration checks are
  ignored. Mount/error branches were exercised through disposable fixtures and
  injected callbacks, not a new live elevated deletion or WIM mount operation.
  Native save-dialog cancellation is covered at its returned-result boundary.
- No release package, ISO rebuild, ZIP or push. No user output ISO was deleted,
  no existing ISO mount was dismounted, and the user's running app was left open.

## A1706 compatibility and USB Setup diagnostics — 2026-10-22

- Preserve the v17 GPU/Setup ordering, Intel 21.20.16.5174, BasicDisplay and
  igfx settings, boot allowlist, source BCD, and existing driver-only
  SetupComplete. Other model patching is unchanged. No persistent collector.
- GPU production-loop tests inject immediate/delayed discovery, a changing
  enumeration buffer, API failure, no match, ambiguous matches and a wrong
  hardware-ID prefix. Startup rejects empty IDs and has one restart command.
- A native C collector starts before wpeinit and records bounded timestamped
  events, dialog labels, device state/version, storage space and separate
  Panther/SetupAPI/rollback snapshots. It deliberately skips edit controls.
  USB copying is asynchronous relative to the startup script. Collection errors,
  tail truncation, log limits and running/interrupted sessions are explicit.
- Native disposable fixture tests exercise the real watcher and copy routines:
  marked/unmarked destinations, multiple USB destinations, marker removal and
  reattachment, fixed-disk eligibility abstraction, read-only and simulated-full
  destinations, locked sources, partial failures, 16 MB tails, spool limits,
  path escapes, junctions, separate sessions, context reopening and recovery
  isolation. Tests do not read host Windows Setup logs or launch Setup.
- The production read-only USB check recognized the attached Ventoy D: volume,
  matching Windows' USB bus classification, and rejected C:. No marker or log
  was written to the user's USB. Physical USB marker creation/removal during
  a real WinPE installation remains to be tested on hardware.
- Packaging tests verify native executable headers, boot payload contents,
  model/build manifests, the copyable marker and collector launcher. Manifests
  omit source directory paths, retain image revisions, and identify dirty code
  builds. Older saved image/inventory JSON remains readable.
- Completion now has a separate Prepare USB for setup logs card after Ventoy
  instructions. Review/completion show source-reference differences while
  keeping supported inputs buildable. Matching revision is not ISO verification.
- Full tests: **127 passed, 5 ignored**. Formatting and Clippy pass. Debug preview
  only. Actual app screenshots cover dark/light 1240 × 700 and a smaller
  800 × 520 window; sidebar/page content remains scrollable. Explorer inspection
  shows generated Support tools and a clearly labelled disposable log session.
- The working manual ISO is untouched. No ISO rebuild, release build, ZIP,
  push, actual Windows installation or A1706 hardware test was performed.
  Hardware validation against the verified manual ISO remains pending.

## WorkingSpace naming and setup-stage moves — 2026-10-22

- Renamed the configured folder to WorkingSpace in Review, Settings, space
  requirements and relocation errors. Review now shows its saved path and a
  Change WorkingSpace button which uses the verified relocation operation.
- WorkingSpace moves are enabled on idle Model, ISO, Editions and Review screens,
  including the optional donation screen. Downloads, scans, installation, cleanup,
  moves, closing, recovery failures and active builds block changes. Failed builds
  can return to setup before moving. Cache moves still require idle model selection.
- Relocation gates now depend on folder kind in both UI and backend. Build
  preflight and build initiation wait for moves. WorkingSpace completion preserves
  the ISO, model, editions, output filename, donation state and driver readiness;
  the next build request uses the new saved path with existing settings keys.
- All-target tests: 120 passed, 5 existing environment-specific tests ignored.
  Added setup-stage/busy-state coverage, backend gate checks and a real move from
  Review followed by UI completion, selection/readiness checks and settings reload.
  Clippy passes with warnings denied. Renderer captures at 1240 x 700 and 800 x 520
  in dark/light themes verify Review controls, setup/donation availability and
  build-time locking; retained in `.cache/workingspace-ui-20261022/`.
  `cargo build --locked --release` succeeds and refreshes
  `target/release/macbook-iso-patcher.exe`.

## Download rates and movable storage — 2026-10-21

- Added two-second rolling download rates in decimal MB/s for driver transfers,
  build-time downloads, and the app-managed ADK installer download. Progress
  remains usable without a Content-Length and falls to zero during stalls;
  transfer state clears before verification.
- Settings exposes folder paths, free space, Move cache folder and Move workspace
  folder. Moving is restricted to idle model selection, including backend guards.
  Continue and competing operations are blocked while moving; closing waits.
- Relocation copies to a sibling staging folder, verifies SHA-256 hashes, updates
  settings and build ownership metadata, then removes original contents. A journal
  supports rollback before commit and retains original leftovers after commit.
  Locked files, protected source ISOs, links, overlapping paths, destination
  conflicts and retained image mounts are rejected or reported without losing the
  verified copy. Workspace relocation preserves output identity checks and cleanup.
- Settings now live in Local AppData, with automatic import of legacy Roaming
  settings. Space failures include the checked folder, drive, available and required
  bytes. Canonical and normal Windows paths identify the same imported build.
- `cargo test --locked --all-targets`: 117 passed, 5 existing explicit-environment
  tests ignored. Coverage includes a real C: to E: cache move, history refresh and
  ownership-checked deletion after a workspace move, sharing violations, junctions,
  verification failure, interrupted moves, startup lock contention, persistence,
  workflow gating, rate calculation and serialization compatibility.
- Clippy passes with warnings denied; `cargo build --locked --release` succeeds
  and produces `target/release/macbook-iso-patcher.exe`. Native renderer captures inspected in dark
  and light themes at 1240 x 700 and 800 x 520 cover Settings, disabled and moving
  states, driver downloads and build downloads with known/unknown totals. Captures
  are retained in `.cache/relocation-ui-20261021/`. Full ISO and hardware boot tests
  were not run for these changes.

## Dedicated completion page and shared installation guide — 2026-10-21

- Finished now navigates to Complete and resets the page scroll to the top.
  The page shows the generated filename/path, Mac profile/identifier, editions,
  and installation instructions. The fixed footer exposes Save build logs and
  Open ISO location; Explorer selects the file after rechecking its existence.
  A missing/moved file produces the existing actionable Copy error popup.
- One shared seven-step guide supplies both the UI and the ISO's model-specific
  README. It allows Boot Camp Assistant or another partitioning method, explains
  Ventoy USB setup and menu-selected WIMBOOT, then Boot Camp Setup.exe, restart,
  checking for Apple Software Update, desktop iTunes only if missing, applicable
  Apple updates, and another restart. No keypress instruction remains in the
  shipped guide. Model/edition support is not guaranteed; A1706 retains its
  graphics-installer and USB A1706Logs guidance.
- Official Ventoy, desktop iTunes, and Apple update links open in the browser.
  Report an issue opens an encoded GitHub draft containing model/profile,
  identifier, Windows editions/build, graphics, and app version, with attachment
  instructions. Nothing submits an issue, uploads logs, installs software, or
  changes partitions automatically.
- Added authenticated BuildFiles metadata carrying the job and full log path;
  Finished and job/history formats remain unchanged. Both pages use one export
  action: copy the full retained log where available, otherwise clearly label a
  buffered partial export in the dialog, filename, saved header, and notice.
  Exports require a .log destination and cannot overwrite their retained source.
- Replaced pink/maroon/purple surfaces with blue/teal in both themes. Errors,
  missing-status text, and unverified badges use amber; readiness remains green.
  Sidebar step gaps were tightened so the new Complete step preserves access to
  the existing footer links at the default size.
- 98 normal tests pass; formatting and Clippy with warnings denied pass. Coverage
  includes actual scroll-state reset, missing ISO actions, 4,500 retained log
  entries without truncation, missing/invalid log fallback, save failures/source
  protection, issue-draft escaping and metadata, guide sequence, and fragmented
  BuildFiles transport on the authenticated connection path. The separate native
  Explorer integration passes with disposable spaced/extended paths and confirms
  selection; it closes only its own test windows.
- Native debug screenshots inspected in light/dark at 1240 x 700 and 800 x 520,
  including top/bottom of the guide, unverified model, and cool prerequisite
  surfaces. Preview ISO/log files are disposable UI fixtures, not built media.
  No full ISO build, release build, package refresh, ZIP, push, issue submission,
  software installation, or closure of the user's running app was performed.

## Larger build log and manual restart — 2026-10-21

- Tightened build-page heading/progress spacing while retaining both stacked
  bars and the live stage/details. Removed the redundant Build details heading;
  the expanded log now uses the remaining viewport height without a 320px cap.
  It opens by default and retains its own scrolling, compact line spacing,
  automatic following, and header-row Save visible log control.
- Failed builds expose Retry in the existing Copy error popup and Retry build
  on the page after dismissal. Back to setup returns to editable output/workspace
  settings. The failure remains copyable on the build page.
- Manual retry runs the normal prerequisite/disk-space checks and starts the
  normal build pipeline from step 1, with a new UUID job directory. It does not
  resume partial output or delete the previous workspace. Verified downloads are
  reused. Progress, cancellation, and the visible log reset only after preflight
  succeeds; failed checks retain the previous attempt's diagnostics.
- Preflight checks the workspace build lock before preparing files. A retry also
  checks the previous attempt's workspace if the user changes the destination,
  so a surviving worker blocks another attempt. The elevated worker still takes
  its normal exclusive lock before mount recovery and job creation.
- 92 normal automated tests pass; formatting and Clippy with warnings denied
  pass. New tests verify retained diagnostics on failed prerequisites, restart
  at step 1 with unchanged selections, retry eligibility, active/released worker
  locks, and at least 180px of log viewport at 1240 x 700 in active/failed states.
- Actual debug-app previews inspected in both themes, including failure popup,
  persistent Retry build/Back to setup, and an 800 x 520 active view. At the
  default size roughly ten log lines are visible. No full ISO test, release
  build, package refresh, ZIP, push, or deletion of retained user files.

## Build progress, log header, and limited Settings — 2026-10-21

- Progress shows the live stage/details, current DISM or Oscdimg operation,
  and overall build steps. Tool percentages appear only in Progress; the footer
  and build button share the overall completed-step fraction. Overall measures
  actual operations, not time, and reaches 100% only on Finished.
- The authenticated transport carries typed operation/overall events. Tool output
  is read incrementally across CR/LF and fragmented chunks while retaining log
  messages. Supported percentage formats are recognized only for the relevant
  tools. Each command resets its operation state; terminal states clear operation
  and download details while preserving completed overall steps on interruption.
- Build step totals account for each selected edition's export/patch and the
  model-specific Windows Setup patch. Preparation, administrator handoff, media
  copying, driver staging, validation, ISO creation/publication, and cleanup are
  included. Windows image servicing itself was not rerun for this UI change.
- Save visible log shares the expandable heading row and is hidden when closed.
  The recovered space goes to the compact log viewport, sized to the visible
  area. Saving reports write errors and never toggles the log.
- Settings remains openable. Selecting a model alone does not lock cleanup.
  Continuing to the ISO page locks destructive actions, including backend retry
  dispatch; existing activity guards remain in place. Browsing, refresh, folder
  links, and copying errors remain available. The cache path now opens its folder.
- 89 normal tests pass, plus formatting and Clippy with warnings denied. Tests
  cover supported/unsupported percentages, fragmented UTF-16/CR/LF updates,
  overall totals and terminal states, fragmented progress events on authenticated
  loopback transport, save-button placement/click independence, and cleanup guards
  before/after Continue. Existing popup geometry/hover tests also pass.
- Actual debug-renderer screenshots inspected in both themes at 1240 x 700;
  smaller 800 x 520 captures verify stacked progress and scrollable limited
  Settings. Settings captures use current history. No cleanup action, full ISO
  build, release build, package refresh, ZIP, push, or closure of the running app.

## A1706 fresh-image graphics preparation — 2026-10-21

- The working project's earlier scripts explicitly prepared the offline `igfx`
  service and kernel binary. The fresh-ISO path previously imported the signed
  package and only checked for that service. A successful DISM import in the
  user's log was followed by a missing `igfx` registry key.
- A1706 boot.wim index 2 now copies `igdkmd64.sys` from the validated signed
  driver store, checks it against the Intel package, and prepares the offline
  service with Type=1, Start=3, ErrorControl=0, Group=Video, and its image-relative
  System32 driver path. The INF/catalog remain unchanged. Other model paths do
  not perform this preparation.
- Resolve Current/Default/LastKnownGood control sets through the image's Select
  key. Validate all referenced sets before writing; refuse disabled/missing
  BasicDisplay or unsafe StartOverride values. Explicitly dispose registry
  handles before unloading the hive; unload errors stop image publication.
- 82 normal automated tests pass; formatting and Clippy with warnings denied
  pass. Disposable HKCU fixtures exercise the production registry functions for
  missing/existing services, idempotence, non-001 control sets, inherited
  boot-start correction, invalid selectors, unsafe BasicDisplay/overrides, and
  invalid binary paths. A separate test rejects a changed kernel binary before
  writing to the image.
- The elevated opt-in DISM test mounts a disposable stock boot.wim copy, imports
  the pinned signed driver, reproduces the missing-service check, prepares it,
  commits, remounts, and verifies persistence. On this host the attempted run
  stopped before preparation: DISM /Add-Driver returned initialization error 87.
  A separate untouched-hive load also failed ("filename or extension is too
  long"). The disposable WIM mount was successfully discarded. Image-level
  persistence and an end-to-end ISO build therefore remain unverified here.
- The original v17 ISO is absent from the reference project; comparison used
  its retained scripts, REPRODUCE.md and WorkingReport.md. No release build,
  package refresh, ZIP, push, hardware boot test, or full ISO build was performed.

## Storage navigation review — 2026-10-21

- Follow-up: replaced minimum-height sizing with one exact outer allocation.
  All routes use the same clipped scrollable body and a separately bounded
  footer, so child content cannot enlarge the popup. Expanded cache confirmation
  was captured using the real cache, alongside Settings/Builds/build confirmation
  in both themes and a smaller-window cache capture. No additional popup is
  created for any of these views.
- Removed bulk selection, selection highlights and the selected-count footer.
  ISO-only and full deletion remain available on individual cards.
- Settings, Builds, confirmation and checking states share one bounded shell
  and an anchored footer. Navigation is applied between frames. Back restores
  the build list's scroll identity; entering from Settings starts at the top.
- 80 automated tests pass, including production-popup geometry checks at
  1240 × 700, 800 × 520 and 640 × 400, plus Back/Escape cancellation and busy
  navigation checks. Cancellation is handled before a ready preflight can
  dispatch cleanup. Formatting and Clippy with warnings denied pass.
- Geometry checks also cover cache confirmation, very long cache paths/notices,
  large errors and unusually long build text. A pointer-click regression verifies
  Cancel in the fixed footer returns to Builds without changing the popup bounds.
- Hover clipping regression checks the actual painted outlines against their clip
  rectangles on Refresh and Close in both themes. Body/footer clips now permit
  widget expansion into existing frame padding while leaving layout bounds exact.
  Native debug hover screenshots confirm the complete outlines without moving the
  user's OS cursor; the hover input override is absent from release builds.
- Actual-history native screenshots checked in both themes, with separate
  confirmation/checking captures and a smaller-window confirmation capture.
  No release build, package refresh, ZIP or push was performed.

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


## MacBookPro13,1 and MacBookPro13,3 testing recipes (2026-10-23)

The local development build now enables external-display Setup recipes for
A1708 2016 / MacBookPro13,1 and A1707 2016 / MacBookPro13,3. Both remain
Unverified. No hardware acceptance, release build, complete ISO rebuild or push
is implied by these checks.

- A1708 retains Apple 041-88734 and Intel 21.20.16.5174, matching present
  8086:1926 devices. It uses the 2016 signed-kernel copy and demand-start igfx
  configuration with BasicDisplay enabled. Touch Bar drivers are optional and
  excluded from its explicit startup load loop.
- A1707 retains Apple 041-88737 and its signed Radeon 21.19.128.7 display INF.
  Supported variants are Pro 460 (0160106B/C0), Pro 455 (0166106B/C7), and Pro 450
  (0167106B/EF), all device 1002:67EF. Discovery ignores Intel HD 530. Readiness
  checks the associated amdkmdap device service; amdkmdap and amdkmdag remain
  demand-start, with their actual serviced kernel locations and the INF's
  PowerXpressLoad setting. Installed Windows retains the other bundled AMD
  driver packages; WinPE receives the selected display INF.
- New recipes retry discovery five times and wait up to 60 seconds for a
  started device without problems, bound to the pinned service/version. They
  restart exactly one selected device and stop on failed restart/readiness.
  Existing 13,2 startup bytes and the 2017 helper/recipe remain unchanged.
- The real downloaded Radeon package passed catalog signature validation and
  checks for 140 selected CopyFiles/coinstaller dependencies. DriverStore
  validation follows those dependencies and compares installed files with
  the pinned package, including nested kernel paths. Unused source-media
  files are not required in DriverStore. The pinned Intel download SHA256
  matches aab51027dac8210bca98b6679472b2a00ab15a5d4f8a6b777259200de52e48e2;
  its catalog signature and 1926 support also passed validation.
- Tests cover delayed/error/missing/ambiguous discovery, each Radeon variant
  beside Intel HD 530, incorrect service/version, readiness timeouts, restart
  gating, optional driver folders, missing/altered serviced files,
  offline-registry configuration in disposable HKCU trees, payload placement,
  manifests, issue drafts and automatic/manual/background logging profiles.
- Model logs use MacBookPatcher-Logs/A1708-2016 and
  MacBookPatcher-Logs/A1707-2016. Marker validation, collection bounds, async
  writes and the two existing profile folders retain their behavior.
- Actual debug-rendered screenshots were inspected in light and dark themes
  at 1240 x 700 and 800 x 520, including scrolled USB-preparation cards. They
  are UI fixtures, not evidence of a successful ISO or hardware installation.
  Screenshots and command output are retained locally under
  .cache/recipe-research-13-3/.

Hardware acceptance must include Setup through a direct USB-C HDMI/DisplayPort
adapter, installation and device checks on the selected Mac, including the
intended disconnected-panel configuration. Each Radeon variant requires its
own acceptance; success on one does not verify all three.


Disposable-image result: all four combinations (both models, boot index 2 and
install index 1) mounted successfully but DISM /Add-Driver returned 87 before
model-specific configuration. The system DISM log reports OS-provider failure
to determine the offline OS version, HRESULT 0x80070057. This reproduces the
previous local servicing limitation, and does not establish a driver or recipe
failure. All four mounts were discarded successfully and source-file hashes
were unchanged. Real DriverStore/service persistence after servicing remains
unvalidated here. Retained output: offline-final.stdout.log,
offline-final.stderr.log and offline-final-dism.log under the research cache.


Delivery checks: 162 automated tests passed; formatting and Clippy with warnings
as errors passed. The separate real Radeon-package validation passed. Seven
library and three ISO-lifecycle tests are opt-in/ignored in the default suite;
the new elevated image test was run separately and failed only at DISM's
initialization stage described above. The debug preview was compiled.

The four unmounted disposable WIM copies remain in .cache/r16-6612b5ba
(10,749,772,884 bytes total). Automatic approval review rejected their cleanup
with "blocked by policy". No cleanup was retried through another mechanism.
