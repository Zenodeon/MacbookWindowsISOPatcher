use super::*;

pub(super) struct LogViewResponse {
    pub save_requested: bool,
    #[cfg(test)]
    pub expanded: bool,
    #[cfg(test)]
    pub heading: egui::Rect,
    #[cfg(test)]
    pub save: Option<egui::Rect>,
}

pub(super) fn build_log_view(
    ui: &mut egui::Ui,
    logs: &[String],
    default_open: bool,
) -> LogViewResponse {
    let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(
        ui.ctx(),
        ui.make_persistent_id("build-log-expanded"),
        default_open,
    );
    let mut save_requested = false;
    #[cfg(test)]
    let mut heading = egui::Rect::NOTHING;
    #[cfg(test)]
    let mut save = None;
    ui.horizontal(|ui| {
        state.show_toggle_button(ui, egui::collapsing_header::paint_default_icon);
        let response = ui.add(egui::Label::new("Show build log").sense(egui::Sense::click()));
        #[cfg(test)]
        {
            heading = response.rect;
        }
        if response.clicked() {
            state.toggle(ui);
        }
        if state.is_open() {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let response = ui.button("Save visible log…");
                save_requested = response.clicked();
                #[cfg(test)]
                {
                    save = Some(response.rect);
                }
            });
        }
    });
    state.show_body_unindented(ui, |ui| {
        let height = (ui.clip_rect().bottom() - ui.cursor().top() - 6.0).clamp(96.0, 320.0);
        egui::ScrollArea::vertical()
            .id_salt("log")
            .max_height(height)
            .stick_to_bottom(true)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for line in logs {
                    ui.monospace(line);
                }
            });
    });
    LogViewResponse {
        save_requested,
        #[cfg(test)]
        expanded: state.is_open(),
        #[cfg(test)]
        heading,
        #[cfg(test)]
        save,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn render(ctx: &egui::Context, events: Vec<egui::Event>, width: f32) -> LogViewResponse {
        let mut result = None;
        let _=ctx.run(egui::RawInput{screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(width,400.0))),events,..Default::default()},|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{result=Some(build_log_view(ui,&["A long message that wraps without changing the saved text.\nSecond line".into()],true));});
        });
        result.unwrap()
    }
    fn click(ctx: &egui::Context, pos: egui::Pos2, width: f32) -> LogViewResponse {
        render(
            ctx,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
            width,
        );
        render(
            ctx,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
            width,
        )
    }
    #[test]
    fn save_shares_header_row_and_does_not_toggle_log() {
        for width in [900.0, 400.0] {
            let ctx = egui::Context::default();
            let initial = render(&ctx, vec![], width);
            let save = initial.save.unwrap();
            assert!(save.left() > initial.heading.right());
            assert!((save.center().y - initial.heading.center().y).abs() < 2.0);
            let clicked = click(&ctx, save.center(), width);
            assert!(clicked.save_requested && clicked.expanded);
            let closed = click(&ctx, clicked.heading.center(), width);
            assert!(!closed.expanded && closed.save.is_none());
        }
    }
}
