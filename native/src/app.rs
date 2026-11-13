use std::path::PathBuf;
use std::time::Instant;

use eframe::egui::{self, RichText};

use crate::command::{Command, Payload};
use crate::error::Error;
use crate::git::{LocalGit, Mutation, Side};
use crate::state::{AppState, Pending};
use crate::ui::{Action, components};
use crate::worker::Worker;

pub struct GitRustApp {
    pub state: AppState,
    worker: Worker,
    path_input: String,
    initial_path: Option<PathBuf>,
    summary: String,
    description: String,
    new_branch: String,
    history_tab: bool,
    diff_title: String,
    startup: Instant,
    first_frame: bool,
    first_snapshot: bool,
    smoke_deadline: Option<Instant>,
}

impl GitRustApp {
    pub fn new(
        ctx: &egui::Context,
        initial_path: Option<PathBuf>,
        preferences: Option<PathBuf>,
        startup: Instant,
        smoke: bool,
    ) -> std::io::Result<Self> {
        ctx.set_visuals(egui::Visuals::dark());
        let repaint = ctx.clone();
        let worker = Worker::spawn(LocalGit, preferences, move || repaint.request_repaint())?;
        let mut app = Self {
            state: AppState::default(),
            worker,
            path_input: initial_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            initial_path,
            summary: String::new(),
            description: String::new(),
            new_branch: String::new(),
            history_tab: false,
            diff_title: String::new(),
            startup,
            first_frame: true,
            first_snapshot: true,
            smoke_deadline: smoke.then(|| Instant::now() + std::time::Duration::from_secs(3)),
        };
        app.submit(Command::Preferences);
        Ok(app)
    }

    fn submit(&mut self, command: Command) -> bool {
        if self.state.writing() {
            return false;
        }
        match self.worker.submit(self.state.generation, command.clone()) {
            Ok(id) => {
                self.state.pending = Some(Pending { id, command });
                self.state.error = None;
                true
            }
            Err(error) => {
                self.state.error = Some(error.into());
                false
            }
        }
    }

    fn open(&mut self, path: PathBuf) {
        if self.state.writing() {
            return;
        }
        // Increment only after successful submission, retaining old data if the queue is full.
        let next = self.state.generation + 1;
        match self.worker.submit(next, Command::Open(path.clone())) {
            Ok(id) => {
                self.state.generation = next;
                self.state.pending = Some(Pending {
                    id,
                    command: Command::Open(path),
                });
                self.state.snapshot = None;
                self.state.selected = None;
                self.state.diff = None;
                self.state.history.clear();
                self.state.history_head = None;
                self.state.history_offset = 0;
                self.state.history_has_more = false;
                self.state.error = None;
                self.summary.clear();
                self.description.clear();
                self.history_tab = false;
                self.diff_title.clear();
            }
            Err(err) => self.state.error = Some(err.into()),
        }
    }

    fn events(&mut self) {
        while let Some(event) = self.worker.try_event() {
            // Preferences warnings are session-level and should survive cancelled reads.
            if let Some(warning) = event.warning {
                self.state.warning = Some(warning);
            }
            let event = crate::command::Event {
                warning: None,
                ..event
            };
            if !self.state.accepts(&event) {
                continue;
            }
            let pending = self.state.pending.take().expect("matching request");
            match event.result {
                Ok(Payload::Preferences(preferences)) => {
                    self.state.preferences = preferences;
                    if let Some(path) = self.initial_path.take() {
                        self.open(path);
                    }
                }
                Ok(Payload::Snapshot(snapshot)) => {
                    if self.first_snapshot {
                        eprintln!(
                            "metric repository_ready_ms={:.2}",
                            self.startup.elapsed().as_secs_f64() * 1000.0
                        );
                        self.first_snapshot = false;
                    }
                    self.state.preferences.remember(snapshot.root.clone());
                    self.path_input = snapshot.root.display().to_string();
                    self.state.selected = None;
                    self.state.diff = None;
                    self.state.snapshot = Some(snapshot);
                }
                Ok(Payload::Diff(diff)) => self.state.diff = Some(diff),
                Ok(Payload::History(page)) => {
                    if let Command::History { skip, .. } = pending.command {
                        if skip > 0 && page.head != self.state.history_head {
                            self.state.error = Some("HEAD changed while paging history. Refresh history from its first page.".into());
                            continue;
                        }
                        self.state.history_offset = skip;
                    }
                    self.state.history_head = page.head;
                    self.state.history = page.commits;
                    self.state.history_has_more = page.has_more;
                }
                Ok(Payload::Mutation(message)) => {
                    if let Command::Mutate { root, mutation, .. } = pending.command {
                        if matches!(mutation, Mutation::Commit { .. }) {
                            self.summary.clear();
                            self.description.clear();
                        }
                        self.state.message = Some(message);
                        self.submit(Command::Open(root));
                        self.state.history.clear();
                        self.state.history_head = None;
                        self.state.history_offset = 0;
                    }
                }
                Err(Error::Cancelled) => self.state.message = Some("Read cancelled".into()),
                Err(error) => self.state.error = Some(error.to_string()),
            }
        }
    }

    fn action(&mut self, action: Action) {
        match action {
            Action::Open(path) => self.open(path),
            Action::Cancel if !self.state.writing() => self.worker.cancel(),
            Action::Cancel => {}
            Action::Changes => {
                if self.state.writing() {
                    return;
                }
                self.worker.cancel();
                self.state.pending = None;
                self.history_tab = false;
                self.state.diff = None;
                self.state.selected = None;
                self.diff_title.clear();
            }
            Action::Refresh => {
                if let Some(snapshot) = &self.state.snapshot {
                    if self.submit(Command::Open(snapshot.root.clone())) {
                        self.state.history.clear();
                        self.state.history_head = None;
                        self.state.history_offset = 0;
                        self.state.history_has_more = false;
                        self.history_tab = false;
                        self.diff_title.clear();
                    }
                }
            }
            Action::SelectFile(file) => {
                if let Some(snapshot) = &self.state.snapshot {
                    let command = Command::Diff {
                        root: snapshot.root.clone(),
                        file: file.clone(),
                    };
                    if self.submit(command) {
                        self.diff_title = format!("{} · {:?}", file.path.display(), file.side);
                        self.state.selected = Some(file);
                        self.state.diff = None;
                        self.history_tab = false;
                    }
                }
            }
            Action::History(skip) => {
                if let Some(snapshot) = &self.state.snapshot {
                    if self.submit(Command::History {
                        root: snapshot.root.clone(),
                        skip,
                    }) {
                        self.history_tab = true;
                        self.state.diff = None;
                        self.state.selected = None;
                        self.diff_title.clear();
                    }
                }
            }
            Action::CommitDiff(oid) => {
                if let Some(snapshot) = &self.state.snapshot {
                    if self.submit(Command::CommitDiff {
                        root: snapshot.root.clone(),
                        oid: oid.clone(),
                    }) {
                        self.state.diff = None;
                        self.diff_title = format!("Commit {}", &oid[..8]);
                    }
                }
            }
            Action::Mutate(mutation) => {
                if self.state.pending.is_none() {
                    if let Some(snapshot) = &self.state.snapshot {
                        self.submit(Command::Mutate {
                            root: snapshot.root.clone(),
                            mutation,
                            revision: snapshot.revision,
                        });
                    }
                }
            }
        }
    }
}

impl eframe::App for GitRustApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let frame_start = Instant::now();
        self.events();
        let mut actions = Vec::new();
        let writing = self.state.writing();
        let can_write = self.state.pending.is_none()
            && self
                .state
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.write_blockers.is_empty());

        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("GitRust Desktop");
                ui.label(RichText::new("LOCAL PREVIEW").small().weak());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    egui::widgets::global_theme_preference_buttons(ui);
                });
            });
            ui.add_enabled_ui(!writing, |ui| {
                ui.horizontal(|ui| {
                    let input = ui.add(
                        egui::TextEdit::singleline(&mut self.path_input)
                            .hint_text("Repository path, e.g. /home/user/project")
                            .desired_width(420.0),
                    );
                    if ui.button("Open").clicked()
                        || (input.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                    {
                        actions.push(Action::Open(PathBuf::from(self.path_input.trim())));
                    }
                    egui::ComboBox::from_id_salt("recent")
                        .selected_text("Recent")
                        .show_ui(ui, |ui| {
                            for path in &self.state.preferences.recent {
                                if ui.button(path.display().to_string()).clicked() {
                                    actions.push(Action::Open(path.clone()));
                                }
                            }
                        });
                    if ui
                        .add_enabled(self.state.snapshot.is_some(), egui::Button::new("Refresh"))
                        .clicked()
                    {
                        actions.push(Action::Refresh);
                    }
                });
            });
            if let Some(error) = &self.state.error {
                ui.colored_label(egui::Color32::from_rgb(235, 95, 95), error);
            }
            if let Some(warning) = &self.state.warning {
                ui.label(RichText::new(warning).color(egui::Color32::from_rgb(200, 150, 60)));
            }
            if let Some(snapshot) = &self.state.snapshot {
                if !snapshot.write_blockers.is_empty() {
                    ui.collapsing("Read-only: compatibility requirements detected", |ui| {
                        for blocker in &snapshot.write_blockers {
                            ui.label(blocker);
                        }
                    });
                }
            }
        });
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if self.state.pending.is_some() {
                    ui.spinner();
                    ui.label(if writing {
                        "Writing Git changes…"
                    } else {
                        "Reading repository…"
                    });
                    if !writing && ui.small_button("Cancel read").clicked() {
                        actions.push(Action::Cancel);
                    }
                } else {
                    ui.label(self.state.message.as_deref().unwrap_or(
                        "Ready · Local Git workflows · Remote operations are not available yet",
                    ));
                }
            });
        });
        egui::SidePanel::left("repository")
            .default_width(320.0)
            .min_width(240.0)
            .show(ctx, |ui| {
                ui.add_enabled_ui(!writing, |ui| {
                    if let Some(snapshot) = &self.state.snapshot {
                        ui.heading(
                            snapshot
                                .root
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy(),
                        );
                        ui.label(format!("Current branch: {}", snapshot.head));
                        egui::ComboBox::from_id_salt("branch")
                            .selected_text(&snapshot.head)
                            .show_ui(ui, |ui| {
                                for branch in &snapshot.branches {
                                    if ui
                                        .add_enabled(
                                            can_write && !branch.current,
                                            egui::Button::new(&branch.name),
                                        )
                                        .clicked()
                                    {
                                        actions.push(Action::Mutate(Mutation::Checkout(
                                            branch.name.clone(),
                                        )));
                                    }
                                }
                            });
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.new_branch)
                                    .hint_text("New branch")
                                    .desired_width(180.0),
                            );
                            if ui
                                .add_enabled(
                                    can_write && !self.new_branch.trim().is_empty(),
                                    egui::Button::new("Create"),
                                )
                                .clicked()
                            {
                                actions.push(Action::Mutate(Mutation::CreateBranch(
                                    self.new_branch.trim().into(),
                                )));
                            }
                        });
                        let staged = snapshot
                            .files
                            .iter()
                            .filter(|file| file.side == Side::Staged)
                            .count();
                        ui.label(format!(
                            "{staged} staged · {} unstaged",
                            snapshot.files.len() - staged
                        ));
                    }
                    ui.separator();
                    egui::TopBottomPanel::bottom("commit_form").show_inside(ui, |ui| {
                        components::selected_file_actions(
                            ui,
                            self.state.selected.as_ref(),
                            can_write,
                            &mut actions,
                        );
                        ui.separator();
                        ui.label("Commit staged changes");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.summary)
                                .hint_text("Summary")
                                .desired_width(f32::INFINITY),
                        );
                        ui.add(
                            egui::TextEdit::multiline(&mut self.description)
                                .hint_text("Description (optional)")
                                .desired_rows(3)
                                .desired_width(f32::INFINITY),
                        );
                        let staged = self.state.snapshot.as_ref().is_some_and(|snapshot| {
                            snapshot.files.iter().any(|file| file.side == Side::Staged)
                        });
                        if ui
                            .add_enabled(
                                can_write && staged && !self.summary.trim().is_empty(),
                                egui::Button::new("Commit staged changes"),
                            )
                            .clicked()
                        {
                            actions.push(Action::Mutate(Mutation::Commit {
                                summary: self.summary.clone(),
                                description: self.description.clone(),
                            }));
                        }
                        ui.small("Commits preserve the index staged from your terminal.");
                    });
                    components::file_list(ui, &self.state, &mut actions);
                });
            });
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_enabled_ui(!writing, |ui| {
                ui.horizontal(|ui| {
                    if ui.selectable_label(!self.history_tab, "Changes").clicked() {
                        actions.push(Action::Changes);
                    }
                    if ui
                        .add_enabled(
                            self.state.snapshot.is_some(),
                            egui::Button::selectable(self.history_tab, "History"),
                        )
                        .clicked()
                    {
                        actions.push(Action::History(0));
                    }
                });
                if self.history_tab {
                    let row_height = ui.text_style_height(&egui::TextStyle::Body) + 8.0;
                    egui::ScrollArea::vertical()
                        .id_salt("history")
                        .max_height(200.0)
                        .show_rows(ui, row_height, self.state.history.len(), |ui, rows| {
                            for row in rows {
                                let commit = &self.state.history[row];
                                if ui
                                    .selectable_label(
                                        self.diff_title.ends_with(&commit.oid[..8]),
                                        format!(
                                            "{}  {} · {}",
                                            &commit.oid[..8],
                                            commit.summary,
                                            commit.author
                                        ),
                                    )
                                    .clicked()
                                {
                                    actions.push(Action::CommitDiff(commit.oid.clone()));
                                }
                            }
                        });
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                self.state.history_offset > 0,
                                egui::Button::new("Previous 100"),
                            )
                            .clicked()
                        {
                            actions.push(Action::History(
                                self.state.history_offset.saturating_sub(100),
                            ));
                        }
                        if ui
                            .add_enabled(self.state.history_has_more, egui::Button::new("Next 100"))
                            .clicked()
                        {
                            actions.push(Action::History(self.state.history_offset + 100));
                        }
                    });
                }
                ui.separator();
                if !self.diff_title.is_empty() {
                    ui.label(&self.diff_title);
                }
                components::diff_view(ui, self.state.diff.as_ref());
            });
        });
        if let Some(action) = actions.into_iter().next() {
            self.action(action);
        }
        if self.first_frame {
            eprintln!(
                "metric first_ui_frame_ms={:.2}",
                self.startup.elapsed().as_secs_f64() * 1000.0
            );
            self.first_frame = false;
        }
        let elapsed = frame_start.elapsed().as_secs_f64() * 1000.0;
        if elapsed > 16.7 {
            eprintln!("metric slow_ui_update_ms={elapsed:.2}");
        }
        if let Some(deadline) = self.smoke_deadline {
            if Instant::now() >= deadline {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                ctx.request_repaint_after(deadline.saturating_duration_since(Instant::now()));
            }
        }
    }
}
