//! Choosing a file, inside the window.
//!
//! Cellarium is one window that runs its own simulation, and this keeps the
//! choice of file in the same place. It also means the dialog can show the
//! things this application knows and a system picker does not: which
//! experiments were opened recently, and what the file about to be written is
//! going to be called.

use std::path::{Path, PathBuf};

use eframe::egui::{self, RichText};

use crate::gui::theme;

/// The extension every experiment file carries.
pub const EXTENSION: &str = "ron";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileDialogKind {
    Open,
    Save,
}

impl FileDialogKind {
    fn title(self) -> &'static str {
        match self {
            FileDialogKind::Open => "Open experiment",
            FileDialogKind::Save => "Save experiment as",
        }
    }

    fn confirm(self) -> &'static str {
        match self {
            FileDialogKind::Open => "Open",
            FileDialogKind::Save => "Save",
        }
    }
}

/// What the dialog decided this frame.
#[derive(Clone, Debug, PartialEq)]
pub enum FileDialogOutcome {
    /// Still open, waiting for the user.
    Pending,
    Cancelled,
    Chosen(PathBuf),
}

/// An open file dialog and where it is looking.
pub struct FileDialog {
    kind: FileDialogKind,
    directory: PathBuf,
    file_name: String,
    /// Why the last attempt did not work, shown in place rather than as a
    /// message somewhere else after the dialog has closed.
    error: Option<String>,
    /// Whether the name field has been given focus yet.
    focused: bool,
}

impl FileDialog {
    pub fn new(kind: FileDialogKind, start: Option<&Path>, suggested: &str) -> Self {
        let (directory, file_name) = match start {
            Some(path) if path.is_dir() => (path.to_path_buf(), suggested.to_string()),
            Some(path) => (
                path.parent().map(Path::to_path_buf).unwrap_or_else(cwd),
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| suggested.to_string()),
            ),
            None => (cwd(), suggested.to_string()),
        };
        Self {
            kind,
            directory,
            file_name,
            error: None,
            focused: false,
        }
    }

    pub fn kind(&self) -> FileDialogKind {
        self.kind
    }

    /// Draw the dialog and report what the user did.
    pub fn show(&mut self, ctx: &egui::Context, recent: &[PathBuf]) -> FileDialogOutcome {
        let mut outcome = FileDialogOutcome::Pending;
        egui::Modal::new(egui::Id::new("file_dialog")).show(ctx, |ui| {
            ui.set_width(560.0);
            ui.label(RichText::new(self.kind.title()).strong());
            ui.label(
                RichText::new(match self.kind {
                    FileDialogKind::Open => "Pick an experiment to open.",
                    FileDialogKind::Save => {
                        "Choose a folder and a name. The file keeps the .ron extension."
                    }
                })
                .weak(),
            );
            ui.separator();

            if !recent.is_empty() {
                ui.label(RichText::new("Recent").weak());
                for path in recent.iter().take(5) {
                    let label = path.to_string_lossy().into_owned();
                    if ui
                        .add(
                            egui::Button::new(RichText::new(shorten(&label)).monospace())
                                .frame(false),
                        )
                        .on_hover_text(&label)
                        .clicked()
                    {
                        self.directory = path.parent().map(Path::to_path_buf).unwrap_or_else(cwd);
                        self.file_name = path
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_default();
                    }
                }
                ui.separator();
            }

            ui.horizontal(|ui| {
                if ui
                    .add_enabled(self.directory.parent().is_some(), egui::Button::new("Up"))
                    .on_hover_text("Go to the folder above")
                    .on_disabled_hover_text("This is the top of the filesystem")
                    .clicked()
                    && let Some(parent) = self.directory.parent()
                {
                    self.directory = parent.to_path_buf();
                }
                ui.add(
                    egui::Label::new(
                        RichText::new(self.directory.to_string_lossy().into_owned()).monospace(),
                    )
                    .truncate(),
                );
            });

            let entries = read_directory(&self.directory);
            egui::ScrollArea::vertical()
                .max_height(220.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if entries.is_empty() {
                        ui.label(RichText::new("this folder has no experiments in it").weak());
                    }
                    for entry in &entries {
                        let name = entry
                            .path
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        let text = if entry.is_dir {
                            RichText::new(format!("{name}/")).monospace()
                        } else {
                            RichText::new(name.clone()).monospace()
                        };
                        let selected = !entry.is_dir && name == self.file_name;
                        if ui.add(egui::Button::selectable(selected, text)).clicked() {
                            if entry.is_dir {
                                self.directory = entry.path.clone();
                            } else {
                                self.file_name = name;
                                self.error = None;
                            }
                        }
                    }
                });

            ui.separator();
            ui.horizontal(|ui| {
                ui.label("File");
                let field = ui.add(
                    egui::TextEdit::singleline(&mut self.file_name)
                        .desired_width(420.0)
                        .hint_text("my-experiment.ron, or a full path"),
                );
                // The name is what the user came here to type, so the caret
                // starts in it. Having to click the field first is a step that
                // exists for no reason.
                if !self.focused {
                    self.focused = true;
                    field.request_focus();
                }
            });
            if let Some(error) = &self.error {
                ui.label(RichText::new(error).color(theme::state_color(theme::State::Invalid)));
            }

            ui.horizontal(|ui| {
                let ready = !self.file_name.trim().is_empty();
                if ui
                    .add_enabled(ready, egui::Button::new(self.kind.confirm()))
                    .on_disabled_hover_text("Type a file name first")
                    .clicked()
                {
                    outcome = self.confirm();
                }
                if ui.button("Cancel").clicked() {
                    outcome = FileDialogOutcome::Cancelled;
                }
                // A dialog that ignores Enter and Escape is a dialog the user
                // has to reach for the mouse to answer.
                if ui.input(|input| input.key_pressed(egui::Key::Enter)) && ready {
                    outcome = self.confirm();
                }
                if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                    outcome = FileDialogOutcome::Cancelled;
                }
            });
        });
        outcome
    }

    fn confirm(&mut self) -> FileDialogOutcome {
        // A typed path is honoured as a path. Someone who knows where the file
        // goes should not have to click their way there one folder at a time,
        // and a name with no separator still means "here", as it reads.
        let typed = self.file_name.trim();
        let named = Path::new(typed);
        let path = if named.is_absolute() {
            PathBuf::from(with_extension(typed))
        } else {
            // A relative path, with or without separators, is relative to the
            // folder on screen.
            self.directory.join(with_extension(typed))
        };
        if self.kind == FileDialogKind::Open && !path.is_file() {
            self.error = Some(format!(
                "there is no file called {} in this folder",
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default()
            ));
            return FileDialogOutcome::Pending;
        }
        FileDialogOutcome::Chosen(path)
    }
}

/// Append the extension unless the user already typed one.
pub fn with_extension(name: &str) -> String {
    if Path::new(name)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case(EXTENSION))
    {
        name.to_string()
    } else {
        format!("{name}.{EXTENSION}")
    }
}

struct Entry {
    path: PathBuf,
    is_dir: bool,
}

/// Folders and experiment files, folders first, each group sorted by name.
///
/// Only `.ron` files are listed: an experiment picker offering every file on
/// the disk makes the user do the filtering.
fn read_directory(directory: &Path) -> Vec<Entry> {
    let Ok(reader) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut entries: Vec<Entry> = reader
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            let is_dir = path.is_dir();
            let name = path.file_name()?.to_string_lossy().into_owned();
            // Hidden entries stay hidden; a dot-directory is not what someone
            // looking for their experiment is looking for.
            if name.starts_with('.') {
                return None;
            }
            let keep = is_dir
                || path
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case(EXTENSION));
            keep.then_some(Entry { path, is_dir })
        })
        .collect();
    entries.sort_by(|left, right| {
        right
            .is_dir
            .cmp(&left.is_dir)
            .then_with(|| left.path.file_name().cmp(&right.path.file_name()))
    });
    entries
}

fn cwd() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"))
}

/// Keep a long path readable by dropping the middle rather than the end: the
/// file name is the part the user is scanning for.
fn shorten(path: &str) -> String {
    const LIMIT: usize = 58;
    let characters: Vec<char> = path.chars().collect();
    if characters.len() <= LIMIT {
        return path.to_string();
    }
    let tail: String = characters[characters.len() - (LIMIT - 4)..]
        .iter()
        .collect();
    format!(".../{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_extension_is_added_once_and_only_when_missing() {
        assert_eq!(with_extension("world"), "world.ron");
        assert_eq!(with_extension("world.ron"), "world.ron");
        assert_eq!(with_extension("world.RON"), "world.RON");
        assert_eq!(with_extension("a.b"), "a.b.ron");
    }

    #[test]
    fn a_long_path_keeps_its_file_name_visible() {
        let path = format!("/{}/experiment.ron", "very-long-directory".repeat(6));
        let shortened = shorten(&path);
        assert!(shortened.len() < path.len());
        assert!(
            shortened.ends_with("experiment.ron"),
            "the name is what identifies the file: {shortened}"
        );
    }

    #[test]
    fn a_save_dialog_opens_on_the_current_files_folder_and_name() {
        let dialog = FileDialog::new(
            FileDialogKind::Save,
            Some(Path::new("/tmp/studies/orbium.ron")),
            "untitled.ron",
        );
        assert_eq!(dialog.directory, PathBuf::from("/tmp/studies"));
        assert_eq!(dialog.file_name, "orbium.ron");
    }

    #[test]
    fn a_typed_absolute_path_is_used_as_written() {
        let mut dialog = FileDialog::new(FileDialogKind::Save, None, "untitled.ron");
        dialog.file_name = "/tmp/elsewhere/study".to_string();
        let outcome = dialog.confirm();
        assert_eq!(
            outcome,
            FileDialogOutcome::Chosen(PathBuf::from("/tmp/elsewhere/study.ron")),
            "a user who types where the file goes should not be overridden"
        );
    }

    #[test]
    fn a_bare_name_lands_in_the_folder_on_screen() {
        let mut dialog = FileDialog::new(
            FileDialogKind::Save,
            Some(Path::new("/tmp/studies/orbium.ron")),
            "untitled.ron",
        );
        dialog.file_name = "second".to_string();
        assert_eq!(
            dialog.confirm(),
            FileDialogOutcome::Chosen(PathBuf::from("/tmp/studies/second.ron"))
        );
    }

    #[test]
    fn a_dialog_with_no_starting_path_suggests_a_name() {
        let dialog = FileDialog::new(FileDialogKind::Save, None, "untitled.ron");
        assert_eq!(dialog.file_name, "untitled.ron");
    }
}
