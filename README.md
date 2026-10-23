# MacBook ISO Patcher

Patch your Windows ISO to boot Windows Setup on a **headless MacBook** and send
the display output to an **external screen**, so you can continue installation
without an internal display.

![MacBook ISO Patcher model selection in dark mode](assets/readme/model-selection-dark.png)

## Tested on a headless MacBook

Photos from the working Windows 10 setup on a **headless MacBook Pro 13-inch
A1706 (2016)**, using an external screen. This is the setup the patcher's 2016
A1706 recipe is based on.

| Windows Setup on the external screen | Windows finishing setup |
| --- | --- |
| <img src="assets/readme/headless-a1706-setup-1.jpg" alt="Windows Setup language selection on an external screen above a headless MacBook" width="360"> | <img src="assets/readme/headless-a1706-setup-2.jpg" alt="Windows finishing setup on an external screen above a headless MacBook" width="360"> |

## Supported Macs

The goal is to support **Intel-based Macs first**, then explore support for
**Apple silicon (M-series) Macs**. The only hardware-verified model so
far is a **headless MacBook Pro 13-inch A1706 (2016)**. Support and testing for
other models are still in progress.

| Mac | Identifier | Graphics | Status |
| --- | --- | --- | --- |
| MacBook Pro 13-inch 2015 | Pending | Model-dependent | Coming soon |
| MacBook Pro 15-inch 2015 | Pending | Model-dependent | Coming soon |
| MacBook Pro 13-inch 2016 · A1708 · No Touch Bar | `MacBookPro13,1` | Iris 540 | Ready for testing · Unverified |
| MacBook Pro 13-inch 2016 · A1706 · Touch Bar | `MacBookPro13,2` | Iris 550 | Hardware Verified (headless) |
| MacBook Pro 15-inch 2016 · A1707 · Touch Bar | `MacBookPro13,3` | Intel HD 530 + Radeon Pro | Ready for testing · Unverified |
| MacBook Pro 13-inch 2017 · A1706 · Touch Bar | `MacBookPro14,2` | Iris Plus 650 | Ready for testing · Unverified |
| MacBook Pro 13-inch 2017 · No Touch Bar | Pending | Model-dependent | Coming soon |
| MacBook Pro 15-inch 2017 | Pending | Model-dependent | Coming soon |
| MacBook Pro 2018-2020 · Intel | Pending | Model-dependent | Coming soon |
| MacBook Air · Intel | Pending | Model-dependent | Coming soon |
| MacBook 12-inch · Intel | Pending | Model-dependent | Coming soon |
| Other Intel Macs · iMac, Mac mini and Mac Pro | Pending | Model-dependent | Planned |

The verified result comes from the working 2016 A1706 Windows 10 Pro recipe.
Newly generated ISOs still need testing on the target Mac. Coming-soon and planned
models are a roadmap.

Select the year and identifier carefully: **A1706 includes both 2016 and 2017 models**.
The two A1706 profiles have separate external-display Setup recipes.

## Files you need

- **An original Microsoft Windows 10 ISO**.

The patcher downloads the drivers for your selected model and provides download
or installation actions for missing build tools. You do not need to collect the
Mac drivers yourself.

Run the patcher on a **64-bit Windows 10 or 11 PC**, with internet access and
enough free storage for the build. Your original ISO stays unchanged; the result
is saved as a separate custom ISO.

## Create your Custom MacBook Windows ISO

1. Open the patcher on your Windows PC.
2. Choose your MacBook model, then press **Start download & continue to Windows ISO**
   to start downloading its drivers.
3. Drag your original Windows ISO into the app, or browse for it. Select the
   Windows edition or editions you want to include. **Start with one edition**
   and test it on your Mac first. Once it works, come back and build an ISO with
   multiple editions if you need them.
4. Review the output location and WorkingSpace. Finish any missing-tool
   installation and wait for the model's driver downloads to complete.
5. Press **Build ISO** and approve the administrator prompt. When it finishes,
   use **Open ISO location** to find the file and **Save build logs** to keep a copy.

## Install Windows on the Mac

1. **Prepare a Windows partition** using Boot Camp Assistant or another method.
2. **Set up the USB with [Ventoy](https://www.ventoy.net/)** and copy the generated
   ISO onto it.
3. **Prepare setup logging**, when the completion page offers it. Click
   **Prepare USB for setup logs**, select the USB root and wait for **USB prepared**.
   These logs help troubleshoot failures and improve support for your Mac model.
4. **Boot the USB**, select your ISO in Ventoy, and choose **WIMBOOT**.
   Keep the USB connected during Windows Setup.
5. **Complete Windows Setup** using the partition you prepared.
6. **Install Boot Camp drivers** in Windows: run `C:\BootCamp\Setup.exe` as
   administrator, then restart.
7. **Open Apple Software Update** from Windows search. If it is missing, install
   [Apple's desktop iTunes installer](https://www.apple.com/itunes/download/win64/),
   then check again. Install the applicable updates and restart.
8. **Check the MacBook's features**, including the display, keyboard, trackpad,
   sound and Wi-Fi. [Report anything that still fails](https://github.com/Zenodeon/MacbookWindowsISOPatcher/issues).

The app creates the ISO; partitioning and preparing a bootable Ventoy USB are
separate steps. **Prepare USB for setup logs** adds a logging marker and does not
format the drive or install Ventoy.

## Something did not work?

Use **Copy error** to keep the error message and **Save build logs** to export the
build log.

[Open a GitHub issue](https://github.com/Zenodeon/MacbookWindowsISOPatcher/issues)
with your Mac model/identifier, Windows edition/build, display connection,
what happened and the last visible screen. Attach your build log and, for A1706
Setup failures, the session folder from the prepared USB:

- **2016 A1706:** `MacBookPatcher-Logs\A1706\<session>`
- **2017 A1706:** `MacBookPatcher-Logs\A1706-2017\<session>`

Review logs before sharing; Windows logs can contain
account names and paths. The app does not upload them automatically.

## Build from source

Install Rust stable with the MSVC toolchain, Visual Studio C++ Build Tools and a
Windows SDK. From the repository folder, run:

```powershell
cargo test --locked --all-targets
cargo build --locked --release
.\target\release\macbook-iso-patcher.exe
```

[Driver sources](docs/driver-sources.md)

## Support development

If the project helps you, you can [support it on Ko-fi](https://ko-fi.com/zenodeon).
