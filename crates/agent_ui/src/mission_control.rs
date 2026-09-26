//! Mission control: the room where the person supervises shepherd rather
//! than chats with it. Every running conversation with its status, spend and
//! blockers; one queue of decisions waiting on the person, most consequential
//! first; the evidence each change shipped with; content that was withheld as
//! prompt injection; this month's spend against the budget; what shepherd is
//! allowed to do, in plain words; the provenance chain; and how shepherd's
//! past work actually held up.

use std::path::{Path, PathBuf};
use std::time::Duration;

use acp_thread::{AgentThreadEntry, ThreadStatus, ToolCallStatus};
use agent_settings::AgentSettings;
use anyhow::Context as _;
use editor::Editor;
use gpui::{
    Action, App, AsyncApp, Context, Entity, EventEmitter, FocusHandle, Focusable, Pixels,
    SharedString, Subscription, Task, WeakEntity, Window, actions, px,
};
use language_model::LanguageModelRegistry;
use noah_trust::{
    calibration, cost, evidence::Bundle, outcomes, project_files, provenance, rules_check,
};
use settings::Settings as _;
use ui::{Divider, Indicator, Tooltip, prelude::*};
use util::ResultExt as _;
use workspace::{
    Workspace,
    dock::{DockPosition, Panel, PanelEvent},
};

use crate::AgentPanel;
use crate::thread_metadata_store::ThreadId;

actions!(
    mission_control,
    [
        /// Opens mission control, where shepherd's work is supervised.
        ToggleFocus,
    ]
);

pub fn init(cx: &mut App) {
    cx.observe_new(|workspace: &mut Workspace, _, _| {
        workspace.register_action(|workspace, _: &ToggleFocus, window, cx| {
            workspace.toggle_panel_focus::<MissionControlPanel>(window, cx);
        });
    })
    .detach();
    agent::trust::load_quarantine(cx);
    load_brokered_secrets(cx);
}

/// Records whether the person kept or rejected shepherd's changes to these
/// buffers, next to the confidence shepherd gave each file in its latest
/// evidence, so mission control can show whether that confidence holds up.
pub(crate) fn record_review_outcome(
    buffers: impl IntoIterator<Item = Entity<language::Buffer>>,
    kept: bool,
    cx: &mut App,
) {
    let paths: Vec<PathBuf> = buffers
        .into_iter()
        .filter_map(|buffer| {
            let file = buffer.read(cx).file()?;
            Some(file.as_local()?.abs_path(cx))
        })
        .collect();
    if paths.is_empty() {
        return;
    }
    cx.background_spawn(async move {
        for path in paths {
            let Some(root) = path
                .ancestors()
                .find(|ancestor| project_files::path(ancestor, project_files::EVIDENCE).is_dir())
            else {
                continue;
            };
            let relative = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
            let latest = latest_bundles(root, 5).into_iter().find_map(|(_, bundle)| {
                let file = bundle.files.iter().find(|file| {
                    let claimed = file.path.replace('\\', "/");
                    let claimed = claimed.trim_start_matches("./");
                    !claimed.is_empty()
                        && (Path::new(&relative).ends_with(claimed)
                            || Path::new(claimed).ends_with(&relative))
                })?;
                Some((file.confidence, bundle.model.clone()))
            });
            let Some((confidence, model)) = latest else {
                continue;
            };
            calibration::record(
                &project_files::path(root, project_files::CALIBRATION),
                &calibration::Outcome {
                    time: chrono::Local::now().to_rfc3339(),
                    path: relative,
                    confidence,
                    kept,
                    model,
                },
            )
            .log_err();
        }
    })
    .detach();
}

fn latest_bundles(root: &Path, count: usize) -> Vec<(PathBuf, Bundle)> {
    let evidence_directory = project_files::path(root, project_files::EVIDENCE);
    let mut bundles: Vec<(PathBuf, Bundle)> = std::fs::read_dir(&evidence_directory)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "json"))
        .filter_map(|path| {
            let bundle: Bundle = serde_json::from_str(&std::fs::read_to_string(&path).ok()?).ok()?;
            Some((path.with_extension("md"), bundle))
        })
        .collect();
    bundles.sort_by(|a, b| b.1.created.cmp(&a.1.created));
    bundles.truncate(count);
    bundles
}

const SECRET_URL_PREFIX: &str = "noah-secret://";

fn secret_names_path() -> PathBuf {
    paths::data_dir().join("secrets.json")
}

fn read_secret_names() -> Vec<String> {
    std::fs::read_to_string(secret_names_path())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Secrets live in the system keychain; only their names are on disk.
fn load_brokered_secrets(cx: &mut App) {
    let credentials = zed_credentials_provider::global(cx);
    cx.spawn(async move |cx| {
        let mut secrets = Vec::new();
        for name in read_secret_names() {
            let url = format!("{SECRET_URL_PREFIX}{name}");
            if let Some((_, value)) = credentials.read_credentials(&url, cx).await.log_err().flatten() {
                secrets.push((name, String::from_utf8_lossy(&value).to_string()));
            }
        }
        cx.update(|cx| agent::trust::set_brokered_secrets(secrets, cx));
    })
    .detach();
}

struct FleetRow {
    thread_id: ThreadId,
    title: SharedString,
    status: FleetStatus,
    model: Option<String>,
    used_tokens: u64,
    max_tokens: u64,
    next_turn_usd: Option<f64>,
    pending: Vec<SharedString>,
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum FleetStatus {
    Working,
    NeedsYou,
    Idle,
}

struct Decision {
    thread_id: ThreadId,
    thread_title: SharedString,
    request: SharedString,
    irreversible: bool,
}

#[derive(Default)]
struct Snapshot {
    bundles: Vec<(PathBuf, Bundle)>,
    integrity: Option<Result<provenance::Integrity, String>>,
    provenance_entries: usize,
    outcomes: Option<outcomes::Outcomes>,
    calibration: Option<calibration::Calibration>,
    rules_findings: Option<usize>,
    month_by_model: Vec<(String, f64)>,
}

pub struct MissionControlPanel {
    focus_handle: FocusHandle,
    workspace: WeakEntity<Workspace>,
    project: Entity<project::Project>,
    position: DockPosition,
    zoomed: bool,
    snapshot: Snapshot,
    refreshing: Option<Task<()>>,
    secret_name: Entity<Editor>,
    secret_value: Entity<Editor>,
    secret_names: Vec<String>,
    message: Option<SharedString>,
    _ticker: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl MissionControlPanel {
    pub async fn load(
        workspace: WeakEntity<Workspace>,
        mut cx: gpui::AsyncWindowContext,
    ) -> anyhow::Result<Entity<Self>> {
        workspace.update_in(&mut cx, |workspace, window, cx| {
            let weak_workspace = cx.weak_entity();
            let project = workspace.project().clone();
            cx.new(|cx| Self::new(weak_workspace, project, window, cx))
        })
    }

    fn new(
        workspace: WeakEntity<Workspace>,
        project: Entity<project::Project>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let secret_name = cx.new(|cx| {
            let mut editor = Editor::single_line(window, cx);
            editor.set_placeholder_text("NAME, such as STRIPE_TEST_KEY", window, cx);
            editor
        });
        let secret_value = cx.new(|cx| {
            let mut editor = Editor::single_line(window, cx);
            editor.set_placeholder_text("value (kept in your system keychain)", window, cx);
            editor.set_masked(true, cx);
            editor
        });
        // The fleet changes every moment shepherd works, so the room redraws
        // on a short tick while it's open; file-backed sections refresh less
        // often because they read the disk and git.
        let ticker = cx.spawn(async move |this, cx| {
            let mut ticks = 0u32;
            loop {
                cx.background_executor().timer(Duration::from_secs(2)).await;
                ticks += 1;
                let result = this.update(cx, |this, cx| {
                    if ticks.is_multiple_of(10) {
                        this.refresh(cx);
                    }
                    cx.notify();
                });
                if result.is_err() {
                    break;
                }
            }
        });
        let mut this = Self {
            focus_handle: cx.focus_handle(),
            workspace,
            project,
            position: DockPosition::Right,
            zoomed: false,
            snapshot: Snapshot::default(),
            refreshing: None,
            secret_name,
            secret_value,
            secret_names: read_secret_names(),
            message: None,
            _ticker: ticker,
            _subscriptions: Vec::new(),
        };
        this.refresh(cx);
        this
    }

    fn project_root(&self, cx: &App) -> Option<PathBuf> {
        self.project
            .read(cx)
            .visible_worktrees(cx)
            .next()
            .map(|worktree| worktree.read(cx).abs_path().to_path_buf())
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.refreshing.is_some() {
            return;
        }
        let root = self.project_root(cx);
        self.refreshing = Some(cx.spawn(async move |this, cx| {
            let snapshot = load_snapshot(root, cx).await;
            this.update(cx, |this, cx| {
                this.snapshot = snapshot;
                this.refreshing = None;
                cx.notify();
            })
            .ok();
        }));
    }

    fn agent_panel(&self, cx: &App) -> Option<Entity<AgentPanel>> {
        self.workspace.upgrade()?.read(cx).panel::<AgentPanel>(cx)
    }

    fn fleet(&self, cx: &App) -> Vec<FleetRow> {
        let Some(agent_panel) = self.agent_panel(cx) else {
            return Vec::new();
        };
        let registry = LanguageModelRegistry::read_global(cx);
        let models: Vec<_> = registry.available_models(cx).collect();
        agent_panel
            .read(cx)
            .conversation_views()
            .into_iter()
            .filter_map(|view| {
                let view = view.read(cx);
                let thread = view.root_thread(cx)?;
                let thread_view = view.root_thread_view();
                let thread = thread.read(cx);
                if thread.entries().is_empty() {
                    return None;
                }
                let pending: Vec<SharedString> = thread
                    .entries()
                    .iter()
                    .filter_map(|entry| match entry {
                        AgentThreadEntry::ToolCall(call)
                            if matches!(call.status, ToolCallStatus::WaitingForConfirmation { .. }) =>
                        {
                            Some(call.label.read(cx).source().clone())
                        }
                        _ => None,
                    })
                    .collect();
                let status = if !pending.is_empty() {
                    FleetStatus::NeedsYou
                } else if thread.status() == ThreadStatus::Generating {
                    FleetStatus::Working
                } else {
                    FleetStatus::Idle
                };
                let model = thread_view.and_then(|thread_view| thread_view.read(cx).current_model_id(cx));
                let usage = thread.token_usage();
                let used_tokens = usage.map_or(0, |usage| usage.used_tokens);
                let next_turn_usd = model.as_ref().and_then(|model_id| {
                    let model = models.iter().find(|candidate| candidate.id().0.as_ref() == model_id.as_str())?;
                    let (input, output) = model.price_per_million_tokens()?;
                    Some(
                        cost::estimate_turn(
                            used_tokens.max(2_000),
                            cost::Pricing {
                                input_per_million: input,
                                output_per_million: output,
                            },
                        )
                        .usd,
                    )
                });
                Some(FleetRow {
                    thread_id: view.thread_id,
                    title: thread.title().unwrap_or_else(|| "new conversation".into()),
                    status,
                    model,
                    used_tokens,
                    max_tokens: usage.map_or(0, |usage| usage.max_tokens),
                    next_turn_usd,
                    pending,
                })
            })
            .collect()
    }

    fn open_thread(&self, thread_id: ThreadId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        workspace.update(cx, |workspace, cx| {
            workspace.enter_room(workspace::Room::Shepherd, window, cx);
            if let Some(panel) = workspace.panel::<AgentPanel>(cx) {
                panel.update(cx, |panel, cx| {
                    if panel.active_thread_id(cx) != Some(thread_id) {
                        panel.activate_retained_thread(thread_id, true, window, cx);
                    }
                });
            }
        });
    }

    fn stop_thread(&self, thread_id: ThreadId, cx: &mut Context<Self>) {
        if let Some(panel) = self.agent_panel(cx) {
            panel.update(cx, |panel, cx| {
                panel.cancel_thread(&thread_id, cx);
            });
        }
    }

    fn open_file(&self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(workspace) = self.workspace.upgrade() {
            workspace.update(cx, |workspace, cx| {
                workspace
                    .open_abs_path(path, workspace::OpenOptions::default(), window, cx)
                    .detach_and_log_err(cx);
            });
        }
    }

    fn store_secret(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.secret_name.read(cx).text(cx).trim().to_string();
        let value = self.secret_value.read(cx).text(cx);
        if !noah_trust::secrets::is_valid_secret_name(&name) {
            self.message = Some("a secret's name is capital letters, digits and _, such as API_TOKEN".into());
            cx.notify();
            return;
        }
        if value.is_empty() {
            self.message = Some("enter the secret's value".into());
            cx.notify();
            return;
        }
        let credentials = zed_credentials_provider::global(cx);
        let mut names = self.secret_names.clone();
        if !names.contains(&name) {
            names.push(name.clone());
        }
        self.secret_name.update(cx, |editor, cx| editor.set_text("", window, cx));
        self.secret_value.update(cx, |editor, cx| editor.set_text("", window, cx));
        cx.spawn(async move |this, cx| {
            let url = format!("{SECRET_URL_PREFIX}{name}");
            let stored = credentials
                .write_credentials(&url, "noah", value.as_bytes(), cx)
                .await
                .and_then(|_| {
                    std::fs::create_dir_all(paths::data_dir())?;
                    std::fs::write(secret_names_path(), serde_json::to_string(&names)?)?;
                    Ok(())
                });
            this.update(cx, |this, cx| {
                match stored {
                    Ok(()) => {
                        this.secret_names = names;
                        this.message = Some(format!("${name} is available to shepherd's commands; its value never reaches the model").into());
                        let mut secrets = agent::trust::brokered_secrets(cx);
                        secrets.retain(|(existing, _)| *existing != name);
                        secrets.push((name, value));
                        agent::trust::set_brokered_secrets(secrets, cx);
                    }
                    Err(error) => {
                        this.message = Some(format!("couldn't store the secret: {error:#}").into());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn forget_secret(&mut self, name: String, cx: &mut Context<Self>) {
        let credentials = zed_credentials_provider::global(cx);
        self.secret_names.retain(|existing| *existing != name);
        let names = self.secret_names.clone();
        let mut secrets = agent::trust::brokered_secrets(cx);
        secrets.retain(|(existing, _)| *existing != name);
        agent::trust::set_brokered_secrets(secrets, cx);
        cx.spawn(async move |_, cx| {
            credentials
                .delete_credentials(&format!("{SECRET_URL_PREFIX}{name}"), cx)
                .await
                .log_err();
            std::fs::write(secret_names_path(), serde_json::to_string(&names)?)?;
            anyhow::Ok(())
        })
        .detach_and_log_err(cx);
        cx.notify();
    }

    fn export_audit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.project_root(cx) else {
            return;
        };
        let task = cx.background_spawn(async move { write_audit_report(&root) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |this, window, cx| match result {
                Ok(path) => this.open_file(path, window, cx),
                Err(error) => {
                    this.message = Some(format!("couldn't export: {error:#}").into());
                    cx.notify();
                }
            })
        })
        .detach_and_log_err(cx);
    }

    /// Writes the change's evidence and diff as one html page and opens it in
    /// the browser room, ready to be sent to someone.
    fn share_change(&mut self, bundle: Bundle, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.project_root(cx) else {
            return;
        };
        let task = cx.background_spawn(async move { write_share_page(&root, &bundle) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |this, window, cx| match result {
                Ok(path) => {
                    let url = format!("file://{}", path.display());
                    window.dispatch_action(Box::new(zed_actions::OpenInBrowserRoom { url }), cx);
                    this.message = Some(format!("shared page written to {}", path.display()).into());
                    cx.notify();
                }
                Err(error) => {
                    this.message = Some(format!("couldn't write the page: {error:#}").into());
                    cx.notify();
                }
            })
        })
        .detach_and_log_err(cx);
    }

    fn render_section_title(title: &'static str, detail: Option<String>) -> impl IntoElement {
        h_flex()
            .pt_4()
            .pb_1()
            .gap_2()
            .child(Label::new(title).size(LabelSize::Small).color(Color::Muted))
            .when_some(detail, |this, detail| {
                this.child(Label::new(detail).size(LabelSize::XSmall).color(Color::Muted))
            })
    }

    fn render_fleet(&self, fleet: &[FleetRow], cx: &mut Context<Self>) -> impl IntoElement {
        let working = fleet.iter().filter(|row| row.status == FleetStatus::Working).count();
        v_flex()
            .child(Self::render_section_title(
                "fleet",
                Some(format!("{} conversations, {working} working", fleet.len())),
            ))
            .when(fleet.is_empty(), |this| {
                this.child(
                    Label::new("no conversations yet. start one in the shepherd room.")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
            })
            .children(fleet.iter().enumerate().map(|(index, row)| {
                let thread_id = row.thread_id;
                let (dot, status) = match row.status {
                    FleetStatus::Working => (Color::Accent, "working"),
                    FleetStatus::NeedsYou => (Color::Warning, "needs you"),
                    FleetStatus::Idle => (Color::Muted, "idle"),
                };
                let mut detail = status.to_string();
                if let Some(model) = &row.model {
                    detail.push_str(&format!(" · {model}"));
                }
                if row.max_tokens > 0 {
                    detail.push_str(&format!(
                        " · {}k of {}k tokens",
                        row.used_tokens / 1000,
                        row.max_tokens / 1000
                    ));
                }
                if let Some(usd) = row.next_turn_usd {
                    detail.push_str(&format!(" · next turn ≈ {}", cost::format_usd(usd)));
                }
                h_flex()
                    .id(("fleet-row", index))
                    .py_1()
                    .gap_2()
                    .child(Indicator::dot().color(dot))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(Label::new(row.title.clone()).size(LabelSize::Small).truncate())
                            .child(Label::new(detail).size(LabelSize::XSmall).color(Color::Muted)),
                    )
                    .child(
                        Button::new(("fleet-open", index), "open")
                            .style(ButtonStyle::Subtle)
                            .label_size(LabelSize::Small)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_thread(thread_id, window, cx)
                            })),
                    )
                    .when(row.status != FleetStatus::Idle, |this| {
                        this.child(
                            Button::new(("fleet-stop", index), "stop")
                                .style(ButtonStyle::Subtle)
                                .label_size(LabelSize::Small)
                                .tooltip(Tooltip::text("stop this conversation's current turn"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.stop_thread(thread_id, cx)
                                })),
                        )
                    })
            }))
    }

    fn render_decisions(&self, fleet: &[FleetRow], cx: &mut Context<Self>) -> impl IntoElement {
        let mut decisions: Vec<Decision> = fleet
            .iter()
            .flat_map(|row| {
                row.pending.iter().map(|request| Decision {
                    thread_id: row.thread_id,
                    thread_title: row.title.clone(),
                    request: request.clone(),
                    irreversible: request.starts_with("irreversible"),
                })
            })
            .collect();
        // Irreversible actions first: they are the decisions where attention
        // matters most, and everything else waits behind them anyway.
        decisions.sort_by_key(|decision| !decision.irreversible);
        v_flex()
            .child(Self::render_section_title(
                "decisions waiting on you",
                Some(if decisions.is_empty() {
                    "none. anything your permission rules allow is answered for you".to_string()
                } else {
                    format!("{}", decisions.len())
                }),
            ))
            .children(decisions.into_iter().enumerate().map(|(index, decision)| {
                let thread_id = decision.thread_id;
                h_flex()
                    .py_1()
                    .gap_2()
                    .child(Indicator::dot().color(if decision.irreversible {
                        Color::Error
                    } else {
                        Color::Warning
                    }))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                Label::new(decision.request)
                                    .size(LabelSize::Small)
                                    .truncate(),
                            )
                            .child(
                                Label::new(decision.thread_title)
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted),
                            ),
                    )
                    .child(
                        Button::new(("decide", index), "decide")
                            .style(ButtonStyle::Filled)
                            .label_size(LabelSize::Small)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_thread(thread_id, window, cx)
                            })),
                    )
            }))
    }

    fn render_evidence(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let bundles = &self.snapshot.bundles;
        v_flex()
            .child(Self::render_section_title(
                "evidence",
                Some(if bundles.is_empty() {
                    "shepherd hasn't handed in any evidence in this project yet".to_string()
                } else {
                    format!("latest {}", bundles.len())
                }),
            ))
            .children(bundles.iter().enumerate().map(|(index, (path, bundle))| {
                let problems = bundle.ungrounded_claims().len() + bundle.warnings.len();
                let passed = bundle.checks.iter().filter(|check| check.passed()).count();
                let mut detail = format!(
                    "{} · {passed}/{} checks passed",
                    bundle.created.get(..16).unwrap_or(&bundle.created).replace('T', " "),
                    bundle.checks.len()
                );
                if let Some(least) = bundle.review_order().first() {
                    detail.push_str(&format!(" · review {} first ({:.2})", least.path, least.confidence));
                }
                let path = path.clone();
                h_flex()
                    .id(("evidence-row", index))
                    .py_1()
                    .gap_2()
                    .cursor_pointer()
                    .child(Indicator::dot().color(if problems > 0 { Color::Warning } else { Color::Success }))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(Label::new(bundle.title.clone()).size(LabelSize::Small).truncate())
                            .child(Label::new(detail).size(LabelSize::XSmall).color(Color::Muted)),
                    )
                    .when(problems > 0, |this| {
                        this.child(
                            Label::new(format!("{problems} to check"))
                                .size(LabelSize::XSmall)
                                .color(Color::Warning),
                        )
                    })
                    .child({
                        let bundle = bundle.clone();
                        Button::new(("share-change", index), "share")
                            .style(ButtonStyle::Subtle)
                            .label_size(LabelSize::XSmall)
                            .tooltip(Tooltip::text("share this change: one html page with the evidence and the diff"))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.share_change(bundle.clone(), window, cx);
                            }))
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_file(path.clone(), window, cx)
                    }))
            }))
    }

    fn render_quarantine(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let records = agent::trust::recent_quarantine(cx);
        v_flex()
            .child(Self::render_section_title(
                "prompt injection caught",
                Some(if records.is_empty() {
                    "nothing so far".to_string()
                } else {
                    format!("{} recent", records.len())
                }),
            ))
            .children(records.into_iter().take(8).flat_map(|record| {
                let verb = if record.withheld { "withheld from" } else { "flagged in" };
                record.findings.into_iter().take(3).map(move |finding| {
                    v_flex()
                        .py_0p5()
                        .child(
                            Label::new(format!("{verb} {}: {}", record.tool, finding.reason))
                                .size(LabelSize::XSmall)
                                .color(Color::Warning),
                        )
                        .child(
                            Label::new(finding.text)
                                .size(LabelSize::XSmall)
                                .color(Color::Muted)
                                .truncate(),
                        )
                })
            }))
    }

    fn render_spend(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let spent = agent::trust::month_spend(cx);
        let budget = AgentSettings::get_global(cx).monthly_budget_usd;
        let state = cost::budget_state(spent, budget, 0.0);
        let (summary, color) = match state {
            cost::BudgetState::Unlimited => (
                format!("{} this month, no budget set (agent.monthly_budget_usd)", cost::format_usd(spent)),
                Color::Muted,
            ),
            cost::BudgetState::Within { remaining } => (
                format!("{} this month, {} left", cost::format_usd(spent), cost::format_usd(remaining)),
                Color::Muted,
            ),
            cost::BudgetState::Near { remaining } => (
                format!("{} this month, only {} left", cost::format_usd(spent), cost::format_usd(remaining)),
                Color::Warning,
            ),
            cost::BudgetState::Exceeded { over } => (
                format!("{} this month, {} over budget", cost::format_usd(spent), cost::format_usd(over)),
                Color::Error,
            ),
        };
        v_flex()
            .child(Self::render_section_title("spend", None))
            .child(Label::new(summary).size(LabelSize::Small).color(color))
            .children(self.snapshot.month_by_model.iter().take(5).map(|(model, usd)| {
                Label::new(format!("{model}: {}", cost::format_usd(*usd)))
                    .size(LabelSize::XSmall)
                    .color(Color::Muted)
            }))
            .child(
                Label::new("prices come from the provider; local models are free and never counted")
                    .size(LabelSize::XSmall)
                    .color(Color::Muted),
            )
    }

    fn render_permissions(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = AgentSettings::get_global(cx);
        let mut lines: Vec<String> = Vec::new();
        if settings.offline {
            lines.push("offline mode: only local models; no web, browser or registry access".into());
        }
        lines.push("irreversible commands (force pushes, recursive deletes, migrations, infrastructure, publishing) always ask you, whatever the rules say".into());
        lines.push(if settings.allowed_hosts.is_empty() {
            "web and browser: any host".to_string()
        } else {
            format!("web and browser: only {}", settings.allowed_hosts.join(", "))
        });
        lines.push("secrets are redacted from everything shepherd reads; web pages are screened for injected instructions".into());
        if settings.teaching_mode {
            lines.push("teaching mode: shepherd leaves the critical parts for you to write".into());
        }
        let secret_names = self.secret_names.clone();
        v_flex()
            .child(Self::render_section_title("what shepherd may do", None))
            .children(lines.into_iter().map(|line| {
                Label::new(format!("· {line}"))
                    .size(LabelSize::XSmall)
                    .color(Color::Muted)
            }))
            .child(
                Label::new("per-tool rules live in Settings > AI > Tool Permissions")
                    .size(LabelSize::XSmall)
                    .color(Color::Muted),
            )
            .child(Self::render_section_title(
                "secrets",
                Some("shepherd's commands get these as environment variables; it only sees the names".into()),
            ))
            .children(secret_names.into_iter().enumerate().map(|(index, name)| {
                let forget = name.clone();
                h_flex()
                    .gap_2()
                    .child(Label::new(format!("${name}")).size(LabelSize::Small))
                    .child(
                        Button::new(("forget-secret", index), "forget")
                            .style(ButtonStyle::Subtle)
                            .label_size(LabelSize::XSmall)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.forget_secret(forget.clone(), cx)
                            })),
                    )
            }))
            .child(
                h_flex()
                    .gap_1()
                    .pt_1()
                    .child(
                        div()
                            .flex_1()
                            .px_1p5()
                            .py_0p5()
                            .rounded_sm()
                            .bg(cx.theme().colors().element_background)
                            .child(self.secret_name.clone()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .px_1p5()
                            .py_0p5()
                            .rounded_sm()
                            .bg(cx.theme().colors().element_background)
                            .child(self.secret_value.clone()),
                    )
                    .child(
                        Button::new("store-secret", "store")
                            .style(ButtonStyle::Outlined)
                            .label_size(LabelSize::Small)
                            .on_click(cx.listener(|this, _, window, cx| this.store_secret(window, cx))),
                    ),
            )
    }

    fn render_provenance(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (summary, color) = match &self.snapshot.integrity {
            None => ("no project open".to_string(), Color::Muted),
            Some(Ok(provenance::Integrity::Intact { entries: 0 })) => {
                ("nothing recorded yet".to_string(), Color::Muted)
            }
            Some(Ok(provenance::Integrity::Intact { entries })) => (
                format!("{entries} changes recorded, chain intact"),
                Color::Success,
            ),
            Some(Ok(provenance::Integrity::Broken { entry, reason })) => (
                format!("chain broken at entry {entry}: {reason}"),
                Color::Error,
            ),
            Some(Err(error)) => (format!("couldn't read: {error}"), Color::Error),
        };
        v_flex()
            .child(Self::render_section_title(
                "provenance",
                Some("who or what wrote each change, tamper-evident".into()),
            ))
            .child(
                h_flex()
                    .gap_2()
                    .child(Label::new(summary).size(LabelSize::Small).color(color))
                    .child(div().flex_1())
                    .child(
                        Button::new("export-audit", "export audit report")
                            .style(ButtonStyle::Subtle)
                            .label_size(LabelSize::Small)
                            .on_click(cx.listener(|this, _, window, cx| this.export_audit(window, cx))),
                    ),
            )
    }

    fn render_outcomes(&self) -> impl IntoElement {
        let mut lines = Vec::new();
        if let Some(outcomes) = &self.snapshot.outcomes {
            lines.push(format!(
                "{} of {} commits were written with shepherd",
                outcomes.shepherd_commits, outcomes.total_commits
            ));
            for (model, numbers) in &outcomes.by_model {
                lines.push(format!(
                    "{model}: {} commits, {} reverted, {} fixed within a week",
                    numbers.commits, numbers.reverted, numbers.fixed_soon_after
                ));
            }
        }
        if let Some(calibration) = &self.snapshot.calibration {
            lines.push(format!("confidence: {}", calibration.summary()));
        }
        if let Some(findings) = self.snapshot.rules_findings {
            lines.push(format!(
                "agent instructions: {findings} problem{} (ask shepherd to run codebase rules)",
                if findings == 1 { "" } else { "s" }
            ));
        }
        v_flex()
            .child(Self::render_section_title(
                "outcomes",
                Some("what landed and stayed, not lines generated".into()),
            ))
            .when(lines.is_empty(), |this| {
                this.child(
                    Label::new("open a git project to see how shepherd's work held up")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
            })
            .children(lines.into_iter().map(|line| {
                Label::new(line).size(LabelSize::XSmall).color(Color::Muted)
            }))
            .children(
                self.snapshot
                    .calibration
                    .as_ref()
                    .map(|calibration| Self::render_calibration(calibration)),
            )
    }

    /// The calibration curve, drawn: for each band of stated confidence, the
    /// faint bar is what shepherd said and the solid bar is how often it was
    /// right, so the two line up exactly when its confidence means something.
    fn render_calibration(calibration: &calibration::Calibration) -> impl IntoElement {
        v_flex()
            .pt_1()
            .gap_0p5()
            .children(
                calibration
                    .buckets
                    .iter()
                    .filter(|bucket| bucket.count > 0)
                    .map(|bucket| {
                        let stated = ((bucket.low + bucket.high) / 2.0).clamp(0.0, 1.0);
                        let observed = bucket.kept_rate().unwrap_or(0.0).clamp(0.0, 1.0);
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                Label::new(format!("says {:.1}–{:.1}", bucket.low, bucket.high))
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted),
                            )
                            .child(
                                v_flex()
                                    .flex_1()
                                    .gap_px()
                                    .child(
                                        div().h_px().w(relative(stated)).bg(gpui::white().opacity(0.18)),
                                    )
                                    .child(
                                        div().h(px(3.)).w(relative(observed)).bg(gpui::white().opacity(0.7)),
                                    ),
                            )
                            .child(
                                Label::new(format!(
                                    "right {}% of {}",
                                    (observed * 100.0).round() as u32,
                                    bucket.count
                                ))
                                .size(LabelSize::XSmall)
                                .color(Color::Muted),
                            )
                    }),
            )
    }
}

async fn load_snapshot(root: Option<PathBuf>, cx: &mut AsyncApp) -> Snapshot {
    let Some(root) = root else {
        return Snapshot::default();
    };
    let git_log = {
        let root = root.clone();
        cx.background_spawn(async move {
            let output = util::command::new_command(paths::git_program())
                .current_dir(&root)
                .args(["log", "-n", "2000", "--name-only", outcomes::GIT_LOG_FORMAT])
                .output()
                .await
                .ok()?;
            output
                .status
                .success()
                .then(|| String::from_utf8_lossy(&output.stdout).to_string())
        })
        .await
    };
    cx.background_spawn(async move {
        let mut snapshot = Snapshot::default();
        snapshot.bundles = latest_bundles(&root, 8);

        let provenance_log = project_files::path(&root, project_files::PROVENANCE);
        snapshot.integrity = Some(provenance::verify(&provenance_log).map_err(|error| format!("{error:#}")));
        snapshot.provenance_entries = provenance::read(&provenance_log).map(|entries| entries.len()).unwrap_or(0);

        if let Some(log) = git_log {
            snapshot.outcomes = Some(outcomes::measure(&outcomes::parse_git_log(&log)));
        }
        let outcomes_log = calibration::read(&project_files::path(&root, project_files::CALIBRATION));
        if !outcomes_log.is_empty() {
            snapshot.calibration = Some(calibration::calibrate(&outcomes_log));
        }
        let mut instruction_files = Vec::new();
        for name in rules_check::INSTRUCTION_FILES {
            if let Ok(text) = std::fs::read_to_string(root.join(name)) {
                instruction_files.push((name.to_string(), text));
            }
        }
        if !instruction_files.is_empty() {
            snapshot.rules_findings = Some(
                rules_check::check(&instruction_files, |path| root.join(path).exists()).len(),
            );
        }
        let month = chrono::Local::now().format("%Y-%m").to_string();
        let mut by_model: std::collections::BTreeMap<String, f64> = Default::default();
        for entry in cost::read(&agent::trust::spend_log_path()) {
            if entry.time.starts_with(&month) {
                *by_model.entry(entry.model).or_insert(0.0) += entry.usd;
            }
        }
        let mut by_model: Vec<(String, f64)> = by_model.into_iter().collect();
        by_model.sort_by(|a, b| b.1.total_cmp(&a.1));
        snapshot.month_by_model = by_model;
        snapshot
    })
    .await
}

/// Writes a self-contained audit report: provenance chain status and every
/// recorded change, plus the evidence each change shipped with.
/// Writes one self-contained html page for a change: the evidence bundle
/// (claims against their checks, what wasn't verified, the files and how
/// sure shepherd was of each) and the diff of those files, so the proof can
/// be handed to someone who doesn't have noah.
fn write_share_page(root: &Path, bundle: &Bundle) -> anyhow::Result<PathBuf> {
    fn escape(text: &str) -> String {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    }
    let files: Vec<&str> = bundle.files.iter().map(|file| file.path.as_str()).collect();
    let diff = if files.is_empty() {
        String::new()
    } else {
        let working = std::process::Command::new(paths::git_program())
            .current_dir(root)
            .arg("diff")
            .arg("--")
            .args(&files)
            .output()
            .map(|output| String::from_utf8_lossy(&output.stdout).to_string())
            .unwrap_or_default();
        if working.trim().is_empty() {
            // Already committed: show the last commit that touched them.
            std::process::Command::new(paths::git_program())
                .current_dir(root)
                .args(["show", "--format=commit %h %s", "--"])
                .args(&files)
                .output()
                .map(|output| String::from_utf8_lossy(&output.stdout).to_string())
                .unwrap_or_default()
        } else {
            working
        }
    };

    let mut body = String::new();
    body.push_str(&format!("<h1>{}</h1>\n", escape(&bundle.title)));
    body.push_str(&format!(
        "<p class=\"meta\">{} · {}{}</p>\n",
        escape(&bundle.created.replace('T', " ")),
        escape(root.file_name().and_then(|name| name.to_str()).unwrap_or("project")),
        bundle
            .model
            .as_deref()
            .map(|model| format!(" · {}", escape(model)))
            .unwrap_or_default()
    ));
    body.push_str(&format!("<p>{}</p>\n", escape(&bundle.summary)));
    if !bundle.behavior_changes.is_empty() {
        body.push_str("<h2>behavior changed</h2>\n<ul>\n");
        for change in &bundle.behavior_changes {
            body.push_str(&format!("<li>{}</li>\n", escape(change)));
        }
        body.push_str("</ul>\n");
    }
    if !bundle.claims.is_empty() {
        body.push_str("<h2>claims</h2>\n<ul class=\"claims\">\n");
        for claim in &bundle.claims {
            let (mark, class) = match bundle.grounding(claim) {
                noah_trust::evidence::Grounding::Grounded => ("✓", "ok"),
                _ => ("✗", "bad"),
            };
            body.push_str(&format!(
                "<li class=\"{class}\"><span class=\"mark\">{mark}</span> {}{}</li>\n",
                escape(&claim.text),
                if claim.grounds.is_empty() {
                    String::new()
                } else {
                    format!(" <span class=\"grounds\">{}</span>", escape(&claim.grounds.join(", ")))
                }
            ));
        }
        body.push_str("</ul>\n");
    }
    if !bundle.checks.is_empty() {
        body.push_str("<h2>checks</h2>\n");
        for check in &bundle.checks {
            body.push_str(&format!(
                "<details><summary><code>{}</code> <span class=\"{}\">{}</span> · {} ms</summary><pre>{}</pre></details>\n",
                escape(&check.id),
                if check.passed() { "ok" } else { "bad" },
                escape(&check.command),
                check.duration_ms,
                escape(&check.output_tail)
            ));
        }
    }
    if !bundle.not_verified.is_empty() {
        body.push_str("<h2>not verified</h2>\n<ul>\n");
        for item in &bundle.not_verified {
            body.push_str(&format!("<li>{}</li>\n", escape(item)));
        }
        body.push_str("</ul>\n");
    }
    if !bundle.files.is_empty() {
        body.push_str("<h2>files</h2>\n<ul>\n");
        for file in &bundle.files {
            body.push_str(&format!(
                "<li><code>{}</code> <span class=\"meta\">confidence {:.2}</span>{}</li>\n",
                escape(&file.path),
                file.confidence,
                if file.note.is_empty() { String::new() } else { format!(" · {}", escape(&file.note)) }
            ));
        }
        body.push_str("</ul>\n");
    }
    if !diff.trim().is_empty() {
        body.push_str("<h2>diff</h2>\n<pre class=\"diff\">");
        for line in diff.lines() {
            let class = match line.chars().next() {
                Some('+') if !line.starts_with("+++") => "add",
                Some('-') if !line.starts_with("---") => "del",
                Some('@') => "hunk",
                _ => "",
            };
            body.push_str(&format!("<span class=\"{class}\">{}</span>\n", escape(line)));
        }
        body.push_str("</pre>\n");
    }
    body.push_str("<p class=\"meta\">made with noah · shepherd's evidence, not a summary of it</p>\n");

    let html = format!(
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>{}</title>\n<style>\n{}\n</style></head>\n<body><main>\n{body}</main></body></html>\n",
        escape(&bundle.title),
        SHARE_PAGE_STYLE
    );
    let directory = project_files::path(root, project_files::EVIDENCE);
    std::fs::create_dir_all(&directory)?;
    let path = directory.join(format!("{}.html", bundle.id));
    std::fs::write(&path, html)?;
    Ok(path)
}

// The page follows shepherd's interface rules: layered blacks, thin
// lowercase type, and one color that appears only where a claim is backed.
const SHARE_PAGE_STYLE: &str = "\
:root{color-scheme:dark}\
body{margin:0;background:#070909;color:#d6e0d2;font:300 15px/1.6 system-ui,-apple-system,'Segoe UI',sans-serif;-webkit-font-smoothing:antialiased}\
main{max-width:820px;margin:0 auto;padding:56px 24px 96px}\
h1{font-weight:300;font-size:30px;letter-spacing:-.01em;margin:0 0 6px}\
h2{font-weight:300;font-size:13px;letter-spacing:.08em;color:#8a9a8e;margin:36px 0 10px}\
p{margin:0 0 12px}\
.meta{color:#8a9a8e;font-size:13px}\
ul{padding-left:18px;margin:0}\
li{margin:4px 0}\
.claims{list-style:none;padding:0}\
.mark{display:inline-block;width:18px}\
.ok .mark{color:#9fd4a8}\
.bad .mark{color:#c07a74}\
.grounds{color:#8a9a8e;font-size:12px}\
code{font:12.5px/1.6 ui-monospace,Menlo,Consolas,monospace;background:#0f1412;padding:1px 5px;border-radius:3px}\
details{border:1px solid #121714;border-radius:4px;padding:8px 12px;margin:6px 0;background:#0b0f0d}\
summary{cursor:pointer;color:#b9beb7}\
pre{margin:10px 0 0;padding:12px;background:#080d0b;border-radius:4px;overflow:auto;font:12.5px/1.55 ui-monospace,Menlo,Consolas,monospace;color:#b9beb7;white-space:pre}\
pre.diff{padding:14px 16px}\
.add{color:#a8bf8f}\
.del{color:#c07a74}\
.hunk{color:#7f9fb0}\
span.ok{color:#9fd4a8}\
span.bad{color:#c07a74}\
@media(max-width:560px){main{padding:32px 16px 64px}h1{font-size:24px}}";

fn write_audit_report(root: &Path) -> anyhow::Result<PathBuf> {
    let provenance_log = project_files::path(root, project_files::PROVENANCE);
    let integrity = provenance::verify(&provenance_log)?;
    let entries = provenance::read(&provenance_log)?;
    let mut report = format!(
        "# audit report\n\nproject: `{}`\ngenerated: {}\n\n## provenance chain\n\n{}\n\n",
        root.display(),
        chrono::Local::now().to_rfc3339(),
        match &integrity {
            provenance::Integrity::Intact { entries } => format!("intact, {entries} entries"),
            provenance::Integrity::Broken { entry, reason } => format!("BROKEN at entry {entry}: {reason}"),
        }
    );
    report.push_str("| time | actor | action | path | lines | model | evidence | hash |\n|---|---|---|---|---|---|---|---|\n");
    for entry in &entries {
        report.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | `{}` |\n",
            entry.time,
            entry.actor,
            entry.action,
            entry.path.as_deref().unwrap_or(""),
            entry
                .lines
                .map(|(start, end)| format!("{start}–{end}"))
                .unwrap_or_default(),
            entry.model.as_deref().unwrap_or(""),
            entry.evidence.as_deref().unwrap_or(""),
            entry.hash.get(..12).unwrap_or(&entry.hash)
        ));
    }
    report.push_str("\n## evidence\n\n");
    let evidence_directory = project_files::path(root, project_files::EVIDENCE);
    let mut evidence_files: Vec<PathBuf> = std::fs::read_dir(&evidence_directory)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.extension().is_some_and(|extension| extension == "md"))
                .collect()
        })
        .unwrap_or_default();
    evidence_files.sort();
    for path in evidence_files {
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("couldn't read {}", path.display()))?;
        report.push_str(&text.replacen("# ", "### ", 1));
        report.push_str("\n\n");
    }
    let path = project_files::path(
        root,
        &format!("audit-{}.md", chrono::Local::now().format("%Y%m%d-%H%M%S")),
    );
    std::fs::write(&path, report)?;
    Ok(path)
}

impl EventEmitter<PanelEvent> for MissionControlPanel {}

impl Focusable for MissionControlPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for MissionControlPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let fleet = self.fleet(cx);
        v_flex()
            .id("mission-control")
            .key_context("MissionControlPanel")
            .track_focus(&self.focus_handle)
            .size_full()
            .px_3()
            .pb_4()
            .overflow_y_scroll()
            .child(
                h_flex()
                    .pt_3()
                    .child(Headline::new("mission control").size(HeadlineSize::XSmall))
                    .child(div().flex_1())
                    .child(
                        IconButton::new("mission-refresh", IconName::RotateCw)
                            .icon_size(IconSize::Small)
                            .tooltip(Tooltip::text("refresh"))
                            .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
                    ),
            )
            .when_some(self.message.clone(), |this, message| {
                this.child(Label::new(message).size(LabelSize::Small).color(Color::Accent))
            })
            .child(self.render_decisions(&fleet, cx))
            .child(Divider::horizontal())
            .child(self.render_fleet(&fleet, cx))
            .child(Divider::horizontal())
            .child(self.render_evidence(cx))
            .child(Divider::horizontal())
            .child(self.render_spend(cx))
            .child(Divider::horizontal())
            .child(self.render_quarantine(cx))
            .child(Divider::horizontal())
            .child(self.render_permissions(cx))
            .child(Divider::horizontal())
            .child(self.render_provenance(cx))
            .child(Divider::horizontal())
            .child(self.render_outcomes())
    }
}

impl Panel for MissionControlPanel {
    fn persistent_name() -> &'static str {
        "MissionControlPanel"
    }

    fn panel_key() -> &'static str {
        "MissionControlPanel"
    }

    fn position(&self, _window: &Window, _cx: &App) -> DockPosition {
        self.position
    }

    fn position_is_valid(&self, position: DockPosition) -> bool {
        matches!(position, DockPosition::Left | DockPosition::Right)
    }

    fn set_position(&mut self, position: DockPosition, _: &mut Window, cx: &mut Context<Self>) {
        self.position = position;
        cx.notify();
    }

    fn default_size(&self, _window: &Window, _cx: &App) -> Pixels {
        px(460.)
    }

    fn icon(&self, _window: &Window, _cx: &App) -> Option<IconName> {
        Some(IconName::ListTodo)
    }

    fn icon_tooltip(&self, _window: &Window, _cx: &App) -> Option<&'static str> {
        Some("mission control")
    }

    fn toggle_action(&self) -> Box<dyn Action> {
        Box::new(ToggleFocus)
    }

    fn is_zoomed(&self, _window: &Window, _cx: &App) -> bool {
        self.zoomed
    }

    fn set_zoomed(&mut self, zoomed: bool, _window: &mut Window, cx: &mut Context<Self>) {
        self.zoomed = zoomed;
        cx.notify();
    }

    fn activation_priority(&self) -> u32 {
        9
    }
}
