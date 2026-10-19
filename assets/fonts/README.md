# Embedded website fonts

Font roles verified against https://zenodeon.com/Portfolio/main.css on 2026-10-19:

- Atomed: display and creator wordmark, downloaded from
  https://zenodeon.com/Assets/Font/atomed-x31w8.otf at the site owner's request.
- Exo 2: body, controls, and headings. Google Fonts upstream:
  https://github.com/google/fonts/tree/main/ofl/exo2 (SIL Open Font License).
- Space Mono: diagnostic logs. Google Fonts upstream:
  https://github.com/google/fonts/tree/main/ofl/spacemono (SIL Open Font License).

These files are embedded in the Rust executable. The app does not fetch fonts at
runtime. The Google Fonts license texts are included alongside the fonts.
