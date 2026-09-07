//! Typed error presentation. Never infer permissions from translated strings.
use i18n_embed_fl::fl;

use crate::{App, AppError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Recovery {
    SelectProcess,
    NewSearch,
    EditValue,
    None,
}

pub(crate) fn presentation(error: &AppError) -> (String, String, Recovery) {
    let (title, hint, action) = match error {
        AppError::ProcessExited { .. } | AppError::CurrentProcessMissing | AppError::CurrentPidUnavailable => (
            fl!(crate::LANGUAGE_LOADER, "error-process-title"),
            fl!(crate::LANGUAGE_LOADER, "error-process-help"),
            Recovery::SelectProcess,
        ),
        AppError::AccessDenied { .. } => (
            fl!(crate::LANGUAGE_LOADER, "error-access-title"),
            fl!(crate::LANGUAGE_LOADER, "error-access-help"),
            Recovery::SelectProcess,
        ),
        AppError::SearchReadFailed | AppError::ProcessMapRead { .. } | AppError::AttachDiagnostic { .. } => (
            fl!(crate::LANGUAGE_LOADER, "error-read-title"),
            fl!(crate::LANGUAGE_LOADER, "error-read-help"),
            Recovery::SelectProcess,
        ),
        AppError::SearchValueParse { .. } | AppError::InvalidValue { .. } => (
            fl!(crate::LANGUAGE_LOADER, "error-input-title"),
            fl!(crate::LANGUAGE_LOADER, "error-input-help"),
            Recovery::EditValue,
        ),
        AppError::MemoryWrite { .. } | AppError::InvalidAddress { .. } => (
            fl!(crate::LANGUAGE_LOADER, "error-address-title"),
            fl!(crate::LANGUAGE_LOADER, "error-address-help"),
            Recovery::NewSearch,
        ),
        _ => (
            fl!(crate::LANGUAGE_LOADER, "error-other-title"),
            fl!(crate::LANGUAGE_LOADER, "error-other-help"),
            Recovery::None,
        ),
    };
    let mut details = error.to_string();
    if matches!(
        error,
        AppError::AccessDenied { .. } | AppError::AttachDiagnostic { .. } | AppError::ProcessMapRead { .. } | AppError::SearchReadFailed
    ) {
        details.push('\n');
        details.push_str(&platform_help());
    }
    (format!("{title} {hint}"), details, action)
}

fn platform_help() -> String {
    if cfg!(target_os = "linux") {
        fl!(crate::LANGUAGE_LOADER, "error-access-linux")
    } else if cfg!(target_os = "macos") {
        fl!(crate::LANGUAGE_LOADER, "error-access-macos")
    } else if cfg!(target_os = "windows") {
        fl!(crate::LANGUAGE_LOADER, "error-access-windows")
    } else {
        fl!(crate::LANGUAGE_LOADER, "error-access-help")
    }
}

pub(crate) fn show(app: &mut App, ui: &mut egui::Ui) {
    let Some(error) = app.state.current_error() else { return };
    let (summary, details, action) = presentation(error);
    let mut dismiss = false;
    let mut recover = false;
    egui::Panel::top("in_process_error")
        .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(20, 6)))
        .show(ui, |ui| {
            egui::Frame::new()
                .fill(egui::Color32::from_rgba_unmultiplied(160, 50, 50, 30))
                .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(160, 70, 70)))
                .corner_radius(egui::CornerRadius::same(8))
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    dismiss = super::notice::show(ui, "error_notice", &summary, &details).dismissed;
                    let label = match action {
                        Recovery::SelectProcess => Some(fl!(crate::LANGUAGE_LOADER, "error-select-process")),
                        Recovery::NewSearch => Some(fl!(crate::LANGUAGE_LOADER, "error-new-search")),
                        Recovery::EditValue => Some(fl!(crate::LANGUAGE_LOADER, "error-edit-value")),
                        Recovery::None => None,
                    };
                    if let Some(label) = label {
                        recover = ui.button(label).clicked();
                    }
                });
        });
    if dismiss {
        app.state.dismiss_error();
    } else if recover {
        match action {
            Recovery::SelectProcess => app.attach_action(),
            Recovery::NewSearch => {
                app.state.dismiss_error();
                app.new_search();
            }
            Recovery::EditValue => {
                app.state.dismiss_error();
                if app.editing_result.is_some() {
                    app.result_edit_request_focus = true;
                } else {
                    app.search_value_request_focus = true;
                }
            }
            Recovery::None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_are_typed_and_preserve_technical_details() {
        for (error, recovery, title) in [
            (
                AppError::AccessDenied { source: "OS denied".into() },
                Recovery::SelectProcess,
                "error-access-title",
            ),
            (AppError::SearchReadFailed, Recovery::SelectProcess, "error-read-title"),
            (AppError::ProcessExited { name: "game".into() }, Recovery::SelectProcess, "error-process-title"),
            (
                AppError::MemoryWrite {
                    addr: 123,
                    source: "bad address".into(),
                },
                Recovery::NewSearch,
                "error-address-title",
            ),
            (
                AppError::InvalidValue {
                    value: "x".into(),
                    source: "not numeric".into(),
                },
                Recovery::EditValue,
                "error-input-title",
            ),
        ] {
            let (summary, details, action) = presentation(&error);
            assert_eq!(action, recovery);
            assert!(details.contains(&error.to_string()));
            assert!(!summary.is_empty());
            let _ = title;
        }
        let error = AppError::Generic {
            message: "Permission denied".into(),
        };
        assert_eq!(presentation(&error).2, Recovery::None, "do not guess categories by matching text");
        assert!(AppError::access_error(&std::io::Error::from(std::io::ErrorKind::PermissionDenied)).is_some());
        assert!(AppError::access_error(&std::io::Error::from(std::io::ErrorKind::UnexpectedEof)).is_none());
    }
}
