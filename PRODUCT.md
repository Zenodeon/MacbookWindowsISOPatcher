# MacBook ISO Patcher

## Platform and stack
Windows-first desktop app. Rust with egui/eframe; no JavaScript or TypeScript.
The platform backend boundary allows future macOS/Linux implementations.

## Users and purpose
People installing Windows on an Intel MacBook, beginning with the 2016 A1706
(MacBookPro13,2, Iris 550). Select Windows media and editions; create a patched
installer using the documented external HDMI fix when the internal panel is disconnected.

## Confirmed behavior
Scan multi-edition Windows media. One or several editions may be included.
Multi-edition builds show an optional donation step linking to https://ko-fi.com/zenodeon.
Continuing without donating always works. There is no license or payment verification.
Download and cache official, pinned driver packages. Windows servicing requires elevation.
Only Pro was validated by the source project; other compatible editions are unverified.
No automatic disk partitioning and no USB-writing functionality.

## Evidence
`C:/Github/MacbookISOBuildingSpace/REPRODUCE.md` and the v17 runtime payloads.
The source records a successful Pro installation on the target Mac; this app's
own hardware validation remains pending. Original media and output ISOs are not in Git.
