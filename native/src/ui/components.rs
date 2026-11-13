use eframe::egui::{self, Color32, RichText};

use super::Action;
use crate::git::{DiffPreview, FileChange, LineKind, Mutation, Side};
use crate::state::AppState;

pub fn file_list(ui: &mut egui::Ui, state: &AppState, actions: &mut Vec<Action>) {
    let Some(snapshot) = &state.snapshot else {
        ui.label("Open a repository to see changes.");
        return;
    };
    if snapshot.files.is_empty() {
        ui.label("Working tree clean");
        return;
    }
    let height = ui.text_style_height(&egui::TextStyle::Body) + 8.0;
    egui::ScrollArea::vertical().id_salt("files").show_rows(
        ui,
        height,
        snapshot.files.len(),
        |ui, rows| {
            for row in rows {
                let file = &snapshot.files[row];
                let selected = state.selected.as_ref() == Some(file);
                let side = if file.side == Side::Staged {
                    "INDEX"
                } else {
                    "WORKTREE"
                };
                let text = format!("{side} · {} · {}", file.label, file.path.display());
                if ui
                    .selectable_label(selected, text)
                    .on_hover_text(file.path.display().to_string())
                    .clicked()
                {
                    actions.push(Action::SelectFile(file.clone()));
                }
            }
        },
    );
}

pub fn selected_file_actions(
    ui: &mut egui::Ui,
    file: Option<&FileChange>,
    enabled: bool,
    actions: &mut Vec<Action>,
) {
    if let Some(file) = file {
        let label = if file.side == Side::Staged {
            "Unstage file"
        } else {
            "Stage file"
        };
        if ui
            .add_enabled(enabled && !file.conflicted, egui::Button::new(label))
            .clicked()
        {
            actions.push(Action::Mutate(if file.side == Side::Staged {
                Mutation::Unstage(file.clone())
            } else {
                Mutation::Stage(file.clone())
            }));
        }
    }
}

pub fn diff_view(ui: &mut egui::Ui, preview: Option<&DiffPreview>) {
    let Some(preview) = preview else {
        ui.centered_and_justified(|ui| {
            ui.label("Select a changed file or commit to inspect its diff.");
        });
        return;
    };
    if let Some(notice) = &preview.notice {
        ui.label(RichText::new(notice).color(Color32::from_rgb(210, 160, 65)));
        ui.separator();
    }
    let row_height = ui.text_style_height(&egui::TextStyle::Monospace) + 4.0;
    egui::ScrollArea::both()
        .id_salt("diff")
        .auto_shrink([false, false])
        .show_rows(ui, row_height, preview.lines.len(), |ui, rows| {
            for row in rows {
                let line = &preview.lines[row];
                let old = line.old_number.map(|n| n.to_string()).unwrap_or_default();
                let new = line.new_number.map(|n| n.to_string()).unwrap_or_default();
                let (prefix, color, background) = match line.kind {
                    LineKind::Addition => (
                        "+",
                        Color32::from_rgb(110, 220, 140),
                        Color32::from_rgb(19, 49, 31),
                    ),
                    LineKind::Deletion => (
                        "-",
                        Color32::from_rgb(255, 145, 145),
                        Color32::from_rgb(62, 27, 33),
                    ),
                    LineKind::Header => {
                        (" ", Color32::from_rgb(120, 185, 250), Color32::TRANSPARENT)
                    }
                    LineKind::Context => (
                        " ",
                        if ui.visuals().dark_mode {
                            Color32::LIGHT_GRAY
                        } else {
                            Color32::DARK_GRAY
                        },
                        Color32::TRANSPARENT,
                    ),
                };
                let text = String::from_utf8_lossy(&preview.buffer[line.bytes.clone()]);
                // One widget per visible row; unwrapped rows keep virtualization heights stable.
                let content = format!(
                    "{old:>6} {new:>6} {prefix} {}",
                    text.trim_end_matches(['\n', '\r']).replace('\t', "    ")
                );
                ui.add(
                    egui::Label::new(
                        RichText::new(content)
                            .monospace()
                            .color(color)
                            .background_color(background),
                    )
                    .wrap_mode(egui::TextWrapMode::Extend),
                );
            }
        });
}
