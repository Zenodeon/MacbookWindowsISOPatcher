# A1706 manual and app ISO comparison

Inspected 2026-10-22. This compares the actual Desktop artifacts, rather than
assuming that either contains the latest repository code.

| Artifact | Manual | App |
| --- | --- | --- |
| Filename | `Win10_Pro_A1706_SetupGUI_v17.iso` | `Win10_a1706_Intel_Patched.iso` |
| ISO bytes | 6,554,916,864 | 6,456,293,376 |
| Installed edition | Windows 10 Pro x64 | Windows 10 Pro x64 |
| Install image version | 19045.3803 | 19045.2965 |
| Both WinPE image versions | 19041.3803 | 19041.2965 |
| boot.wim bytes | 785,038,908 | 924,469,913 |
| install.wim bytes | 5,254,581,193 | 5,016,746,862 |

## Matching patch components

- Both have two boot images, with index 2 designated as the boot image, and
  one Windows 10 Pro install image.
- Both stage Intel graphics **21.20.16.5174**, dated 2020-11-08, in the Setup
  and installed Windows driver stores. Their `igdlh64.inf` and `igdkmd64.sys`
  contents match by WIM SHA-1. The kernel SHA-1 is
  `1d6953678780f23b1d84f7dcfd46f53057c4eaf2`.
- Setup SYSTEM hives both have `ControlSet001\Services\igfx` with Start=3,
  Type=1, ErrorControl=0, Group=Video, and ImagePath
  `\SystemRoot\System32\drivers\igdkmd64.sys`.
- Both keep BasicDisplay Start=1. Neither has an igfx StartOverride.
- Both installed-Windows SYSTEM hives lack an igfx service before first boot;
  both nevertheless contain the same staged Intel driver package. Its absence
  here is not an app-only difference.
- Both launch `a1706.cmd` through Setup's `winpeshl.ini`. Both scripts retain
  the v17 flow: initialize WinPE, restart Iris, load keyboard/audio/Touch Bar
  drivers, wait for Iris, select the external display, start Setup, and retain
  diagnostics under `A1706Logs`.
- The 32 files under the installed image's `BootCamp` directory match by WIM
  SHA-1. All 841 shared files under `Drivers` match as well.
- `SetupComplete.cmd` is identical and runs
  `pnputil /add-driver %SystemDrive%\Drivers\*.inf /subdirs /install`.
- `autounattend.xml` has identical parsed settings; formatting differs.
  `sources\ei.cfg` is identical and selects Professional/Retail.
- Outside WIMs, 1,705 files match by SHA-256, including all files in the
  shared ISO-root Boot Camp and `$WinPEDriver$` trees.

## Meaningful differences

1. **Different Windows source revisions.** The manual image has revision 3803;
   the app image has revision 2965. Setup executables, Windows components,
   boot binaries and many other files consequently differ. These are not a
   controlled comparison of the patcher using identical source media.
2. **Different boot signing configuration.** Both manual BCD stores
   (`boot\bcd` and `efi\microsoft\boot\bcd`) set OS-loader elements
   `16000048` and `16000049` to binary `01`. Both elements are absent from the
   corresponding app loader. Microsoft identifies these as
   [nointegritychecks and testsigning](https://learn.microsoft.com/en-us/windows/security/operating-system-security/data-protection/bitlocker/bcd-settings-and-bitlocker).
   This records configured values, not proof of their effective behavior under
   the Mac's boot environment or proof that they are required.
3. **Thunderbolt package missing from the app install image's Drivers folder.**
   Manual has `Drivers\Thunderbolt\tbt81x.inf`, `tbt81x.cat`, `tbt81x.sys`,
   and `tbtcoinx.dll`. Its INF
   declares Intel version **16.1.47.2**, dated 2016-04-18. App has no equivalent
   `Drivers\Thunderbolt` directory. Neither Setup image has that package
   staged under a `tbt81x` driver-store path. Its presence in the manual
   installed image makes it eligible for the post-install pnputil command;
   this is not evidence it fixes an early WinPE display failure.
4. **Old graphics payload intentionally omitted from app Drivers.** Manual
   includes 359 `Drivers\IntelHDGraphics64` files; app omits them. This does
   not mean app lacks graphics: the matching newer Intel package is staged
   in both images' driver stores.
5. **GPU device instance lookup.** Manual's script hardcodes one PCI instance
   string. App calls its additional `a1706gpu.exe` to find a currently present
   device matching `PCI\VEN_8086&DEV_1927&SUBSYS_015D106B`, then restarts that
   instance. The other three custom helper executables also have different
   bytes; binary equivalence was not established.
6. **Direct kernel copy.** App Setup additionally has
   `Windows\System32\drivers\igdkmd64.sys`, matching the driver-store kernel.
   Manual's WIM listing only contains its driver-store copy, despite its
   service pointing to the same direct path.
7. **Fallback startup.** Manual `startnet.cmd` calls the A1706 script if it has
   not run; app `startnet.cmd` only runs wpeinit. The primary winpeshl launch
   exists in both. Manual uses CRLF and app uses LF in winpeshl.ini; their
   entries match.
8. **Instructions snapshot.** The app ISO still contains the older README
   with "press w" wording. Later repository completion-guide edits do not
   retroactively update this existing ISO.

## Method and limits

Read ISO contents from read-only Windows mounts. Used 7-Zip WIM listings with
content SHA-1 values, selective extraction of scripts, INF files and SYSTEM
hives, WIM XML metadata, SHA-256 hashing outside WIMs, parsed XML comparison,
and offline registry parsing. Cross-checked relevant registry results with
python-registry. Evidence and extracted comparison copies are under
`.cache/iso-comparison` (not committed).

Native DISM information commands required elevation and native hive opening
failed on this host. The offline parsers provided the metadata and registry
values without mounting or servicing any WIM. This is not evidence that the
ISOs' hives are corrupt.

No Mac hardware boot, Windows installation, exhaustive compressed-resource
integrity test, or custom executable behavioral equivalence test was performed.
Neither source ISO nor the app's patching behavior was changed. A controlled
hardware test using the same Windows source revision is needed to attribute a
boot/display failure to any of these differences.

## Follow-up: compatibility recommendation

The user confirms that the manual ISO works. It remains the hardware-verified
baseline; the app ISO has not earned the same status from this inspection.

Reading the manual project's `REPRODUCE.md`, especially its boot-path allow
list and "Do not repeat these" section, resolves two apparent omissions:

- **Thunderbolt is an incomplete leftover, not a proven missing fix.** The
  extracted manual `tbt81x.inf` references `setup.msi` in SourceDisksFiles and
  its copy section. That file is absent from the four-file package. The
  reproduction notes record import error `0x80070002`, explicitly exclude it
  from WinPE, and say it remained uninstalled on the working system. Do not
  add this incomplete package to improve HDMI compatibility. A future complete
  Thunderbolt package would require a separate device/function test.
- **Test-signing is an inherited experiment.** The notes explicitly say the
  iterated ISO already had it enabled, to preserve an existing BCD, and that a
  fresh stock ISO using the signed catalog does not need disabled signature
  checks. All four inspected images contain the same Intel catalog (SHA-1
  `89df517cda83a04fa664eaab4062e8e0af9ea3c2`). Its presence, matching INF/kernel,
  and the notes support preserving the source BCD rather than enabling bypasses
  by default. Whether those flags affect this exact Mac is still an A/B-test
  question, not established by their mere presence in a working ISO.

For portability across **2016 A1706 machines with Iris 550**, keep dynamic device
instance lookup. The manual guide explicitly recommends finding the device
whose ID contains `PCI\VEN_8086&DEV_1927&SUBSYS_015D106B` when the patcher knows
the hardware ID rather than one particular machine's instance ID. The app uses
the documented [present-device enumeration API](https://learn.microsoft.com/en-us/windows/win32/api/cfgmgr32/nf-cfgmgr32-cm_get_device_id_lista)
and then the same [pnputil restart operation](https://learn.microsoft.com/en-us/windows-hardware/drivers/devtest/pnputil-command-syntax).
Both inspected Windows sources support that command.

The present lookup implementation needs more explicit failure handling before
calling it robust: enumeration failure/no match emits no ID, yet the batch file
still calls `pnputil /restart-device ""`. It also takes the first match and has
no retry when the device list changes between sizing and reading it. Recommended
hardening: bounded enumeration retry, explicit zero/ambiguous-match diagnostics,
log the resolved instance, and restart only a validated matching instance once.
Do not fall back to the original machine's hardcoded instance on other Macs.

The source for the other three C helpers (`a1706iris`, `a1706dlg`, `a1706cue`)
matches the manual staging source byte-for-byte. Their rebuilt executable bytes
alone are not evidence of changed logic. The Iris readiness parser still checks
English "Iris" and "Started" text; the app currently restricts input to en-US.
Supporting more languages would need a structured device-status check.

Recommended order: reproduce the working v17 sequence with the same original
Windows source revision (3803), Intel 21.20.16.5174, and existing boot driver
allow list; retain and harden dynamic instance resolution; then hardware-test
that controlled output before expanding accepted source revisions or models.
The manual remains the practical fallback until this succeeds. No patching
code, ISO, or build has been changed for this investigation.
