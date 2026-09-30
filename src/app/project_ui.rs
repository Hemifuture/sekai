use std::{collections::BTreeMap, path::PathBuf, sync::mpsc};

use super::{
    authoring::SaveStamp,
    project_file::{
        compare_generator, current_generator_identity, upgrade_copy, Compatibility,
        GeneratorIdentity, ProjectFile, ProjectView,
    },
    project_io::{
        read_project, save_project, DiskStamp, LoadedProject, ProjectIoError, SaveExpectation,
        SaveReceipt,
    },
    AuthorInputs, PendingOpenBuild, TemplateApp,
};

/// Native project commands; input history stays in the authoring session.
#[derive(Debug, Clone)]
pub(super) enum ProjectAction {
    New,
    Open(PathBuf),
    Save,
    SaveAs(PathBuf),
    Undo,
    Redo,
    CancelBuild,
    RetryBuild,
    Exit,
}

struct ProjectIoCompletion {
    id: u64,
    epoch: u64,
    result: Result<ProjectIoOutcome, ProjectIoError>,
}

enum ProjectIoOutcome {
    Opened(Box<LoadedProject>),
    Saved(SaveReceipt),
    Inspected(DiskStamp),
}

/// One short native worker, distinct from the single world-build slot.
struct PendingProjectIo {
    id: u64,
    epoch: u64,
    revision: u64,
    save_stamp: Option<SaveStamp>,
    path: PathBuf,
    receiver: mpsc::Receiver<ProjectIoCompletion>,
    worker: Option<std::thread::JoinHandle<()>>,
}

enum IoTask {
    Read,
    Save(Box<ProjectFile>, SaveExpectation),
    InspectOverwrite,
}

#[derive(Clone, Copy)]
enum PathDialog {
    Open,
    SaveAs,
}

struct PendingConfirmation {
    epoch: u64,
    revision: u64,
    input_edit: Option<AuthorInputs>,
    kind: ConfirmationKind,
}

enum ConfirmationKind {
    Unsaved(ProjectAction),
    Compatibility {
        path: PathBuf,
        loaded: Box<LoadedProject>,
        current: GeneratorIdentity,
        compatibility: Compatibility,
    },
    Overwrite {
        path: PathBuf,
        disk_stamp: DiskStamp,
    },
}

#[derive(Clone, Copy)]
enum ConfirmationChoice {
    Save,
    Discard,
    Continue,
    Cancel,
}

/// Presentation and file identity only; editable recipes live in AuthoringSession.
#[derive(Default)]
pub(super) struct ProjectUiState {
    path: Option<PathBuf>,
    disk_stamp: Option<DiskStamp>,
    path_text: String,
    path_dialog: Option<PathDialog>,
    confirmation: Option<PendingConfirmation>,
    after_save: Option<(u64, ProjectAction)>,
    io: Option<PendingProjectIo>,
    next_io_id: u64,
    status: Option<String>,
    conflict_path: Option<PathBuf>,
    extensions: BTreeMap<String, serde_json::Value>,
    upgraded_from: Option<GeneratorIdentity>,
    protected_source: Option<PathBuf>,
    exit_approved: bool,
}

impl ProjectUiState {
    fn entered_path(&self) -> Option<PathBuf> {
        let path = PathBuf::from(&self.path_text);
        (!self.path_text.trim().is_empty() && path.is_absolute()).then_some(path)
    }
}

impl TemplateApp {
    /// Includes pending numeric text without duplicating recipe dirty state.
    pub(super) fn project_is_dirty(&self) -> bool {
        self.authoring.is_dirty() || self.input_edit.is_some()
    }

    /// Applies a UI command before any completion may settle.
    pub(super) fn dispatch_project_action(&mut self, action: ProjectAction, ctx: &egui::Context) {
        match action {
            ProjectAction::Undo | ProjectAction::Redo => {
                release_project_text_focus(ctx);
                self.input_edit = None;
                let changed = match action {
                    ProjectAction::Undo => self.authoring.undo(),
                    _ => self.authoring.redo(),
                };
                if changed {
                    self.cancel_world_build();
                }
                self.project_ui.status = None;
            }
            ProjectAction::CancelBuild => self.cancel_world_build(),
            ProjectAction::RetryBuild => {
                if self.input_edit.is_some() {
                    self.project_ui.status = Some("请先提交或取消参数文本，再重建".into());
                } else {
                    self.pending_open_build = None;
                    self.request_spherical_world_build();
                }
            }
            ProjectAction::Save => {
                if self.input_edit.is_some() {
                    self.project_ui.status = Some("请先提交或取消参数文本，再保存".into());
                    self.project_ui.after_save = None;
                    return;
                }
                if let Some(path) = self.project_ui.path.clone() {
                    self.start_project_save(path, None, ctx);
                } else {
                    self.show_project_path_dialog(PathDialog::SaveAs);
                }
            }
            ProjectAction::SaveAs(path) => self.start_project_save(path, None, ctx),
            action @ (ProjectAction::New | ProjectAction::Open(_) | ProjectAction::Exit) => {
                if self.project_is_dirty() {
                    if matches!(action, ProjectAction::Exit) {
                        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                    }
                    self.set_project_confirmation(ConfirmationKind::Unsaved(action));
                } else {
                    self.continue_project_action(action, ctx);
                }
            }
        }
    }

    fn continue_project_action(&mut self, action: ProjectAction, ctx: &egui::Context) {
        match action {
            ProjectAction::New => {
                release_project_text_focus(ctx);
                self.cancel_world_build();
                if let Err(error) = self.authoring.reset_project(AuthorInputs::default(), false) {
                    self.project_ui.status = Some(error.to_string());
                    return;
                }
                self.clear_project_publication();
                self.project_ui.path = None;
                self.project_ui.disk_stamp = None;
                self.project_ui.extensions.clear();
                self.project_ui.upgraded_from = None;
                self.project_ui.protected_source = None;
                self.project_ui.status = Some("新项目尚未保存；点击重建生成地图".into());
                self.project_ui.after_save = None;
            }
            ProjectAction::Open(path) => {
                release_project_text_focus(ctx);
                self.start_project_io(path, None, IoTask::Read, ctx);
            }
            ProjectAction::Exit => {
                self.cancel_world_build();
                self.project_ui.exit_approved = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            _ => {}
        }
    }

    fn clear_project_publication(&mut self) {
        self.input_edit = None;
        self.spherical_presentation
            .with_resource(|published| *published = None);
        self.published_world_token = None;
        self.amplified_detail = None;
        self.amplified_mesh = None;
        self.amplified_map_projected = None;
        self.river_polylines = None;
        self.river_radius_m = None;
        self.formation_surface = None;
        self.stage_cache = crate::engine::MemoryStageCache::new();
        self.active_runtime_graph = None;
        self.active_runtime_stage_ids.clear();
        self.spherical_inspector_cache = super::SphericalInspectorCache::default();
        self.spherical_runtime_error = None;
    }

    fn start_project_save(
        &mut self,
        path: PathBuf,
        expected: Option<SaveExpectation>,
        ctx: &egui::Context,
    ) {
        if self.input_edit.is_some() {
            self.project_ui.status = Some("请先提交或取消参数文本，再保存".into());
            self.project_ui.after_save = None;
            return;
        }
        let mut file = match ProjectFile::new(self.authoring.draft().clone()) {
            Ok(file) => file,
            Err(error) => {
                self.project_ui.status = Some(format!("保存失败：{error}"));
                self.project_ui.after_save = None;
                return;
            }
        };
        file.view =
            ProjectView::from_state(&self.spherical_canvas_state, self.reduce_loading_motion);
        file.extensions = self.project_ui.extensions.clone();
        file.upgraded_from = self.project_ui.upgraded_from.clone();
        let expected = expected.unwrap_or_else(|| {
            if self.project_ui.path.as_ref() == Some(&path) {
                self.project_ui
                    .disk_stamp
                    .map(SaveExpectation::Unchanged)
                    .unwrap_or(SaveExpectation::NewFile)
            } else {
                SaveExpectation::NewFile
            }
        });
        self.start_project_io(
            path,
            Some(self.authoring.capture_save()),
            IoTask::Save(Box::new(file), expected),
            ctx,
        );
    }

    fn start_project_io(
        &mut self,
        path: PathBuf,
        save_stamp: Option<SaveStamp>,
        task: IoTask,
        ctx: &egui::Context,
    ) {
        if self.project_ui.io.is_some() {
            self.project_ui.status = Some("文件操作尚未结束，请稍候重试".into());
            self.project_ui.after_save = None;
            return;
        }
        if !path.is_absolute() {
            self.project_ui.status = Some("请输入完整的绝对文件路径".into());
            self.project_ui.after_save = None;
            return;
        }
        self.project_ui.next_io_id = self
            .project_ui
            .next_io_id
            .checked_add(1)
            .expect("project I/O ID exhausted");
        let id = self.project_ui.next_io_id;
        let epoch = self.authoring.capture_save().epoch;
        let worker_path = path.clone();
        let protected = self.project_ui.protected_source.clone();
        let worker_ctx = ctx.clone();
        let (sender, receiver) = mpsc::channel();
        self.project_ui.status = Some(
            match task {
                IoTask::Read => "正在读取项目…",
                IoTask::Save(..) => "正在保存配方…",
                IoTask::InspectOverwrite => "正在检查磁盘版本…",
            }
            .into(),
        );
        let worker = std::thread::spawn(move || {
            let result = match task {
                IoTask::Read => read_project(&worker_path)
                    .map(Box::new)
                    .map(ProjectIoOutcome::Opened),
                IoTask::InspectOverwrite => read_project(&worker_path)
                    .map(|loaded| ProjectIoOutcome::Inspected(loaded.disk_stamp)),
                IoTask::Save(file, expected) => {
                    if protected
                        .as_ref()
                        .is_some_and(|source| same_existing_path(source, &worker_path))
                    {
                        Err(ProjectIoError::Write {
                            source: std::io::Error::new(
                                std::io::ErrorKind::InvalidInput,
                                "升级副本必须保存到其他路径，原项目保留",
                            ),
                            recovery_paths: vec![],
                        })
                    } else {
                        save_project(&worker_path, &file, expected).map(ProjectIoOutcome::Saved)
                    }
                }
            };
            let _ = sender.send(ProjectIoCompletion { id, epoch, result });
            worker_ctx.request_repaint();
        });
        self.project_ui.io = Some(PendingProjectIo {
            id,
            epoch,
            revision: self.authoring.revision(),
            save_stamp,
            path,
            receiver,
            worker: Some(worker),
        });
        ctx.request_repaint();
    }

    /// Receives only completed native workers and rejects stale operation identities.
    pub(super) fn poll_project_io(&mut self, ctx: &egui::Context) {
        let Some(pending) = self.project_ui.io.as_mut() else {
            return;
        };
        if pending
            .worker
            .as_ref()
            .is_some_and(|worker| !worker.is_finished())
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
            return;
        }
        if let Some(worker) = pending.worker.take() {
            if worker.join().is_err() {
                self.project_ui.status = Some("文件操作失败：线程意外终止".into());
                self.project_ui.io = None;
                self.project_ui.after_save = None;
                ctx.request_repaint();
                return;
            }
        }
        let completion = match pending.receiver.try_recv() {
            Ok(completion) => completion,
            Err(mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
                return;
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.project_ui.io = None;
                self.project_ui.after_save = None;
                self.project_ui.status = Some("文件操作失败：线程没有返回结果".into());
                ctx.request_repaint();
                return;
            }
        };
        let pending = self
            .project_ui
            .io
            .take()
            .expect("received project I/O was pending");
        if completion.id != pending.id
            || completion.epoch != pending.epoch
            || completion.epoch != self.authoring.capture_save().epoch
        {
            return;
        }
        self.project_ui.conflict_path = None;
        match completion.result {
            Ok(ProjectIoOutcome::Opened(loaded)) => {
                if pending.revision != self.authoring.revision() || self.input_edit.is_some() {
                    // A discard authorized before the read cannot discard edits made
                    // while it ran. Re-read only after another explicit choice.
                    self.dispatch_project_action(ProjectAction::Open(pending.path), ctx);
                } else {
                    self.receive_open_project(pending.path, *loaded);
                }
            }
            Ok(ProjectIoOutcome::Inspected(disk_stamp)) => {
                self.project_ui.status = Some("磁盘版本已检查，请确认覆盖".into());
                self.set_project_confirmation(ConfirmationKind::Overwrite {
                    path: pending.path,
                    disk_stamp,
                });
            }
            Ok(ProjectIoOutcome::Saved(receipt)) => {
                let Some(stamp) = pending.save_stamp else {
                    return;
                };
                if !self.authoring.mark_saved(stamp) {
                    return;
                }
                self.project_ui.path = Some(receipt.path);
                self.project_ui.disk_stamp = Some(receipt.disk_stamp);
                self.project_ui.status = Some(match receipt.backup {
                    Some(path) => {
                        format!("配方已保存；旧配方备份：{}。重开需重新生成", path.display())
                    }
                    None => "配方已保存；重开需重新生成".into(),
                });
                if let Some((epoch, action)) = self.project_ui.after_save.take() {
                    if epoch == self.authoring.capture_save().epoch {
                        self.dispatch_project_action(action, ctx);
                    }
                }
            }
            Err(error) => {
                let operation = if pending.save_stamp.is_some() {
                    "保存失败"
                } else {
                    "打开或检查失败"
                };
                let paths = error
                    .recovery_paths()
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>();
                self.project_ui.status = Some(format!(
                    "{operation}：{error}{}",
                    if paths.is_empty() {
                        String::new()
                    } else {
                        format!(
                            "；返回时可读的文件位置（未确认完整恢复）：{}",
                            paths.join("，")
                        )
                    }
                ));
                if pending.save_stamp.is_some() && matches!(error, ProjectIoError::Conflict { .. })
                {
                    self.project_ui.conflict_path = Some(pending.path);
                }
                self.project_ui.after_save = None;
            }
        }
        ctx.request_repaint();
    }

    fn receive_open_project(&mut self, path: PathBuf, loaded: LoadedProject) {
        let current = match current_generator_identity(&loaded.file.inputs) {
            Ok(current) => current,
            Err(error) => {
                self.project_ui.status = Some(format!("打开失败：{error}"));
                return;
            }
        };
        let compatibility = compare_generator(&loaded.file.generator, &current);
        if compatibility == Compatibility::Exact {
            self.install_open_project(path, loaded, false, false);
        } else {
            self.set_project_confirmation(ConfirmationKind::Compatibility {
                path,
                loaded: Box::new(loaded),
                current,
                compatibility,
            });
        }
    }

    fn install_open_project(
        &mut self,
        path: PathBuf,
        loaded: LoadedProject,
        upgraded: bool,
        platform: bool,
    ) {
        let (canvas, reduce_motion) = match loaded.file.view.clone().try_into_state() {
            Ok(view) => view,
            Err(error) => {
                self.project_ui.status = Some(format!("打开失败：{error}"));
                return;
            }
        };
        // Cancel the OLD epoch before reset. Cancel after setting the intent would erase
        // the new project's one-shot request while its old worker is still exiting.
        self.cancel_world_build();
        if let Err(error) = self.authoring.reset_project(loaded.file.inputs, !upgraded) {
            self.project_ui.status = Some(format!("打开失败：{error}"));
            return;
        }
        self.clear_project_publication();
        self.spherical_canvas_state = canvas;
        self.reduce_loading_motion = reduce_motion;
        self.project_ui.path = (!upgraded).then(|| path.clone());
        self.project_ui.disk_stamp = (!upgraded).then_some(loaded.disk_stamp);
        self.project_ui.protected_source = upgraded.then_some(path);
        self.project_ui.extensions = loaded.file.extensions;
        self.project_ui.upgraded_from = loaded.file.upgraded_from;
        self.project_ui.after_save = None;
        self.project_ui.status = Some(
            if upgraded {
                "已用当前生成器升级为副本；原文件保留，请另存为"
            } else if platform {
                "项目已打开，配方已恢复；已确认跨平台重建，不保证逐位恢复"
            } else {
                "项目已打开，保存的配方已恢复"
            }
            .into(),
        );
        let stamp = self.authoring.capture_save();
        self.pending_open_build = Some(PendingOpenBuild {
            epoch: stamp.epoch,
            recipe_id: stamp.recipe_id,
        });
    }

    fn resolve_project_confirmation(&mut self, choice: ConfirmationChoice, ctx: &egui::Context) {
        let Some(pending) = self.project_ui.confirmation.take() else {
            return;
        };
        if pending.epoch != self.authoring.capture_save().epoch {
            return;
        }
        if !matches!(choice, ConfirmationChoice::Cancel)
            && (pending.revision != self.authoring.revision()
                || pending.input_edit != self.input_edit)
        {
            match pending.kind {
                ConfirmationKind::Compatibility { path, .. } => {
                    self.dispatch_project_action(ProjectAction::Open(path), ctx);
                }
                kind => self.set_project_confirmation(kind),
            }
            self.project_ui.status = Some("确认期间参数已变化，请重新确认项目操作".into());
            return;
        }
        match (pending.kind, choice) {
            (ConfirmationKind::Unsaved(action), ConfirmationChoice::Save) => {
                self.project_ui.after_save = Some((pending.epoch, action));
                self.dispatch_project_action(ProjectAction::Save, ctx);
            }
            (ConfirmationKind::Unsaved(action), ConfirmationChoice::Discard) => {
                self.input_edit = None;
                self.continue_project_action(action, ctx);
            }
            (
                ConfirmationKind::Compatibility {
                    path,
                    mut loaded,
                    current,
                    compatibility,
                },
                ConfirmationChoice::Continue,
            ) => {
                let upgraded = matches!(compatibility, Compatibility::UpgradeRequired(_));
                if upgraded {
                    loaded.file = upgrade_copy(&loaded.file, current);
                }
                self.install_open_project(path, *loaded, upgraded, !upgraded);
            }
            (ConfirmationKind::Overwrite { path, disk_stamp }, ConfirmationChoice::Continue) => {
                self.start_project_save(
                    path,
                    Some(SaveExpectation::ConfirmedOverwrite(disk_stamp)),
                    ctx,
                );
            }
            _ => {
                self.project_ui.after_save = None;
                self.project_ui.status = Some("项目操作已取消".into());
            }
        }
    }

    fn set_project_confirmation(&mut self, kind: ConfirmationKind) {
        self.project_ui.confirmation = Some(PendingConfirmation {
            epoch: self.authoring.capture_save().epoch,
            revision: self.authoring.revision(),
            input_edit: self.input_edit.clone(),
            kind,
        });
    }

    fn show_project_path_dialog(&mut self, kind: PathDialog) {
        self.project_ui.path_text = self
            .project_ui
            .path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        self.project_ui.path_dialog = Some(kind);
    }

    /// Collects menu actions for dispatch after this frame commits authoring input.
    pub(super) fn show_project_menus(
        &mut self,
        ui: &mut egui::Ui,
        actions: &mut Vec<ProjectAction>,
    ) {
        ui.menu_button("文件", |ui| {
            if ui.button("新建").clicked() {
                actions.push(ProjectAction::New);
                ui.close_menu();
            }
            if ui.button("打开…").clicked() {
                self.show_project_path_dialog(PathDialog::Open);
                ui.close_menu();
            }
            if ui.button("保存").clicked() {
                actions.push(ProjectAction::Save);
                ui.close_menu();
            }
            if ui.button("另存为…").clicked() {
                self.show_project_path_dialog(PathDialog::SaveAs);
                ui.close_menu();
            }
            ui.separator();
            if ui.button("退出").clicked() {
                actions.push(ProjectAction::Exit);
                ui.close_menu();
            }
        });
        ui.menu_button("编辑", |ui| {
            if ui.button("撤销  Ctrl+Z").clicked() {
                actions.push(ProjectAction::Undo);
                ui.close_menu();
            }
            if ui.button("重做  Ctrl+Shift+Z / Ctrl+Y").clicked() {
                actions.push(ProjectAction::Redo);
                ui.close_menu();
            }
        });
    }

    /// Returns history shortcuts only when a text widget does not own keyboard input.
    pub(super) fn project_shortcut(&self, ctx: &egui::Context) -> Option<ProjectAction> {
        if ctx.wants_keyboard_input() {
            return None;
        }
        ctx.input(|input| {
            if !input.modifiers.command {
                return None;
            }
            if input.key_pressed(egui::Key::Z) {
                Some(if input.modifiers.shift {
                    ProjectAction::Redo
                } else {
                    ProjectAction::Undo
                })
            } else if input.key_pressed(egui::Key::Y) {
                Some(ProjectAction::Redo)
            } else {
                None
            }
        })
    }

    /// Draws absolute-path and epoch-bound confirmation dialogs.
    pub(super) fn show_project_dialogs(
        &mut self,
        ctx: &egui::Context,
        actions: &mut Vec<ProjectAction>,
    ) {
        if let Some(kind) = self.project_ui.path_dialog {
            let title = match kind {
                PathDialog::Open => "打开项目",
                PathDialog::SaveAs => "配方另存为",
            };
            let mut open = true;
            let mut accepted = None;
            egui::Window::new(title)
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label("粘贴完整的绝对文件路径（支持中文与空格）");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.project_ui.path_text)
                            .desired_width(430.0),
                    );
                    ui.label(format!("目标：{}", self.project_ui.path_text));
                    let path = self.project_ui.entered_path();
                    if ui
                        .add_enabled(
                            path.is_some(),
                            egui::Button::new(match kind {
                                PathDialog::Open => "打开",
                                PathDialog::SaveAs => "保存",
                            }),
                        )
                        .clicked()
                    {
                        accepted = path;
                    }
                    if ui.button("取消").clicked() {
                        self.project_ui.path_dialog = None;
                        self.project_ui.after_save = None;
                    }
                });
            if !open {
                self.project_ui.path_dialog = None;
                self.project_ui.after_save = None;
            }
            if let Some(path) = accepted {
                self.project_ui.path_dialog = None;
                actions.push(match kind {
                    PathDialog::Open => ProjectAction::Open(path),
                    PathDialog::SaveAs => ProjectAction::SaveAs(path),
                });
            }
        }
        if self
            .project_ui
            .confirmation
            .as_ref()
            .is_some_and(|pending| pending.epoch != self.authoring.capture_save().epoch)
        {
            self.project_ui.confirmation = None;
        }
        if let Some(pending) = &self.project_ui.confirmation {
            let mut choice = None;
            let mut open = true;
            egui::Window::new("确认项目操作")
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .show(ctx, |ui| {
                    match &pending.kind {
                        ConfirmationKind::Unsaved(_) => {
                            ui.label("当前配方或参数文本尚未保存。");
                            ui.horizontal(|ui| {
                                if ui.button("保存").clicked() {
                                    choice = Some(ConfirmationChoice::Save);
                                }
                                if ui.button("放弃").clicked() {
                                    choice = Some(ConfirmationChoice::Discard);
                                }
                            });
                        }
                        ConfirmationKind::Compatibility {
                            path,
                            compatibility,
                            ..
                        } => {
                            ui.label(format!("项目：{}", path.display()));
                            match compatibility {
                                Compatibility::UpgradeRequired(differences) => {
                                    ui.label(
                                        "原生成器或依赖不兼容，无法按原版重建。原文件将保留。",
                                    );
                                    for difference in differences {
                                        ui.label(format!(
                                            "差异：{}",
                                            compatibility_label(difference)
                                        ));
                                    }
                                    if ui.button("用当前生成器升级为副本").clicked() {
                                        choice = Some(ConfirmationChoice::Continue);
                                    }
                                }
                                _ => {
                                    ui.label(
                                        "此项目的浮点平台与当前平台不同，重新生成不保证逐位恢复。",
                                    );
                                    if ui.button("确认跨平台重新生成").clicked() {
                                        choice = Some(ConfirmationChoice::Continue);
                                    }
                                }
                            }
                        }
                        ConfirmationKind::Overwrite { path, .. } => {
                            ui.label(format!("确认覆盖当前磁盘配方：{}", path.display()));
                            ui.label("保存时会再次检查磁盘版本并保留旧配方备份。");
                            if ui.button("确认覆盖").clicked() {
                                choice = Some(ConfirmationChoice::Continue);
                            }
                        }
                    }
                    if ui.button("取消").clicked() {
                        choice = Some(ConfirmationChoice::Cancel);
                    }
                });
            if !open {
                choice = Some(ConfirmationChoice::Cancel);
            }
            if let Some(choice) = choice {
                self.resolve_project_confirmation(choice, ctx);
            }
        }
    }

    /// Routes system window-close requests through the unsaved-recipe decision.
    pub(super) fn intercept_project_close(&mut self, ctx: &egui::Context) {
        if ctx.input(|input| input.viewport().close_requested()) && !self.project_ui.exit_approved {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.dispatch_project_action(ProjectAction::Exit, ctx);
        }
    }

    /// Displays recipe save identity and the published map source independently.
    pub(super) fn show_project_status(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.label(
            self.project_ui
                .path
                .as_ref()
                .map(|path| format!("项目：{}", path.display()))
                .unwrap_or_else(|| "未命名项目".into()),
        );
        ui.label(if self.input_edit.is_some() {
            "参数文本尚未提交；配方尚未保存"
        } else if self.authoring.is_dirty() {
            "配方尚未保存"
        } else {
            "配方已保存；世界结果不保存在项目中"
        });
        if let Some(token) = self.published_world_token {
            let stamp = self.authoring.capture_save();
            ui.label(
                if self.input_edit.is_some()
                    || token.epoch != stamp.epoch
                    || token.recipe_id != stamp.recipe_id
                {
                    "参数已修改，地图仍为上次有效结果"
                } else {
                    "地图对应当前已提交配方"
                },
            );
        }
        if self.pending_open_build.is_some() && self.world_build.is_some() {
            ui.label("项目已打开，正在等待旧构建退出后生成");
        } else if self.project_ui.path.is_some()
            && self.world_build.as_ref().is_some_and(|build| {
                build.token.epoch == self.authoring.capture_save().epoch
                    && self.authoring.accepts(build.token)
                    && !build.cancellation.is_cancelled()
            })
        {
            ui.label("项目已打开，正在按已提交的配方生成");
        }
        if let Some(status) = &self.project_ui.status {
            ui.label(status);
        }
        if let Some(path) = self.project_ui.conflict_path.clone() {
            ui.label("磁盘文件已存在或已被修改；可以另存为，或检查后明确覆盖。");
            if ui.button("另存为…").clicked() {
                self.show_project_path_dialog(PathDialog::SaveAs);
            }
            if ui.button("检查磁盘版本以确认覆盖").clicked() {
                self.start_project_io(path, None, IoTask::InspectOverwrite, ctx);
            }
        }
    }
}

fn release_project_text_focus(ctx: &egui::Context) {
    if let Some(id) = ctx.memory(|memory| memory.focused()) {
        ctx.data_mut(|data| data.remove::<String>(id));
        ctx.memory_mut(|memory| memory.surrender_focus(id));
    }
}

fn same_existing_path(source: &std::path::Path, target: &std::path::Path) -> bool {
    match (source.canonicalize(), target.canonicalize()) {
        (Ok(source), Ok(target)) => {
            #[cfg(windows)]
            {
                source
                    .to_string_lossy()
                    .eq_ignore_ascii_case(&target.to_string_lossy())
            }
            #[cfg(not(windows))]
            {
                source == target
            }
        }
        _ => source == target,
    }
}

fn compatibility_label(component: &str) -> &str {
    match component {
        "pipeline" => "生成管线",
        "product recipe version" => "生成规则版本",
        "stage namespace/id/version" => "生成阶段版本",
        "profile resolution plan" => "质量档位与分辨率方案",
        "fixed model/rule/dependency content" => "模型、规则或依赖内容",
        _ => "生成身份",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uncommitted_text_cannot_display_saved_recipe() {
        let mut app = TemplateApp::default();
        app.authoring.mark_saved(app.authoring.capture_save());
        app.input_edit = Some(app.authoring.draft().clone());
        assert!(app.project_is_dirty());
    }

    #[test]
    fn exit_with_transient_input_requires_unsaved_confirmation() {
        let mut app = TemplateApp::default();
        app.authoring.mark_saved(app.authoring.capture_save());
        app.input_edit = Some(app.authoring.draft().clone());
        let ctx = egui::Context::default();
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            app.dispatch_project_action(ProjectAction::Exit, ctx);
        });
        assert!(output.viewport_output[&egui::ViewportId::ROOT]
            .commands
            .contains(&egui::ViewportCommand::CancelClose));
    }

    fn attach_io(
        app: &mut TemplateApp,
        saving: bool,
    ) -> std::sync::mpsc::Sender<ProjectIoCompletion> {
        let (sender, receiver) = std::sync::mpsc::channel();
        let stamp = app.authoring.capture_save();
        app.project_ui.next_io_id += 1;
        app.project_ui.io = Some(PendingProjectIo {
            id: app.project_ui.next_io_id,
            epoch: stamp.epoch,
            revision: app.authoring.revision(),
            save_stamp: saving.then_some(stamp),
            path: PathBuf::from("C:/项目 带空格.sekai"),
            receiver,
            worker: None,
        });
        sender
    }

    fn saved(app: &TemplateApp) -> ProjectIoCompletion {
        let io = app.project_ui.io.as_ref().unwrap();
        let bytes = super::super::project_file::encode_project(
            &ProjectFile::new(app.authoring.draft().clone()).unwrap(),
        )
        .unwrap();
        ProjectIoCompletion {
            id: io.id,
            epoch: io.epoch,
            result: Ok(ProjectIoOutcome::Saved(SaveReceipt {
                path: io.path.clone(),
                disk_stamp: super::super::project_io::stamp_for_test(&bytes),
                backup: None,
            })),
        }
    }

    #[test]
    fn save_a_completion_retains_edit_b_and_does_not_continue_exit() {
        let mut app = TemplateApp::default();
        let sender = attach_io(&mut app, true);
        let completion = saved(&app);
        app.project_ui.after_save = Some((completion.epoch, ProjectAction::Exit));
        let mut edit = app.authoring.draft().clone();
        edit.world_seed += 1;
        app.authoring.commit("B".into(), edit).unwrap();
        sender.send(completion).unwrap();
        let ctx = egui::Context::default();
        let output = ctx.run(egui::RawInput::default(), |ctx| app.poll_project_io(ctx));
        assert!(app.authoring.is_dirty());
        assert!(!app.project_ui.exit_approved);
        assert!(app.project_ui.confirmation.is_some());
        assert!(!output.viewport_output[&egui::ViewportId::ROOT]
            .commands
            .contains(&egui::ViewportCommand::Close));
    }

    #[test]
    fn stale_save_cannot_bind_a_new_session() {
        let mut app = TemplateApp::default();
        let sender = attach_io(&mut app, true);
        let completion = saved(&app);
        app.authoring
            .reset_project(app.authoring.draft().clone(), false)
            .unwrap();
        sender.send(completion).unwrap();
        app.poll_project_io(&egui::Context::default());
        assert!(app.authoring.is_dirty());
        assert!(app.project_ui.path.is_none());
        assert!(app.project_ui.disk_stamp.is_none());
    }

    #[test]
    fn save_failure_aborts_pending_open_and_exit() {
        for action in [
            ProjectAction::Open(PathBuf::from("C:/next.sekai")),
            ProjectAction::Exit,
        ] {
            let mut app = TemplateApp::default();
            let sender = attach_io(&mut app, true);
            let io = app.project_ui.io.as_ref().unwrap();
            let (id, epoch) = (io.id, io.epoch);
            app.project_ui.after_save = Some((epoch, action));
            sender
                .send(ProjectIoCompletion {
                    id,
                    epoch,
                    result: Err(ProjectIoError::Write {
                        source: std::io::Error::other("full"),
                        recovery_paths: vec![],
                    }),
                })
                .unwrap();
            app.poll_project_io(&egui::Context::default());
            assert!(app.authoring.is_dirty());
            assert!(app.project_ui.after_save.is_none());
            assert!(!app.project_ui.exit_approved);
            assert!(app.project_ui.path.is_none());
            assert!(app.project_ui.status.as_ref().unwrap().contains("保存失败"));
        }
    }

    fn loaded(app: &TemplateApp) -> LoadedProject {
        LoadedProject {
            file: ProjectFile::new(app.authoring.draft().clone()).unwrap(),
            disk_stamp: super::super::project_io::stamp_for_test(b"fixture"),
        }
    }

    #[test]
    fn compatibility_confirmation_precedes_session_reset_and_generation() {
        for platform in [true, false] {
            let mut app = TemplateApp::default();
            let before = app.authoring.capture_save();
            let mut loaded = loaded(&app);
            if platform {
                loaded.file.generator.float_platform_canary[0] ^= 1;
            } else {
                loaded.file.generator.product_recipe_version += 1;
            }
            app.receive_open_project(PathBuf::from("C:/original.sekai"), loaded);
            assert_eq!(app.authoring.capture_save(), before);
            assert!(app.pending_open_build.is_none());
            assert!(app.world_build.is_none());
            app.resolve_project_confirmation(ConfirmationChoice::Cancel, &egui::Context::default());
            assert_eq!(app.authoring.capture_save(), before);
            assert!(app.pending_open_build.is_none());
        }
    }

    #[test]
    fn explicit_upgrade_is_unsaved_unbound_and_preserves_source_identity() {
        let mut app = TemplateApp::default();
        let mut loaded = loaded(&app);
        loaded.file.generator.product_recipe_version += 1;
        let old = loaded.file.generator.clone();
        let path = PathBuf::from("C:/original.sekai");
        app.receive_open_project(path.clone(), loaded);
        app.resolve_project_confirmation(ConfirmationChoice::Continue, &egui::Context::default());
        assert!(app.authoring.is_dirty());
        assert!(app.project_ui.path.is_none());
        assert!(app.project_ui.disk_stamp.is_none());
        assert_eq!(app.project_ui.upgraded_from, Some(old));
        assert_eq!(app.project_ui.protected_source, Some(path));
        assert!(app.pending_open_build.is_some());
    }

    #[test]
    fn confirmed_platform_open_retains_binding_without_claiming_bitwise_restore() {
        let mut app = TemplateApp::default();
        let mut loaded = loaded(&app);
        loaded.file.generator.float_platform_canary[0] ^= 1;
        app.receive_open_project(PathBuf::from("C:/original.sekai"), loaded);
        app.resolve_project_confirmation(ConfirmationChoice::Continue, &egui::Context::default());
        assert!(!app.authoring.is_dirty());
        assert!(app.project_ui.path.is_some());
        assert!(app.project_ui.status.as_ref().unwrap().contains("跨平台"));
        assert!(app.pending_open_build.is_some());
    }

    #[test]
    fn save_rejects_uncommitted_text_and_leaves_session_dirty() {
        let mut app = TemplateApp::default();
        app.input_edit = Some(app.authoring.draft().clone());
        app.dispatch_project_action(
            ProjectAction::SaveAs(PathBuf::from("C:/未提交.sekai")),
            &egui::Context::default(),
        );
        assert!(app.project_ui.io.is_none());
        assert!(app.project_is_dirty());
        assert!(app
            .project_ui
            .status
            .as_ref()
            .unwrap()
            .contains("提交或取消"));
    }

    #[test]
    fn io_id_mismatch_cannot_bind_or_mark_saved() {
        let mut app = TemplateApp::default();
        let sender = attach_io(&mut app, true);
        let mut completion = saved(&app);
        completion.id += 1;
        sender.send(completion).unwrap();
        app.poll_project_io(&egui::Context::default());
        assert!(app.authoring.is_dirty());
        assert!(app.project_ui.path.is_none());
    }

    #[test]
    fn path_input_preserves_chinese_and_spaces_and_rejects_relative_paths() {
        let mut state = ProjectUiState {
            path_text: "C:/项目 文件/世界.sekai".into(),
            ..ProjectUiState::default()
        };
        assert_eq!(
            state.entered_path().unwrap(),
            PathBuf::from(&state.path_text)
        );
        state.path_text = "relative.sekai".into();
        assert!(state.entered_path().is_none());
    }

    #[test]
    fn old_epoch_confirmation_cannot_discard_new_session() {
        let mut app = TemplateApp::default();
        app.dispatch_project_action(ProjectAction::Exit, &egui::Context::default());
        app.authoring
            .reset_project(app.authoring.draft().clone(), false)
            .unwrap();
        app.resolve_project_confirmation(ConfirmationChoice::Discard, &egui::Context::default());
        assert!(!app.project_ui.exit_approved);
    }

    #[test]
    fn open_completion_and_compatibility_choice_recheck_edits_since_authorization() {
        for waiting_on_confirmation in [false, true] {
            let mut app = TemplateApp::default();
            app.authoring.mark_saved(app.authoring.capture_save());
            let epoch = app.authoring.capture_save().epoch;
            let mut loaded = loaded(&app);
            loaded.file.inputs.world_seed = 999;
            loaded.file.generator = current_generator_identity(&loaded.file.inputs).unwrap();
            if waiting_on_confirmation {
                loaded.file.generator.float_platform_canary[0] ^= 1;
                app.receive_open_project(PathBuf::from("C:/opened.sekai"), loaded);
            } else {
                let sender = attach_io(&mut app, false);
                let io = app.project_ui.io.as_ref().unwrap();
                sender
                    .send(ProjectIoCompletion {
                        id: io.id,
                        epoch,
                        result: Ok(ProjectIoOutcome::Opened(Box::new(loaded))),
                    })
                    .unwrap();
            }
            let mut changed = app.authoring.draft().clone();
            changed.world_seed = 717;
            app.authoring
                .commit("edit while waiting".into(), changed)
                .unwrap();
            if waiting_on_confirmation {
                app.resolve_project_confirmation(
                    ConfirmationChoice::Continue,
                    &egui::Context::default(),
                );
            } else {
                app.poll_project_io(&egui::Context::default());
            }
            assert_eq!(app.authoring.capture_save().epoch, epoch);
            assert_eq!(app.authoring.draft().world_seed, 717);
            assert!(app.authoring.is_dirty());
            assert!(app.pending_open_build.is_none());
            assert!(matches!(
                app.project_ui.confirmation.as_ref().unwrap().kind,
                ConfirmationKind::Unsaved(ProjectAction::Open(_))
            ));
        }
    }

    fn wait_io(app: &mut TemplateApp, ctx: &egui::Context) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while app.project_ui.io.is_some() {
            app.poll_project_io(ctx);
            assert!(
                std::time::Instant::now() < deadline,
                "short file worker timed out"
            );
            std::thread::yield_now();
        }
    }

    // Real file-worker coverage is one tiny recipe, never a generated-world fixture.
    #[test]
    fn native_worker_save_open_and_confirmed_overwrite_preserve_recipe_identity() {
        let directory = std::env::temp_dir().join(format!(
            "sekai-t5-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("中文 配方.sekai");
        let ctx = egui::Context::default();
        let mut app = TemplateApp::default();
        app.dispatch_project_action(ProjectAction::SaveAs(path.clone()), &ctx);
        let captured = app.project_ui.io.as_ref().unwrap().save_stamp.unwrap();
        wait_io(&mut app, &ctx);
        assert!(!app.authoring.is_dirty(), "{:?}", app.project_ui.status);
        assert_eq!(app.project_ui.path, Some(path.clone()));
        assert_eq!(app.authoring.capture_save(), captured);
        let mut external = ProjectFile::new(app.authoring.draft().clone()).unwrap();
        external.inputs.world_seed += 1;
        external.generator = current_generator_identity(&external.inputs).unwrap();
        std::fs::write(
            &path,
            super::super::project_file::encode_project(&external).unwrap(),
        )
        .unwrap();
        app.dispatch_project_action(ProjectAction::Save, &ctx);
        wait_io(&mut app, &ctx);
        assert_eq!(app.project_ui.conflict_path, Some(path.clone()));
        app.start_project_io(path.clone(), None, IoTask::InspectOverwrite, &ctx);
        wait_io(&mut app, &ctx);
        app.resolve_project_confirmation(ConfirmationChoice::Continue, &ctx);
        wait_io(&mut app, &ctx);
        assert!(app
            .project_ui
            .status
            .as_ref()
            .unwrap()
            .contains("旧配方备份"));
        assert_eq!(
            read_project(&path).unwrap().file.inputs,
            *app.authoring.draft()
        );
        app.dispatch_project_action(ProjectAction::Open(path.clone()), &ctx);
        wait_io(&mut app, &ctx);
        assert!(app.pending_open_build.is_some());
        assert!(!app.authoring.is_dirty());
        app.dispatch_project_action(ProjectAction::CancelBuild, &ctx);
        assert!(app.pending_open_build.is_none());
        // This directory is unique to this test and contains only its own recipe/backup.
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn upgrade_save_cannot_overwrite_windows_case_or_canonical_source_alias() {
        let directory = std::env::temp_dir().join(format!(
            "sekai-t5-source-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("Original.sekai");
        let original = ProjectFile::new(AuthorInputs::default()).unwrap();
        save_project(&path, &original, SaveExpectation::NewFile).unwrap();
        let before = std::fs::read(&path).unwrap();
        let ctx = egui::Context::default();
        let mut app = TemplateApp::default();
        app.project_ui.protected_source = Some(path.clone());
        let alias = directory.join(".").join("original.sekai");
        #[cfg(not(windows))]
        let alias = directory.join(".").join("Original.sekai");
        app.start_project_save(
            alias,
            Some(SaveExpectation::ConfirmedOverwrite(
                read_project(&path).unwrap().disk_stamp,
            )),
            &ctx,
        );
        wait_io(&mut app, &ctx);
        assert!(app.authoring.is_dirty());
        assert!(app.project_ui.path.is_none());
        assert!(app.project_ui.status.as_ref().unwrap().contains("其他路径"));
        assert_eq!(std::fs::read(&path).unwrap(), before);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn actual_open_update_waits_for_old_worker_then_starts_once_and_never_retries_cancel() {
        use super::super::{prepare_pending_world_build, WorldBuildCompletion};
        let mut app = TemplateApp::default();
        let old_token = app.authoring.begin_build();
        let (sender, receiver) = mpsc::channel();
        let (release, blocked) = mpsc::channel();
        let (_, mut pending) = prepare_pending_world_build(
            super::super::WorldPipeline::LegacyFoundation,
            crate::engine::MemoryStageCache::new(),
            receiver,
            (old_token, crate::engine::BuildCancellation::new()),
            false,
        );
        pending.worker = Some(std::thread::spawn(move || {
            sender
                .send(WorldBuildCompletion {
                    token: old_token,
                    result: Err("old worker exit".into()),
                    stage_cache: crate::engine::MemoryStageCache::new(),
                    formation_surface: None,
                    amplified: None,
                })
                .unwrap();
            blocked.recv().unwrap();
        }));
        app.world_build = Some(pending);
        let mut loaded = loaded(&app);
        loaded.file.inputs.spherical_space_spec.target_cell_count = 162;
        loaded.file.generator = current_generator_identity(&loaded.file.inputs).unwrap();
        app.receive_open_project(PathBuf::from("C:/new.sekai"), loaded);
        let epoch = app.authoring.capture_save().epoch;
        let ctx = egui::Context::default();
        for _ in 0..2 {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                eframe::App::update(&mut app, ctx, &mut eframe::Frame::_new_kittest());
            });
        }
        assert_eq!(app.world_build.as_ref().unwrap().token, old_token);
        assert_eq!(app.pending_open_build.unwrap().epoch, epoch);
        assert!(app
            .world_build
            .as_ref()
            .unwrap()
            .cancellation
            .is_cancelled());
        release.send(()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(4);
        while app
            .world_build
            .as_ref()
            .is_some_and(|build| build.token.epoch == old_token.epoch)
        {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                eframe::App::update(&mut app, ctx, &mut eframe::Frame::_new_kittest());
            });
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        let token = app.world_build.as_ref().unwrap().token;
        assert_eq!(token.epoch, epoch);
        assert!(app.pending_open_build.is_none());
        assert!(!app.try_start_open_build());
        assert_eq!(app.world_build.as_ref().unwrap().token, token);
        app.dispatch_project_action(ProjectAction::CancelBuild, &ctx);
        while app.world_build.is_some() {
            let _ = ctx.run(egui::RawInput::default(), |ctx| app.poll_world_build(ctx));
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert!(!app.try_start_open_build());
        assert!(app.spherical_presentation.read_resource(Option::is_none));
    }
}
