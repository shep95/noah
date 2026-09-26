use std::path::PathBuf;
use std::sync::Arc;

use agent_client_protocol::schema::v1 as acp;
use anyhow::anyhow;
use futures::FutureExt as _;
use gpui::{App, AppContext as _, Entity, Task};
use noah_addons::{
    AddonRoots, Scope,
    tool_api::{
        ListAddonsInput, RunAddonInput, SaveAddonInput, SaveRequest, render_details, render_list,
        render_refused, render_review, render_run_output, render_saved,
    },
};
use project::{Project, trusted_worktrees::TrustedWorktrees};
use ui::SharedString;

use crate::{AgentTool, ToolCallEventStream, ToolInput, ToolPermissionContext};

/// The project folder whose `.noah/addons` holds project add-ons. There is
/// none for remote projects (the files aren't on this machine), for
/// asherin.chat's own folder, and for workspaces the person hasn't trusted,
/// whose add-ons could carry instructions into shepherd's prompt.
fn project_root(project: &Entity<Project>, cx: &App) -> Option<PathBuf> {
    let project = project.read(cx);
    if !project.is_local()
        || TrustedWorktrees::has_restricted_worktrees(&project.worktree_store(), cx)
    {
        return None;
    }
    let root = project
        .visible_worktrees(cx)
        .next()
        .map(|worktree| worktree.read(cx).abs_path().to_path_buf())?;
    (root != paths::chat_directory()).then_some(root)
}

pub fn addon_roots(project: &Entity<Project>, cx: &App) -> AddonRoots {
    AddonRoots::new(
        noah_addons::global_addons_directory(),
        project_root(project, cx).as_deref(),
    )
}

// Past this many, the prompt lists only the first ones and points shepherd at
// `list_addons` to search the rest, keeping the prompt small.
const MAX_ADDONS_IN_PROMPT: usize = 15;

/// The saved add-ons shepherd is told about at the start of a turn, so it
/// uses one instead of estimating an answer itself.
pub fn addon_catalog(project: &Entity<Project>, cx: &App) -> crate::AddonCatalog {
    let registry = noah_addons::discover(&addon_roots(project, cx));
    let addons: Vec<crate::AddonSummary> = registry
        .addons
        .iter()
        // `use_when` goes into the system prompt, so anything that reads like
        // instructions to an AI stays out; `list_addons` still shows it.
        .filter(|addon| noah_trust::injection::scan(&addon.manifest.use_when).is_empty())
        .map(|addon| crate::AddonSummary {
            name: addon.manifest.name.clone(),
            version: addon.manifest.version,
            use_when: addon.manifest.use_when.trim().to_string(),
        })
        .collect();
    let more = addons.len().saturating_sub(MAX_ADDONS_IN_PROMPT);
    crate::AddonCatalog {
        addons: addons.into_iter().take(MAX_ADDONS_IN_PROMPT).collect(),
        more,
    }
}

pub struct SaveAddonTool {
    project: Entity<Project>,
}

impl SaveAddonTool {
    pub fn new(project: Entity<Project>) -> Self {
        Self { project }
    }
}

impl AgentTool for SaveAddonTool {
    type Input = SaveAddonInput;
    type Output = String;

    const NAME: &'static str = "save_addon";

    fn kind() -> acp::ToolKind {
        acp::ToolKind::Edit
    }

    fn initial_title(
        &self,
        input: Result<Self::Input, serde_json::Value>,
        _cx: &mut App,
    ) -> SharedString {
        match input {
            Ok(input) => match input.restore_version {
                Some(version) => format!("restore add-on {} v{version}", input.name).into(),
                None => format!("save add-on {}", input.name).into(),
            },
            Err(_) => "save add-on".into(),
        }
    }

    fn run(
        self: Arc<Self>,
        input: ToolInput<Self::Input>,
        event_stream: ToolCallEventStream,
        cx: &mut App,
    ) -> Task<Result<Self::Output, Self::Output>> {
        let project = self.project.clone();
        cx.spawn(async move |cx| {
            let input = input.recv().await.map_err(|error| error.to_string())?;
            let request = input.into_request().map_err(|error| format!("{error:#}"))?;
            let roots = cx.update(|cx| addon_roots(&project, cx));
            let prepared = cx
                .background_spawn(async move {
                    match request {
                        SaveRequest::Draft { draft, scope } => {
                            noah_addons::prepare_save(draft, scope.unwrap_or(Scope::Global), &roots)
                        }
                        SaveRequest::Restore { name, version } => {
                            let registry = noah_addons::discover(&roots);
                            let addon = registry
                                .get(&name)
                                .ok_or_else(|| anyhow!("no add-on named `{name}` is saved"))?;
                            noah_addons::prepare_restore(addon, version, &roots)
                        }
                    }
                })
                .await
                .map_err(|error| format!("not saved: {error:#}"))?;
            let manifest = &prepared.source.manifest;
            if !prepared.report.all_passed() {
                return Err(render_refused(&prepared));
            }
            if prepared.is_unchanged() {
                return Ok(format!(
                    "{} v{} already has exactly this content; nothing was saved.",
                    manifest.name,
                    prepared.previous_version.unwrap_or_default()
                ));
            }

            let title = match prepared.previous_version {
                Some(_) => format!(
                    "update add-on {} to v{} ({})",
                    manifest.name, manifest.version, prepared.scope
                ),
                None => format!("install add-on {} ({})", manifest.name, prepared.scope),
            };
            event_stream.update_fields(
                acp::ToolCallUpdateFields::new()
                    .title(title.clone())
                    .content(vec![acp::ToolCallContent::from(render_review(&prepared))]),
            );
            // Creating or changing an add-on always needs the person's own
            // approval, whatever their tool permission settings say: a page
            // or file shepherd read must never be able to plant code here.
            let authorize = cx.update(|cx| {
                let context = ToolPermissionContext::new(Self::NAME, vec![title.clone()]);
                event_stream.authorize_always_prompt(title.clone(), context, cx)
            });
            futures::select! {
                result = authorize.fuse() => result.map_err(|error| error.to_string())?,
                _ = event_stream.cancelled_by_user().fuse() => return Err("cancelled".to_string()),
            }

            cx.background_spawn(async move {
                noah_addons::install(&prepared).map(|_| render_saved(&prepared))
            })
            .await
            .map_err(|error| format!("{error:#}"))
        })
    }
}

pub struct RunAddonTool {
    project: Entity<Project>,
}

impl RunAddonTool {
    pub fn new(project: Entity<Project>) -> Self {
        Self { project }
    }
}

impl AgentTool for RunAddonTool {
    type Input = RunAddonInput;
    type Output = String;

    const NAME: &'static str = "run_addon";

    fn kind() -> acp::ToolKind {
        acp::ToolKind::Execute
    }

    fn initial_title(
        &self,
        input: Result<Self::Input, serde_json::Value>,
        _cx: &mut App,
    ) -> SharedString {
        match input {
            Ok(input) => format!("run add-on {}", input.name).into(),
            Err(_) => "run add-on".into(),
        }
    }

    fn run(
        self: Arc<Self>,
        input: ToolInput<Self::Input>,
        event_stream: ToolCallEventStream,
        cx: &mut App,
    ) -> Task<Result<Self::Output, Self::Output>> {
        let project = self.project.clone();
        cx.spawn(async move |cx| {
            let input = input.recv().await.map_err(|error| error.to_string())?;
            let roots = cx.update(|cx| addon_roots(&project, cx));
            let (title, output) = cx
                .background_spawn(async move {
                    let registry = noah_addons::discover(&roots);
                    let Some(addon) = registry.get(&input.name) else {
                        return Err(format!(
                            "no add-on named `{}`. {}",
                            input.name,
                            render_list(&registry, None)
                        ));
                    };
                    let source = addon.load_source().map_err(|error| {
                        format!("{} can't be loaded: {error:#}", addon.manifest.name)
                    })?;
                    let execution =
                        noah_addons::run(&source, &input.input_value()).map_err(|error| {
                            format!(
                                "{} v{}: {error}",
                                addon.manifest.name, addon.manifest.version
                            )
                        })?;
                    Ok((
                        format!("ran {} v{}", addon.manifest.name, addon.manifest.version),
                        render_run_output(addon, &execution),
                    ))
                })
                .await?;
            event_stream.update_fields(acp::ToolCallUpdateFields::new().title(title));
            Ok(output)
        })
    }
}

pub struct ListAddonsTool {
    project: Entity<Project>,
}

impl ListAddonsTool {
    pub fn new(project: Entity<Project>) -> Self {
        Self { project }
    }
}

impl AgentTool for ListAddonsTool {
    type Input = ListAddonsInput;
    type Output = String;

    const NAME: &'static str = "list_addons";

    fn kind() -> acp::ToolKind {
        acp::ToolKind::Search
    }

    fn initial_title(
        &self,
        input: Result<Self::Input, serde_json::Value>,
        _cx: &mut App,
    ) -> SharedString {
        match input {
            Ok(ListAddonsInput {
                name: Some(name), ..
            }) => format!("show add-on {name}").into(),
            Ok(ListAddonsInput {
                query: Some(query), ..
            }) => format!("find add-ons for {query}").into(),
            _ => "list add-ons".into(),
        }
    }

    fn run(
        self: Arc<Self>,
        input: ToolInput<Self::Input>,
        _event_stream: ToolCallEventStream,
        cx: &mut App,
    ) -> Task<Result<Self::Output, Self::Output>> {
        let project = self.project.clone();
        cx.spawn(async move |cx| {
            let input = input.recv().await.map_err(|error| error.to_string())?;
            let roots = cx.update(|cx| addon_roots(&project, cx));
            cx.background_spawn(async move {
                let registry = noah_addons::discover(&roots);
                let Some(name) = input.name else {
                    return Ok(render_list(&registry, input.query.as_deref()));
                };
                let addon = registry.get(&name).ok_or_else(|| {
                    format!("no add-on named `{name}`. {}", render_list(&registry, None))
                })?;
                let source = match input.version {
                    Some(version) => addon.load_version(version),
                    None => addon.load_source(),
                }
                .map_err(|error| format!("{error:#}"))?;
                Ok(render_details(addon, &source))
            })
            .await
        })
    }
}
