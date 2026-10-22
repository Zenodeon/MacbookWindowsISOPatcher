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
