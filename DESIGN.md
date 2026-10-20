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
identity throughout the flow. All listed models can build once their selected
driver downloads finish. Other models use their own Apple package without the
A1706-only HDMI fix. Cards share the same name, graphics/details line and status
tag layout. List both GPUs where present, but provide no GPU toggle. Choose the
external-display driver automatically: Intel for the 13-inch models, Apple Radeon
for the 15-inch A1707. Keep Unverified orange and place a download-readiness
note and GitHub issue link beside Build, inviting reports of success or problems.
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
Keep driver bytes, progress, ready/error state and Retry inside the center gap of
that same compact action row, without adding footer rows. Narrow gaps show the
percentage with full byte/package details on hover. Keep the GitHub result-report
link beside the selected-model information on Review.
Do not repeat a driver download section inside the scrolling page. Label the
primary build action Build ISO and place a thin progress bar inside it. Use actual
download bytes when known and an animated indicator for unmeasured build stages.
While disabled, name the current state: Downloading, Getting ready, Waiting for
drivers/setup, or Download failed. Build ISO is the ready action, Building ISO is
an active image build, and ISO ready confirms completion. Use blue progress for
preparation and green for building/completion; show no completed bar on the ready
Build ISO action before it has been pressed.
On the source page, put Selected Mac in a neutral section above the blue Source
file section. Compatibility details belong to Source file. Present ordinary drive
and UNC paths while keeping canonical paths for file operations. Reset the main
scroll offset on every page/donation-screen transition, including return visits.
Show Action needed errors in a centered modal, not an inline page banner. Keep
the full message available through Copy error, with Copied feedback. Download
errors have a compact Copy link next to Retry in the footer. Keep
the original message readable with Close; build-check failures also offer Retry.
Retry reruns validation and preserves whether the user was reviewing the build
or had already continued through the donation screen. Do not start a worker while
the requirements still fail. Keep retry actions attached to their own error so
unrelated messages never inherit a build retry.
Keep build-log entries compact with 2 px vertical gaps, preserving the monospace
font, wrapping, auto-scroll and saved log text. Connection errors include their
phase, elapsed time and a retained connection-log path for troubleshooting.
Use its identifier on narrow windows, with the full model name in a tooltip.
Selecting a model only selects it. Pressing Start download & continue to Windows ISO
authorizes its driver downloads and opens ISO selection without blocking scanning.
Changing models revokes that authorization until the button is pressed again.
Show real byte progress, a green cached/ready state, and
a footer retry action on failure. The footer keeps progress
visible while browsing pages. Cancel and finish the previous job before switching
models or cache locations; ignore late results from an older job. Wait for download
cleanup before normal close. Hardware verification status does not block building.
