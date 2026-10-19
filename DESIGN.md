# Desktop design

Operate mode: a person preparing installation media at a Windows desktop.
Light neutral canvas with dark graphite text and a deep blue action color.
The permanent left rail explains the four sequential steps. The main work area
uses wide aligned controls and a scrollable edition table, with contextual
validation and a persistent bottom action row. No decorative imagery or animation.

Use egui's bundled proportional font: 15px body, 13px metadata, 26px screen titles.
Reserve monospace for paths and diagnostic logs. Use 8px control spacing and 24px
between sections. Controls must remain keyboard accessible and resize with the window.
Status uses explicit text as well as color. Unknown progress uses a spinner;
download progress is derived from received bytes. No simulated build percentages.

The donation step is a page in the workflow, with both Donate and Continue without
donating visible. Payment never changes the available functionality.
