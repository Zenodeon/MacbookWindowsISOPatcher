# Desktop design

Operate mode: a person preparing installation media at a Windows desktop.
Light and dark palettes with a blue action color. Follow the OS by default;
allow explicit Light or Dark and remember the choice in user settings.
Dark mode uses a charcoal canvas and brighter, clearly separated controls. Separate tasks with
blue-tinted source/edition sections, violet-tinted Mac/setup sections, and a
neutral output/details section; use the corresponding pale tints in light mode.
The permanent left rail explains five steps: model, ISO, editions, review, build. The main work area
uses wide aligned controls and a scrollable edition table, with contextual
validation and a persistent bottom action row. No decorative imagery or animation.
The first screen presents every model together in one list of rounded selectable
rows. The 2016 A1706 has green Hardware Verified status; the others are orange
Unverified. Every model can continue to ISO inspection. Keep the selected model's
identity throughout the flow; do not substitute the A1706 recipe for unimplemented
profiles. Explain their build limitation and disable Build for those selections.
Model names/identifiers follow Apple's identification page:
https://support.apple.com/en-us/108052.
The second screen offers a clickable ISO drop zone. Once a model is selected,
dropping a local ISO while idle starts its scan. Reject replacement drops while
a source session is mounted or an operation is running.
Keep compatibility and build explanations expandable and diagnostic logs collapsed
by default. Prioritize the action and a short description over repeated prose.

Use the fonts from zenodeon.com: Atomed for the app title and creator wordmark,
Exo 2 for body/controls/headings, and Space Mono for diagnostic logs. Embed fonts
for offline use, retaining bundled fallback fonts for unsupported characters.
Use 15px body, 13px metadata, and 26px screen titles. Use 8px control spacing and 24px
between sections. Controls must remain keyboard accessible and resize with the window.
Start at 1240 × 700 logical pixels, fitting that ratio to smaller monitors.
Allow free resizing; below 900px use a narrower rail and tighter content margins.
Keep missing downloads in a red section above Build setup. ADK setup reports its
download/install state there and automatically detects the installed tool.
Keep the tools section visible in green once all required tools are installed.
Place Build setup above Output & workspace. Edition choices are full-width,
rounded toggle rows with an accent border and Selected text for active choices.
Use green Hardware Verified and orange Unverified labels; explain the original
project's hardware evidence in the verified row's tooltip.
Dismount uses a padded, rounded confirmation modal with a separate filename
surface and right-aligned Keep mounted / Dismount ISO actions. Return to the
source page on success.
Show Dismount only after successful scanning, replacing the ordinary back action.
Review has an Edit editions action. Keep the source mounted for the session;
normal closure cleans up app-owned source mounts after scans/builds have stopped.
Preserve mounts that predate the app unless explicitly ejected by the user.
Status uses explicit text as well as color. Unknown progress uses a spinner;
download progress is derived from received bytes. No simulated build percentages.

The donation step is a page in the workflow, with both Donate and Continue without
donating visible. Payment never changes the available functionality.
The sidebar always provides the theme selector, by zenodeon website attribution,
Ko-fi button, and GitHub repository button. It scrolls on short windows.
Center the selected Mac model in the footer gap between the action buttons.
Use its identifier on narrow windows, with the full model name in a tooltip.
Selecting a model only selects it. Pressing Start download & continue to Windows ISO
authorizes its driver downloads and opens ISO selection without blocking scanning.
Changing models revokes that authorization until the button is pressed again.
Show real byte progress, a green cached/ready state, and
an inline retry action on failure. A compact second footer line keeps progress
visible while browsing pages. Cancel and finish the previous job before switching
models or cache locations; ignore late results from an older job. Wait for download
cleanup before normal close. Download availability does not imply build support.
