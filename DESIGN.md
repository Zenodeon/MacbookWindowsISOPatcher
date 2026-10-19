# Desktop design

Operate mode: a person preparing installation media at a Windows desktop.
Light and dark palettes with a blue action color. Follow the OS by default;
allow explicit Light or Dark and remember the choice in user settings.
Dark mode uses a near-black canvas and subdued controls. Separate tasks with
blue-tinted source/edition sections, violet-tinted Mac/setup sections, and a
neutral output/details section; use the corresponding pale tints in light mode.
The permanent left rail explains the four sequential steps. The main work area
uses wide aligned controls and a scrollable edition table, with contextual
validation and a persistent bottom action row. No decorative imagery or animation.
The first screen offers a large clickable ISO drop zone. Dropping one local ISO
anywhere while idle starts its scan; busy jobs reject replacement drops.
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
Dismount requires a confirmation modal and returns to the source page on success.
Status uses explicit text as well as color. Unknown progress uses a spinner;
download progress is derived from received bytes. No simulated build percentages.

The donation step is a page in the workflow, with both Donate and Continue without
donating visible. Payment never changes the available functionality.
The sidebar always provides the theme selector, by zenodeon website attribution,
Ko-fi button, and GitHub repository button. It scrolls on short windows.
