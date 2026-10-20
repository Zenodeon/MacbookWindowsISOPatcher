use eframe::egui::{
    self, Color32, FontData, FontDefinitions, FontFamily, FontId, TextStyle, Theme, ThemePreference,
};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}
impl ThemeMode {
    pub fn apply(self, ctx: &egui::Context) {
        ctx.set_theme(match self {
            Self::System => ThemePreference::System,
            Self::Light => ThemePreference::Light,
            Self::Dark => ThemePreference::Dark,
        });
        ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(match self {
            Self::System => egui::SystemTheme::SystemDefault,
            Self::Light => egui::SystemTheme::Light,
            Self::Dark => egui::SystemTheme::Dark,
        }));
    }
}
#[derive(Clone, Copy)]
pub struct Palette {
    pub canvas: Color32,
    pub rail: Color32,
    pub footer: Color32,
    pub muted: Color32,
    pub accent: Color32,
    pub primary: Color32,
    pub error: Color32,
    pub source: Color32,
    pub model: Color32,
    pub surface: Color32,
    pub drop: Color32,
    pub missing: Color32,
}
pub fn palette(ctx: &egui::Context) -> Palette {
    if ctx.style().visuals.dark_mode {
        Palette {
            canvas: Color32::from_rgb(25, 29, 37),
            rail: Color32::from_rgb(32, 37, 46),
            footer: Color32::from_rgb(32, 37, 46),
            muted: Color32::from_rgb(174, 185, 203),
            accent: Color32::from_rgb(148, 190, 255),
            primary: Color32::from_rgb(36, 85, 156),
            error: Color32::from_rgb(255, 154, 157),
            source: Color32::from_rgb(35, 47, 63),
            model: Color32::from_rgb(47, 40, 59),
            surface: Color32::from_rgb(37, 42, 51),
            drop: Color32::from_rgb(30, 40, 54),
            missing: Color32::from_rgb(66, 38, 44),
        }
    } else {
        Palette {
            canvas: Color32::from_rgb(248, 249, 251),
            rail: Color32::from_rgb(234, 238, 244),
            footer: Color32::WHITE,
            muted: Color32::from_rgb(78, 88, 103),
            accent: Color32::from_rgb(24, 76, 145),
            primary: Color32::from_rgb(24, 76, 145),
            error: Color32::from_rgb(155, 33, 33),
            source: Color32::from_rgb(230, 239, 251),
            model: Color32::from_rgb(239, 234, 247),
            surface: Color32::WHITE,
            drop: Color32::from_rgb(244, 248, 255),
            missing: Color32::from_rgb(255, 233, 234),
        }
    }
}
pub fn display_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("Atomed".into()))
}
pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    for (name, bytes) in [
        (
            "Atomed",
            include_bytes!("../assets/fonts/Atomed.otf").as_slice(),
        ),
        (
            "Exo 2",
            include_bytes!("../assets/fonts/Exo2.ttf").as_slice(),
        ),
        (
            "Space Mono",
            include_bytes!("../assets/fonts/SpaceMono-Regular.ttf").as_slice(),
        ),
    ] {
        fonts
            .font_data
            .insert(name.into(), FontData::from_static(bytes).into());
    }
    fonts
        .families
        .get_mut(&FontFamily::Proportional)
        .unwrap()
        .insert(0, "Exo 2".into());
    fonts
        .families
        .get_mut(&FontFamily::Monospace)
        .unwrap()
        .insert(0, "Space Mono".into());
    fonts.families.insert(
        FontFamily::Name("Atomed".into()),
        vec!["Atomed".into(), "Exo 2".into()],
    );
    ctx.set_fonts(fonts);
    for theme in [Theme::Light, Theme::Dark] {
        let mut style = egui::Style {
            visuals: match theme {
                Theme::Light => egui::Visuals::light(),
                Theme::Dark => egui::Visuals::dark(),
            },
            ..Default::default()
        };
        style.visuals.override_text_color = Some(if theme == Theme::Dark {
            Color32::from_rgb(233, 238, 246)
        } else {
            Color32::from_rgb(29, 38, 51)
        });
        style.visuals.hyperlink_color = if theme == Theme::Dark {
            Color32::from_rgb(148, 190, 255)
        } else {
            Color32::from_rgb(24, 76, 145)
        };
        style.visuals.selection.bg_fill = Color32::from_rgb(36, 85, 156);
        if theme == Theme::Dark {
            style.visuals.extreme_bg_color = Color32::from_rgb(22, 27, 35);
            style.visuals.window_fill = Color32::from_rgb(42, 48, 59);
            style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(52, 59, 71);
            style.visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(52, 59, 71);
            style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(63, 73, 89);
            style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(63, 73, 89);
            style.visuals.widgets.active.bg_fill = Color32::from_rgb(71, 88, 113);
            style.visuals.widgets.active.weak_bg_fill = Color32::from_rgb(71, 88, 113);
        }
        style.spacing.item_spacing = egui::vec2(10.0, 10.0);
        style.spacing.button_padding = egui::vec2(14.0, 9.0);
        style.spacing.interact_size.y = 34.0;
        style
            .text_styles
            .insert(TextStyle::Body, FontId::proportional(15.0));
        style
            .text_styles
            .insert(TextStyle::Button, FontId::proportional(15.0));
        style
            .text_styles
            .insert(TextStyle::Heading, FontId::proportional(26.0));
        ctx.set_style_of(theme, style);
    }
}
