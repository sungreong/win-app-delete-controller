use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, Label, RichText, ScrollArea, TextEdit, Vec2};

use crate::installed_app::InstalledApp;
use crate::scanner::scan_installed_apps;
use crate::settings::AppSettings;
use crate::uninstaller::{
    UninstallEvent, UninstallReadiness, UninstallTarget, assess_uninstall_command,
    spawn_elevated_uninstall, spawn_uninstall_queue,
};

const ROW_HEIGHT: f32 = 32.0;
const SELECT_WIDTH: f32 = 46.0;
const MIN_APP_WIDTH: f32 = 110.0;
const MIN_PUBLISHER_WIDTH: f32 = 100.0;
const MIN_VERSION_WIDTH: f32 = 78.0;
const MAX_VERSION_WIDTH: f32 = 120.0;
const MIN_INFO_WIDTH: f32 = 118.0;
const MAX_INFO_WIDTH: f32 = 190.0;
const MIN_TABLE_WIDTH: f32 = 420.0;
const VERIFY_INTERVAL: Duration = Duration::from_secs(4);
const VERIFY_WINDOW: Duration = Duration::from_secs(90);
const OUTER_MARGIN_X: i8 = 14;
const OUTER_MARGIN_Y: i8 = 10;
const PAGINATION_RESERVED_HEIGHT: f32 = 42.0;
const DETAILS_RESERVED_HEIGHT: f32 = 86.0;
const JOBS_RESERVED_HEIGHT: f32 = 92.0;
const FILTER_ALL_PUBLISHERS: &str = "";
const FILTER_ALL_READINESS: &str = "all";
const FILTER_SELECTABLE: &str = "selectable";
const FILTER_VERIFIED: &str = "verified";
const FILTER_SHELL: &str = "shell";
const FILTER_UNSUPPORTED: &str = "unsupported";
const FILTER_BLOCKED: &str = "blocked";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SortKey {
    Name,
    Publisher,
    Version,
    Size,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ColumnBoundary {
    AppPublisher,
    PublisherVersion,
    VersionInfo,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JobPhase {
    Launching,
    Running,
    Verifying,
    Failed,
    VerifiedRemoved,
    StillListed,
}

#[derive(Clone, Debug)]
struct ActiveUninstall {
    app_id: String,
    name: String,
    status: String,
    phase: JobPhase,
    pid: Option<u32>,
    exit_code: Option<i32>,
    verify_until: Option<Instant>,
    next_verify_at: Option<Instant>,
    verify_checks: u32,
    elevation_required: bool,
    retry_target: Option<UninstallTarget>,
}

#[derive(Clone, Copy, Debug)]
struct TableLayout {
    total: f32,
    app: f32,
    publisher: f32,
    version: f32,
    info: f32,
}

#[derive(Clone, Copy, Debug)]
struct ColumnWidths {
    app: f32,
    publisher: f32,
    version: f32,
    info: f32,
}

pub fn configure_visuals(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = Color32::from_rgb(24, 26, 29);
    visuals.window_fill = Color32::from_rgb(28, 30, 34);
    visuals.extreme_bg_color = Color32::from_rgb(18, 20, 23);
    visuals.faint_bg_color = Color32::from_rgb(35, 38, 43);
    visuals.override_text_color = Some(Color32::from_rgb(222, 225, 230));
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(42, 45, 51);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(58, 63, 72);
    visuals.widgets.active.bg_fill = Color32::from_rgb(70, 92, 120);
    ctx.set_visuals(visuals);

    let mut style = (*ctx.global_style()).clone();
    style.spacing.item_spacing = Vec2::new(8.0, 6.0);
    style.spacing.button_padding = Vec2::new(8.0, 4.0);
    ctx.set_global_style(style);
}

pub struct DeleteControllerApp {
    apps: Vec<InstalledApp>,
    assessments: HashMap<String, crate::uninstaller::UninstallAssessment>,
    publishers: Vec<String>,
    selected_ids: HashSet<String>,
    settings: AppSettings,
    saved_settings: AppSettings,
    settings_error: Option<String>,
    scan_rx: Option<Receiver<Result<Vec<InstalledApp>, String>>>,
    uninstall_tx: Sender<UninstallEvent>,
    uninstall_rx: Receiver<UninstallEvent>,
    confirm_targets: Option<Vec<UninstallTarget>>,
    status: String,
    is_scanning: bool,
    active_launch_batches: usize,
    jobs: Vec<ActiveUninstall>,
    page_index: usize,
    sort_key: SortKey,
    sort_descending: bool,
    column_widths: Option<ColumnWidths>,
}

impl DeleteControllerApp {
    pub fn new() -> Self {
        let (uninstall_tx, uninstall_rx) = mpsc::channel();
        let settings = AppSettings::load();
        let mut app = Self {
            apps: Vec::new(),
            assessments: HashMap::new(),
            publishers: Vec::new(),
            selected_ids: HashSet::new(),
            saved_settings: settings.clone(),
            settings,
            settings_error: None,
            scan_rx: None,
            uninstall_tx,
            uninstall_rx,
            confirm_targets: None,
            status: "설치된 앱을 불러오는 중입니다.".to_owned(),
            is_scanning: false,
            active_launch_batches: 0,
            jobs: Vec::new(),
            page_index: 0,
            sort_key: SortKey::Name,
            sort_descending: false,
            column_widths: None,
        };
        app.start_scan();
        app
    }

    fn start_scan(&mut self) {
        let (tx, rx) = mpsc::channel();
        self.scan_rx = Some(rx);
        self.is_scanning = true;
        self.status = "설치된 앱을 불러오는 중입니다.".to_owned();

        thread::spawn(move || {
            let _ = tx.send(scan_installed_apps());
        });
    }

    fn drain_scan(&mut self) {
        let Some(rx) = &self.scan_rx else {
            return;
        };

        let Ok(result) = rx.try_recv() else {
            return;
        };

        self.is_scanning = false;
        self.scan_rx = None;

        match result {
            Ok(apps) => {
                self.apps = apps;
                self.rebuild_cached_filters();
                self.selected_ids
                    .retain(|id| self.apps.iter().any(|app| &app.id == id));
                self.refresh_job_verification();
                self.status = format!("{}개의 제거 가능한 앱을 찾았습니다.", self.apps.len());
            }
            Err(error) => {
                self.status = format!("앱 목록을 불러오지 못했습니다: {error}");
            }
        }
    }

    fn rebuild_cached_filters(&mut self) {
        self.assessments = self
            .apps
            .iter()
            .map(|app| {
                (
                    app.id.clone(),
                    assess_uninstall_command(&app.uninstall_string),
                )
            })
            .collect();

        self.publishers = self
            .apps
            .iter()
            .filter_map(|app| app.publisher.as_ref())
            .filter(|publisher| !publisher.trim().is_empty())
            .cloned()
            .collect();
        self.publishers
            .sort_by_key(|publisher| publisher.to_lowercase());
        self.publishers
            .dedup_by(|left, right| left.eq_ignore_ascii_case(right));

        if !self.settings.publisher_filter.is_empty()
            && !self
                .publishers
                .iter()
                .any(|publisher| publisher == &self.settings.publisher_filter)
        {
            self.settings.publisher_filter.clear();
        }
    }

    fn drain_uninstaller(&mut self) {
        while let Ok(event) = self.uninstall_rx.try_recv() {
            match event {
                UninstallEvent::Launching {
                    app_id,
                    index,
                    total,
                    name,
                } => {
                    self.status = format!("[{index}/{total}] {name} 제거 창을 여는 중입니다.");
                    self.upsert_job(
                        app_id,
                        name,
                        JobPhase::Launching,
                        "제거 명령 시작 준비 중".to_owned(),
                        None,
                        None,
                        false,
                        None,
                    );
                }
                UninstallEvent::Launched { app_id, name, pid } => {
                    self.status = format!("{name} 제거 프로세스 실행 중입니다. PID {pid}");
                    self.upsert_job(
                        app_id,
                        name,
                        JobPhase::Running,
                        format!("실행 중 - PID {pid}"),
                        Some(pid),
                        None,
                        false,
                        None,
                    );
                }
                UninstallEvent::Exited { app_id, name, code } => {
                    let code_text = code
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "알 수 없음".to_owned());
                    self.status = format!(
                        "{name} 제거 명령 프로세스가 종료되었습니다. 삭제 여부를 자동 확인합니다."
                    );
                    self.upsert_job(
                        app_id,
                        name,
                        JobPhase::Verifying,
                        format!(
                            "명령 프로세스 종료 - 코드 {code_text}, 목록 제거 여부 자동 확인 중"
                        ),
                        None,
                        code,
                        false,
                        None,
                    );
                    if !self.is_scanning {
                        self.start_scan();
                    }
                }
                UninstallEvent::Failed {
                    app_id,
                    name,
                    message,
                    elevation_required,
                    target,
                } => {
                    self.status = if elevation_required {
                        format!("{name} 제거 명령에 관리자 권한이 필요합니다.")
                    } else {
                        format!("{name} 제거 명령 실행 실패")
                    };
                    self.upsert_job(
                        app_id,
                        name,
                        JobPhase::Failed,
                        if elevation_required {
                            format!("관리자 권한 필요 - {message}")
                        } else {
                            format!("실패 - {message}")
                        },
                        None,
                        None,
                        elevation_required,
                        target,
                    );
                }
                UninstallEvent::Done => {
                    self.active_launch_batches = self.active_launch_batches.saturating_sub(1);
                    self.status = "제거 명령 실행 큐가 끝났습니다. 삭제 여부는 작업 상태에서 자동 확인됩니다."
                        .to_owned();
                }
            }
        }
    }

    fn upsert_job(
        &mut self,
        app_id: String,
        name: String,
        phase: JobPhase,
        status: String,
        pid: Option<u32>,
        exit_code: Option<i32>,
        elevation_required: bool,
        retry_target: Option<UninstallTarget>,
    ) {
        if let Some(job) = self.jobs.iter_mut().find(|job| job.app_id == app_id) {
            job.name = name;
            job.phase = phase;
            job.status = status;
            if pid.is_some() {
                job.pid = pid;
            }
            if exit_code.is_some() {
                job.exit_code = exit_code;
            }
            if phase == JobPhase::Verifying {
                let now = Instant::now();
                job.verify_until = Some(now + VERIFY_WINDOW);
                job.next_verify_at = Some(now);
                job.verify_checks = 0;
            }
            job.elevation_required = elevation_required;
            if retry_target.is_some() {
                job.retry_target = retry_target;
            }
        } else {
            let now = Instant::now();
            let (verify_until, next_verify_at) = if phase == JobPhase::Verifying {
                (Some(now + VERIFY_WINDOW), Some(now))
            } else {
                (None, None)
            };
            self.jobs.push(ActiveUninstall {
                app_id,
                name,
                status,
                phase,
                pid,
                exit_code,
                verify_until,
                next_verify_at,
                verify_checks: 0,
                elevation_required,
                retry_target,
            });
        }

        if self.jobs.len() > 12 {
            self.jobs.remove(0);
        }
    }

    fn refresh_job_verification(&mut self) {
        let now = Instant::now();
        for job in &mut self.jobs {
            if !matches!(job.phase, JobPhase::Verifying | JobPhase::StillListed) {
                continue;
            }

            let still_listed = self.apps.iter().any(|app| app.id == job.app_id);
            if still_listed {
                job.verify_checks += 1;
                if job.verify_until.is_some_and(|until| now <= until) {
                    job.phase = JobPhase::Verifying;
                    job.next_verify_at = Some(now + VERIFY_INTERVAL);
                    job.status = format!(
                        "아직 목록에 있음 - 제거 창 진행 중일 수 있어 자동 재확인 중 ({}회)",
                        job.verify_checks
                    );
                } else {
                    job.phase = JobPhase::StillListed;
                    job.next_verify_at = None;
                    job.status =
                        "제거 완료를 확인하지 못함 - Windows 목록에 아직 남아 있음".to_owned();
                }
            } else {
                job.phase = JobPhase::VerifiedRemoved;
                job.next_verify_at = None;
                job.status = "목록에서 제거 확인".to_owned();
            }
        }
    }

    fn schedule_verification_scans(&mut self) {
        if self.is_scanning {
            return;
        }

        let now = Instant::now();
        let should_scan = self.jobs.iter().any(|job| {
            job.phase == JobPhase::Verifying && job.next_verify_at.is_some_and(|next| now >= next)
        });

        if should_scan {
            self.start_scan();
        }
    }

    fn visible_indices(&self) -> Vec<usize> {
        let terms = self
            .settings
            .query
            .to_lowercase()
            .split_whitespace()
            .map(str::to_owned)
            .collect::<Vec<_>>();

        let mut indices = self
            .apps
            .iter()
            .enumerate()
            .filter_map(|(index, app)| {
                let allowed_system =
                    self.settings.include_system_components || !app.is_system_component;
                let allowed_remove = self.settings.show_no_remove || !app.no_remove;
                let allowed_publisher = self.settings.publisher_filter.is_empty()
                    || app.publisher.as_deref() == Some(self.settings.publisher_filter.as_str());
                let allowed_readiness = self.matches_readiness_filter(app);
                let allowed =
                    allowed_system && allowed_remove && allowed_publisher && allowed_readiness;
                (allowed && app.matches_terms(&terms)).then_some(index)
            })
            .collect::<Vec<_>>();

        self.sort_indices(&mut indices);
        indices
    }

    fn selected_targets(&self) -> Vec<UninstallTarget> {
        self.apps
            .iter()
            .filter(|app| {
                self.selected_ids.contains(&app.id)
                    && !app.no_remove
                    && self.assessment_for(app).is_selectable()
            })
            .map(UninstallTarget::from)
            .collect()
    }

    fn assessment_for(&self, app: &InstalledApp) -> crate::uninstaller::UninstallAssessment {
        self.assessments
            .get(&app.id)
            .cloned()
            .unwrap_or_else(|| assess_uninstall_command(&app.uninstall_string))
    }

    fn matches_readiness_filter(&self, app: &InstalledApp) -> bool {
        let assessment = self.assessment_for(app);
        match self.settings.uninstall_filter.as_str() {
            FILTER_ALL_READINESS => true,
            FILTER_SELECTABLE => !app.no_remove && assessment.is_selectable(),
            FILTER_VERIFIED => !app.no_remove && assessment.status == UninstallReadiness::Verified,
            FILTER_SHELL => !app.no_remove && assessment.status == UninstallReadiness::NeedsShell,
            FILTER_UNSUPPORTED => assessment.status == UninstallReadiness::Unsupported,
            FILTER_BLOCKED => app.no_remove,
            _ => !app.no_remove && assessment.is_selectable(),
        }
    }

    fn sort_indices(&self, indices: &mut [usize]) {
        indices.sort_by(|left, right| {
            let left_app = &self.apps[*left];
            let right_app = &self.apps[*right];
            let ordering = match self.sort_key {
                SortKey::Name => compare_text(&left_app.display_name, &right_app.display_name),
                SortKey::Publisher => {
                    compare_text(left_app.publisher_text(), right_app.publisher_text())
                }
                SortKey::Version => compare_text(left_app.version_text(), right_app.version_text()),
                SortKey::Size => {
                    compare_option_u32(left_app.estimated_size_kb, right_app.estimated_size_kb)
                }
            }
            .then_with(|| compare_text(&left_app.display_name, &right_app.display_name));

            if self.sort_descending {
                ordering.reverse()
            } else {
                ordering
            }
        });
    }

    fn start_uninstall(&mut self, targets: Vec<UninstallTarget>) {
        for target in &targets {
            self.selected_ids.remove(&target.app_id);
        }

        self.active_launch_batches += 1;
        self.status = format!(
            "{}개의 제거 창 실행을 백그라운드로 요청합니다.",
            targets.len()
        );
        spawn_uninstall_queue(targets, self.uninstall_tx.clone());
    }

    fn start_elevated_uninstall(&mut self, target: UninstallTarget) {
        self.selected_ids.remove(&target.app_id);
        self.active_launch_batches += 1;
        self.status = format!(
            "{} 제거 명령을 관리자 권한으로 다시 실행합니다.",
            target.name
        );
        spawn_elevated_uninstall(target, self.uninstall_tx.clone());
    }

    fn save_settings_if_changed(&mut self) {
        if self.settings == self.saved_settings {
            return;
        }

        match self.settings.save() {
            Ok(()) => {
                self.saved_settings = self.settings.clone();
                self.settings_error = None;
            }
            Err(error) => {
                self.settings_error = Some(format!("설정 저장 실패: {error}"));
            }
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui, visible_count: usize) {
        let hidden_system_count = self
            .apps
            .iter()
            .filter(|app| app.is_system_component)
            .count();
        let blocked_count = self.apps.iter().filter(|app| app.no_remove).count();
        let runnable_count = self
            .apps
            .iter()
            .filter(|app| !app.no_remove && self.assessment_for(app).is_selectable())
            .count();
        let unsupported_count = self
            .apps
            .iter()
            .filter(|app| self.assessment_for(app).status == UninstallReadiness::Unsupported)
            .count();

        ui.horizontal_wrapped(|ui| {
            ui.label("검색");
            ui.add_sized(
                [320.0, 28.0],
                TextEdit::singleline(&mut self.settings.query)
                    .hint_text("앱 이름, 게시자, 버전, 위치, 날짜"),
            );

            if ui
                .add_enabled(!self.is_scanning, egui::Button::new("새로고침"))
                .clicked()
            {
                self.start_scan();
            }

            let targets = self.selected_targets();
            if ui
                .add_enabled(!targets.is_empty(), egui::Button::new("선택 제거"))
                .clicked()
            {
                self.confirm_targets = Some(targets);
            }
        });

        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_label("게시자")
                .selected_text(if self.settings.publisher_filter.is_empty() {
                    "전체"
                } else {
                    self.settings.publisher_filter.as_str()
                })
                .width(220.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut self.settings.publisher_filter,
                        FILTER_ALL_PUBLISHERS.to_owned(),
                        "전체",
                    );
                    for publisher in &self.publishers {
                        ui.selectable_value(
                            &mut self.settings.publisher_filter,
                            publisher.clone(),
                            publisher,
                        );
                    }
                });

            egui::ComboBox::from_label("삭제 상태")
                .selected_text(readiness_filter_label(&self.settings.uninstall_filter))
                .width(150.0)
                .show_ui(ui, |ui| {
                    for (value, label) in readiness_filter_options() {
                        ui.selectable_value(
                            &mut self.settings.uninstall_filter,
                            value.to_owned(),
                            label,
                        );
                    }
                });

            egui::ComboBox::from_label("페이지")
                .selected_text(format!("{}개", self.settings.page_size))
                .width(90.0)
                .show_ui(ui, |ui| {
                    for size in [25, 50, 100, 200] {
                        ui.selectable_value(
                            &mut self.settings.page_size,
                            size,
                            format!("{size}개"),
                        );
                    }
                });

            ui.checkbox(
                &mut self.settings.include_system_components,
                "숨김 항목 포함",
            );
            ui.checkbox(&mut self.settings.show_no_remove, "제거 제한 항목 표시");
            ui.checkbox(&mut self.settings.show_details, "선택 상세");
        });

        ui.horizontal_wrapped(|ui| {
            ui.label(format!(
                "전체 {}개 / 표시 {visible_count}개 / 선택 {}개 / 제거 가능 {runnable_count}개",
                self.apps.len(),
                self.selected_ids.len()
            ));
            ui.separator();
            ui.label(format!(
                "숨김 {hidden_system_count}개 / 제거 제한 {blocked_count}개 / 실행 불가 {unsupported_count}개"
            ));

            if let Some(error) = &self.settings_error {
                ui.separator();
                ui.colored_label(Color32::LIGHT_RED, error);
            }
        });
    }

    fn selection_bar(&mut self, ui: &mut egui::Ui, visible_indices: &[usize]) {
        ui.horizontal(|ui| {
            if ui.button("현재 페이지 선택").clicked() {
                for index in visible_indices {
                    let app = &self.apps[*index];
                    if !app.no_remove && self.assessment_for(app).is_selectable() {
                        self.selected_ids.insert(app.id.clone());
                    }
                }
            }

            if ui.button("선택 해제").clicked() {
                self.selected_ids.clear();
            }

            if self.is_scanning {
                ui.spinner();
            }

            ui.label(&self.status);
        });
    }

    fn clamp_page(&mut self, total_count: usize) {
        let page_count = page_count(total_count, self.settings.page_size);
        if self.page_index >= page_count {
            self.page_index = page_count.saturating_sub(1);
        }
    }

    fn paged_indices(&self, visible_indices: &[usize]) -> Vec<usize> {
        let start = self.page_index * self.settings.page_size;
        let end = (start + self.settings.page_size).min(visible_indices.len());
        if start >= visible_indices.len() {
            Vec::new()
        } else {
            visible_indices[start..end].to_vec()
        }
    }

    fn pagination_bar(&mut self, ui: &mut egui::Ui, total_count: usize) {
        let page_count = page_count(total_count, self.settings.page_size);
        let start = if total_count == 0 {
            0
        } else {
            self.page_index * self.settings.page_size + 1
        };
        let end = ((self.page_index + 1) * self.settings.page_size).min(total_count);

        ui.horizontal_wrapped(|ui| {
            ui.label(format!(
                "{start}-{end} / {total_count}개, {} / {page_count} 페이지",
                self.page_index + 1
            ));
            if ui
                .add_enabled(self.page_index > 0, egui::Button::new("처음"))
                .clicked()
            {
                self.page_index = 0;
            }
            if ui
                .add_enabled(self.page_index > 0, egui::Button::new("이전"))
                .clicked()
            {
                self.page_index = self.page_index.saturating_sub(1);
            }
            if ui
                .add_enabled(self.page_index + 1 < page_count, egui::Button::new("다음"))
                .clicked()
            {
                self.page_index += 1;
            }
            if ui
                .add_enabled(self.page_index + 1 < page_count, egui::Button::new("끝"))
                .clicked()
            {
                self.page_index = page_count.saturating_sub(1);
            }
        });
    }

    fn apps_table(&mut self, ui: &mut egui::Ui, visible_indices: &[usize]) {
        let layout = self.table_layout(ui.available_width(), ui.spacing().item_spacing.x);
        self.table_header(ui, layout);

        let job_reserved_height = if self.jobs.is_empty() {
            0.0
        } else {
            JOBS_RESERVED_HEIGHT
        };
        let detail_reserved_height = if self.settings.show_details && !self.selected_ids.is_empty()
        {
            DETAILS_RESERVED_HEIGHT
        } else {
            0.0
        };
        let table_height = (ui.available_height()
            - PAGINATION_RESERVED_HEIGHT
            - job_reserved_height
            - detail_reserved_height)
            .max(180.0);

        ScrollArea::vertical()
            .id_salt("apps_table_scroll")
            .auto_shrink([false; 2])
            .max_height(table_height)
            .show(ui, |ui| {
                egui::Grid::new("apps_table_grid")
                    .num_columns(4)
                    .spacing([ui.spacing().item_spacing.x, 0.0])
                    .min_col_width(0.0)
                    .show(ui, |ui| {
                        for index in visible_indices {
                            self.app_row(ui, *index, layout);
                            ui.end_row();
                        }
                    });
            });
    }

    fn table_layout(&mut self, available_width: f32, item_spacing: f32) -> TableLayout {
        let total = available_width.max(MIN_TABLE_WIDTH);
        let gaps = item_spacing * 4.0;
        let content = (total - gaps).max(0.0);
        let remaining = (content - SELECT_WIDTH).max(0.0);

        let default_widths = default_column_widths(remaining);
        let mut widths = self.column_widths.unwrap_or(default_widths);
        widths = fit_column_widths(widths, remaining);
        self.column_widths = Some(widths);

        TableLayout {
            total,
            app: widths.app,
            publisher: widths.publisher,
            version: widths.version,
            info: widths.info,
        }
    }

    fn table_header(&mut self, ui: &mut egui::Ui, layout: TableLayout) {
        ui.horizontal(|ui| {
            ui.set_width(layout.total);
            Self::header_label(ui, SELECT_WIDTH, "선택");
            let app = self.header_sort_button(ui, layout.app, "앱", SortKey::Name);
            self.column_resize_handle(ui, app.rect, ColumnBoundary::AppPublisher);
            let publisher =
                self.header_sort_button(ui, layout.publisher, "게시자", SortKey::Publisher);
            self.column_resize_handle(ui, publisher.rect, ColumnBoundary::PublisherVersion);
            let version = self.header_sort_button(ui, layout.version, "버전", SortKey::Version);
            self.column_resize_handle(ui, version.rect, ColumnBoundary::VersionInfo);
            self.header_sort_button(ui, layout.info, "크기/설치일", SortKey::Size);
        });
        ui.separator();
    }

    fn app_row(&mut self, ui: &mut egui::Ui, index: usize, layout: TableLayout) {
        let app = &self.apps[index];
        let app_id = app.id.clone();
        let display_name = app.display_name.clone();
        let uninstall_string = app.uninstall_string.clone();
        let publisher = app.publisher_text().to_owned();
        let version = app.version_text().to_owned();
        let location = app.location_text().to_owned();
        let size = app.size_text();
        let install_date = app.install_date_text();
        let info = app.info_text();
        let assessment = self.assessment_for(app);
        let source_hive = app.source_hive.clone();
        let registry_path = app.registry_path.clone();
        let can_select = !app.no_remove && assessment.is_selectable();

        let mut selected = self.selected_ids.contains(&app_id);
        let response = ui.add_enabled_ui(can_select, |ui| {
            ui.add_sized(
                [SELECT_WIDTH, ROW_HEIGHT],
                egui::Checkbox::without_text(&mut selected),
            )
        });

        if response.inner.changed() {
            if selected {
                self.selected_ids.insert(app_id.clone());
            } else {
                self.selected_ids.remove(&app_id);
            }
        }

        let name = if !can_select {
            RichText::new(display_name.clone()).weak()
        } else {
            RichText::new(display_name.clone())
        };
        let app_hover = format!(
            "{display_name}\n검증: {} - {}\n위치: {location}\n원본: {source_hive} - {registry_path}\n명령: {uninstall_string}",
            assessment.label(),
            assessment.detail
        );
        Self::rich_text_cell(ui, layout.app, name, &app_hover);
        Self::text_cell(ui, layout.publisher, &publisher, &publisher);
        Self::text_cell(ui, layout.version, &version, &version);
        Self::text_cell(
            ui,
            layout.info,
            &info,
            &format!(
                "검증: {} - {}\n크기: {size}\n설치일: {install_date}\n원본: {source_hive}\n위치: {location}",
                assessment.label(),
                assessment.detail
            ),
        );
    }

    fn header_label(ui: &mut egui::Ui, width: f32, text: &str) {
        ui.add_sized(
            [width, ROW_HEIGHT],
            Label::new(RichText::new(text).strong()).truncate(),
        );
    }

    fn header_sort_button(
        &mut self,
        ui: &mut egui::Ui,
        width: f32,
        text: &str,
        sort_key: SortKey,
    ) -> egui::Response {
        let indicator = if self.sort_key == sort_key {
            if self.sort_descending { " v" } else { " ^" }
        } else {
            ""
        };

        let response = ui.add_sized(
            [width, ROW_HEIGHT],
            egui::Button::new(RichText::new(format!("{text}{indicator}")).strong()),
        );

        if response.clicked() {
            if self.sort_key == sort_key {
                self.sort_descending = !self.sort_descending;
            } else {
                self.sort_key = sort_key;
                self.sort_descending = matches!(sort_key, SortKey::Size);
            }
            self.page_index = 0;
        }

        response
    }

    fn column_resize_handle(
        &mut self,
        ui: &mut egui::Ui,
        left_rect: egui::Rect,
        boundary: ColumnBoundary,
    ) {
        let handle_rect = egui::Rect::from_min_max(
            egui::pos2(left_rect.right() - 4.0, left_rect.top()),
            egui::pos2(left_rect.right() + 4.0, left_rect.bottom()),
        );
        let id = ui.make_persistent_id(("column_resize", boundary));
        let response = ui.interact(handle_rect, id, egui::Sense::drag());

        if response.hovered() || response.dragged() {
            ui.output_mut(|output| {
                output.cursor_icon = egui::CursorIcon::ResizeHorizontal;
            });
        }

        if response.dragged() {
            let delta = ui.input(|input| input.pointer.delta().x);
            if delta.abs() > 0.0 {
                self.adjust_column_width(boundary, delta);
            }
        }
    }

    fn adjust_column_width(&mut self, boundary: ColumnBoundary, delta: f32) {
        let Some(mut widths) = self.column_widths else {
            return;
        };

        match boundary {
            ColumnBoundary::AppPublisher => {
                let applied = clamp_pair_delta(
                    widths.app,
                    widths.publisher,
                    delta,
                    MIN_APP_WIDTH,
                    MIN_PUBLISHER_WIDTH,
                );
                widths.app += applied;
                widths.publisher -= applied;
            }
            ColumnBoundary::PublisherVersion => {
                let applied = clamp_pair_delta(
                    widths.publisher,
                    widths.version,
                    delta,
                    MIN_PUBLISHER_WIDTH,
                    MIN_VERSION_WIDTH,
                );
                widths.publisher += applied;
                widths.version -= applied;
            }
            ColumnBoundary::VersionInfo => {
                let applied = clamp_pair_delta(
                    widths.version,
                    widths.info,
                    delta,
                    MIN_VERSION_WIDTH,
                    MIN_INFO_WIDTH,
                );
                widths.version += applied;
                widths.info -= applied;
            }
        }

        self.column_widths = Some(widths);
    }

    fn text_cell(ui: &mut egui::Ui, width: f32, text: &str, hover: &str) {
        ui.add_sized([width, ROW_HEIGHT], Label::new(text).truncate())
            .on_hover_text(hover);
    }

    fn rich_text_cell(ui: &mut egui::Ui, width: f32, text: RichText, hover: &str) {
        ui.add_sized([width, ROW_HEIGHT], Label::new(text).truncate())
            .on_hover_text(hover);
    }

    fn confirm_window(&mut self, ctx: &egui::Context) {
        let Some(targets) = self.confirm_targets.clone() else {
            return;
        };

        egui::Window::new("선택한 앱 제거")
            .collapsible(false)
            .resizable(true)
            .default_width(520.0)
            .show(ctx, |ui| {
                ui.label(format!(
                    "{}개의 Windows 제거 창 실행을 백그라운드로 요청합니다.",
                    targets.len()
                ));
                ui.label(
                    "제거 창이 떠 있어도 이 앱에서 검색, 선택, 추가 제거를 계속할 수 있습니다.",
                );
                ui.separator();

                ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                    for target in &targets {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&target.name).strong());
                            ui.label(target.launch_mode.label());
                            ui.label(target.assessment.label());
                        });
                        ui.label(RichText::new(&target.assessment.detail).small());
                        if target.raw_command != target.command {
                            ui.label(
                                RichText::new("MSI 설치 명령을 제거 명령으로 변환했습니다.")
                                    .small(),
                            );
                        }
                        ui.monospace(&target.command)
                            .on_hover_text(&target.raw_command);
                        ui.add_space(6.0);
                    }
                });

                ui.horizontal(|ui| {
                    if ui.button("실행").clicked() {
                        self.confirm_targets = None;
                        self.start_uninstall(targets.clone());
                    }

                    if ui.button("취소").clicked() {
                        self.confirm_targets = None;
                    }
                });
            });
    }

    fn selected_details(&self, ui: &mut egui::Ui) {
        if !self.settings.show_details || self.selected_ids.is_empty() {
            return;
        }

        ui.separator();
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("선택 상세").strong());
            ui.label(format!("{}개 선택", self.selected_ids.len()));
        });

        let selected_apps = self
            .apps
            .iter()
            .filter(|app| self.selected_ids.contains(&app.id))
            .take(3)
            .collect::<Vec<_>>();

        for app in selected_apps {
            ui.horizontal_wrapped(|ui| {
                ui.add_sized([300.0, 22.0], Label::new(&app.display_name).truncate());
                ui.add_sized([220.0, 22.0], Label::new(app.publisher_text()).truncate());
                ui.add_sized([180.0, 22.0], Label::new(app.info_text()).truncate());
                ui.add_sized(
                    [220.0, 22.0],
                    Label::new(self.assessment_for(app).label()).truncate(),
                )
                .on_hover_text(&app.uninstall_string);
            });
        }

        let hidden_count = self.selected_ids.len().saturating_sub(3);
        if hidden_count > 0 {
            ui.label(
                RichText::new(format!(
                    "외 {hidden_count}개는 제거 확인 창에서 표시됩니다."
                ))
                .weak(),
            );
        }
    }

    fn bottom_jobs(&mut self, ui: &mut egui::Ui) {
        if self.jobs.is_empty() {
            return;
        }

        let mut elevated_retry = None;

        ui.separator();
        ui.horizontal(|ui| {
            ui.label(RichText::new("작업 상태").strong());
            if ui.button("완료 항목 지우기").clicked() {
                self.jobs.retain(|job| {
                    matches!(
                        job.phase,
                        JobPhase::Launching | JobPhase::Running | JobPhase::Verifying
                    )
                });
            }
        });

        ScrollArea::vertical().max_height(78.0).show(ui, |ui| {
            for job in self.jobs.iter().rev().take(3) {
                ui.horizontal(|ui| {
                    let color = match job.phase {
                        JobPhase::Launching => Color32::LIGHT_BLUE,
                        JobPhase::Running => Color32::YELLOW,
                        JobPhase::Verifying => Color32::LIGHT_BLUE,
                        JobPhase::StillListed => Color32::LIGHT_GRAY,
                        JobPhase::Failed => Color32::LIGHT_RED,
                        JobPhase::VerifiedRemoved => Color32::LIGHT_GREEN,
                    };
                    ui.colored_label(color, "●");
                    ui.add_sized([260.0, 24.0], Label::new(&job.name).truncate());
                    ui.add_sized([520.0, 24.0], Label::new(&job.status).truncate())
                        .on_hover_text(&job.status);
                    if job.elevation_required
                        && let Some(target) = &job.retry_target
                        && ui.button("관리자 권한 재시도").clicked()
                    {
                        elevated_retry = Some(target.clone());
                    }
                });
            }
        });

        if let Some(target) = elevated_retry {
            self.start_elevated_uninstall(target);
        }
    }
}

fn compare_text(left: &str, right: &str) -> Ordering {
    left.to_lowercase().cmp(&right.to_lowercase())
}

fn compare_option_u32(left: Option<u32>, right: Option<u32>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left.cmp(&right),
        (Some(_), None) => Ordering::Greater,
        (None, Some(_)) => Ordering::Less,
        (None, None) => Ordering::Equal,
    }
}

fn default_column_widths(available: f32) -> ColumnWidths {
    let version = (available * 0.13).clamp(MIN_VERSION_WIDTH, MAX_VERSION_WIDTH);
    let info = (available * 0.20).clamp(MIN_INFO_WIDTH, MAX_INFO_WIDTH);
    let flexible = (available - version - info).max(MIN_APP_WIDTH + MIN_PUBLISHER_WIDTH);
    ColumnWidths {
        app: flexible * 0.58,
        publisher: flexible * 0.42,
        version,
        info,
    }
}

fn fit_column_widths(widths: ColumnWidths, available: f32) -> ColumnWidths {
    let min_total = MIN_APP_WIDTH + MIN_PUBLISHER_WIDTH + MIN_VERSION_WIDTH + MIN_INFO_WIDTH;
    if available <= min_total {
        let scale = (available / min_total).max(0.5);
        return ColumnWidths {
            app: MIN_APP_WIDTH * scale,
            publisher: MIN_PUBLISHER_WIDTH * scale,
            version: MIN_VERSION_WIDTH * scale,
            info: MIN_INFO_WIDTH * scale,
        };
    }

    let mut fitted = ColumnWidths {
        app: widths.app.max(MIN_APP_WIDTH),
        publisher: widths.publisher.max(MIN_PUBLISHER_WIDTH),
        version: widths.version.max(MIN_VERSION_WIDTH),
        info: widths.info.max(MIN_INFO_WIDTH),
    };
    let current_total = fitted.app + fitted.publisher + fitted.version + fitted.info;
    let diff = available - current_total;
    fitted.app = (fitted.app + diff).max(MIN_APP_WIDTH);

    let overflow = fitted.app + fitted.publisher + fitted.version + fitted.info - available;
    if overflow > 0.0 {
        fitted.publisher = (fitted.publisher - overflow).max(MIN_PUBLISHER_WIDTH);
    }

    fitted
}

fn clamp_pair_delta(left: f32, right: f32, delta: f32, left_min: f32, right_min: f32) -> f32 {
    let min_delta = left_min - left;
    let max_delta = right - right_min;
    delta.clamp(min_delta, max_delta)
}

fn page_count(total_count: usize, page_size: usize) -> usize {
    total_count.div_ceil(page_size.max(1)).max(1)
}

fn readiness_filter_options() -> [(&'static str, &'static str); 6] {
    [
        (FILTER_SELECTABLE, "삭제 가능"),
        (FILTER_VERIFIED, "검증됨"),
        (FILTER_SHELL, "셸 검증 제한"),
        (FILTER_UNSUPPORTED, "실행 불가"),
        (FILTER_BLOCKED, "제거 제한"),
        (FILTER_ALL_READINESS, "전체"),
    ]
}

fn readiness_filter_label(value: &str) -> &'static str {
    readiness_filter_options()
        .iter()
        .find_map(|(candidate, label)| (*candidate == value).then_some(*label))
        .unwrap_or("삭제 가능")
}

impl eframe::App for DeleteControllerApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        egui::Frame::NONE
            .inner_margin(egui::Margin::symmetric(OUTER_MARGIN_X, OUTER_MARGIN_Y))
            .show(ui, |ui| {
                self.drain_scan();
                self.drain_uninstaller();
                self.schedule_verification_scans();

                if self.settings != self.saved_settings {
                    self.page_index = 0;
                }
                let visible_indices = self.visible_indices();
                self.clamp_page(visible_indices.len());
                let paged_indices = self.paged_indices(&visible_indices);

                self.top_bar(ui, visible_indices.len());
                ui.separator();
                self.selection_bar(ui, &paged_indices);
                ui.separator();
                self.apps_table(ui, &paged_indices);
                self.pagination_bar(ui, visible_indices.len());
                self.selected_details(ui);
                self.bottom_jobs(ui);

                self.confirm_window(&ctx);
                self.save_settings_if_changed();

                let verifying = self.jobs.iter().any(|job| job.phase == JobPhase::Verifying);
                if self.is_scanning || self.active_launch_batches > 0 || verifying {
                    ctx.request_repaint_after(std::time::Duration::from_millis(150));
                }
            });
    }
}
