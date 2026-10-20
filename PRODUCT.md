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
All listed models can build once their driver downloads finish. Other models
remain Unverified and use their own Apple Boot Camp packages. Invite users to
report both success and problems through GitHub issues beside the Build button.
Include compatible Intel graphics releases from early 2021 for the other models.
Select graphics drivers automatically for the model's external output: Intel on
13-inch models, Apple's Radeon Pro drivers on the 15-inch model. No GPU toggle.
List graphics on every model card, including both GPUs on the dual-GPU model.
No automatic disk partitioning and no USB-writing functionality.

## Evidence
`C:/Github/MacbookISOBuildingSpace/REPRODUCE.md` and the v17 runtime payloads.
The source records a successful Pro installation on the target Mac; this app's
own hardware validation remains pending. Original media and output ISOs are not in Git.
