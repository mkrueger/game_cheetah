use std::time::Duration;

pub(crate) const LIVE_INTERVAL: Duration = Duration::from_millis(33);
pub(crate) const BACKGROUND_INTERVAL: Duration = Duration::from_millis(500);
pub(crate) const IDLE_INTERVAL: Duration = Duration::from_secs(1);
const POLL_INTERVAL: Duration = Duration::from_millis(100);

pub(crate) fn backgrounded(ctx: &egui::Context) -> bool {
    ctx.input(|input| !input.focused || input.viewport().minimized == Some(true))
}

pub(crate) fn interval(ctx: &egui::Context, live_values: bool, busy: bool) -> Duration {
    if backgrounded(ctx) {
        BACKGROUND_INTERVAL
    } else if live_values {
        LIVE_INTERVAL
    } else if busy {
        POLL_INTERVAL
    } else {
        IDLE_INTERVAL
    }
}

pub(crate) fn poll_interval(ctx: &egui::Context) -> Duration {
    interval(ctx, false, true)
}

pub(crate) fn spinner(ui: &mut egui::Ui) {
    if backgrounded(ui.ctx()) {
        // egui's spinner requests immediate repaints while it is visible.
        let size = ui.spacing().interact_size.y;
        ui.allocate_space(egui::vec2(size, size));
    } else {
        ui.spinner();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_rates_preserve_live_values_and_reduce_idle_and_background_wakeups() {
        let ctx = egui::Context::default();
        for (focused, minimized) in [(true, false), (false, false), (true, true)] {
            let mut input = egui::RawInput { focused, ..Default::default() };
            input.viewports.get_mut(&egui::ViewportId::ROOT).unwrap().minimized = Some(minimized);
            let mut output = ctx.run_ui(input, |ui| {
                let background = !focused || minimized;
                assert_eq!(backgrounded(ui.ctx()), background);
                assert_eq!(interval(ui.ctx(), true, false), if background { BACKGROUND_INTERVAL } else { LIVE_INTERVAL });
                assert_eq!(poll_interval(ui.ctx()), if background { BACKGROUND_INTERVAL } else { POLL_INTERVAL });
                assert_eq!(interval(ui.ctx(), false, false), if background { BACKGROUND_INTERVAL } else { IDLE_INTERVAL });
            });
            output.textures_delta.clear();
        }
        assert!(BACKGROUND_INTERVAL.as_millis() >= LIVE_INTERVAL.as_millis() * 15);
        assert!(IDLE_INTERVAL.as_millis() >= LIVE_INTERVAL.as_millis() * 30);
    }

    #[test]
    fn background_spinner_does_not_force_animation_repaints() {
        let ctx = egui::Context::default();
        for _ in 0..3 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    focused: false,
                    ..Default::default()
                },
                |ui| {
                    spinner(ui);
                    ui.ctx().request_repaint_after(BACKGROUND_INTERVAL);
                },
            );
            output.textures_delta.clear();
        }
        let mut output = ctx.run_ui(
            egui::RawInput {
                focused: false,
                ..Default::default()
            },
            |ui| {
                spinner(ui);
                ui.ctx().request_repaint_after(BACKGROUND_INTERVAL);
            },
        );
        assert!(output.viewport_output[&egui::ViewportId::ROOT].repaint_delay >= Duration::from_millis(400));
        output.textures_delta.clear();
    }
}
