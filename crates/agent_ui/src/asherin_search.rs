//! asherin.search's results panel: what shepherd found, drawn beside the
//! conversation the way a browser would show it: the answer, the sources
//! with a line of preview each, and a map of how they connect to the
//! question and to each other. It reads `findings/<name>/results.json`,
//! which shepherd keeps current while it works (see search_guide.md).

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use gpui::{
    Action, App, AppContext as _, Context, EventEmitter, FocusHandle, Focusable, PathBuilder,
    Pixels, Point, Task, WeakEntity, Window, actions, canvas, point, px,
};
use serde::Deserialize;
use ui::{Tooltip, prelude::*};
use workspace::Workspace;
use workspace::dock::{DockPosition, Panel, PanelEvent};

use crate::asherin_chat as room;

actions!(
    asherin_search,
    [
        /// Shows or hides asherin.search's results.
        ToggleResults
    ]
);

#[derive(Deserialize, Default, Clone)]
struct Results {
    #[serde(default)]
    request: String,
    #[serde(default)]
    answer: String,
    #[serde(default)]
    sources: Vec<Source>,
    #[serde(default)]
    claims: Vec<Claim>,
    #[serde(default)]
    links: Vec<Link>,
    #[serde(default)]
    open: Vec<String>,
}

#[derive(Deserialize, Clone)]
struct Source {
    id: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    preview: String,
    #[serde(default)]
    kind: String,
}

#[derive(Deserialize, Clone)]
struct Claim {
    id: String,
    text: String,
    #[serde(default)]
    sources: Vec<String>,
}

#[derive(Deserialize, Clone)]
struct Link {
    from: String,
    to: String,
    #[serde(default)]
    why: String,
}

/// Adds the results panel to a window that is asherin.search, now or when
/// its folder arrives.
pub(crate) fn attach(workspace: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    let folder = paths::search_directory();
    if room::is_room_workspace(workspace, &folder, cx) {
        add_panel(workspace, window, cx);
        return;
    }
    let project = workspace.project().clone();
    cx.subscribe_in(&project, window, move |workspace, _, event, window, cx| {
        if matches!(event, project::Event::WorktreeAdded(_))
            && workspace.panel::<SearchResultsPanel>(cx).is_none()
            && room::is_room_workspace(workspace, &folder, cx)
        {
            add_panel(workspace, window, cx);
        }
    })
    .detach();
}

fn add_panel(workspace: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    let weak_workspace = workspace.weak_handle();
    let panel = cx.new(|cx| SearchResultsPanel::new(weak_workspace, cx));
    workspace.add_panel(panel, window, cx);
    workspace.right_dock().update(cx, |dock, cx| {
        if let Some(index) = dock.panel_index_for_type::<SearchResultsPanel>() {
            dock.activate_panel(index, window, cx);
            dock.set_open(true, window, cx);
        }
    });
}

pub struct SearchResultsPanel {
    _workspace: WeakEntity<Workspace>,
    focus_handle: FocusHandle,
    position: DockPosition,
    root: PathBuf,
    results: Option<Results>,
    results_path: Option<PathBuf>,
    modified: Option<SystemTime>,
    _poll: Task<()>,
}

impl SearchResultsPanel {
    fn new(workspace: WeakEntity<Workspace>, cx: &mut Context<Self>) -> Self {
        let root = paths::search_directory();
        // The file is shepherd's to write and this panel's to read; a poll
        // every two seconds is simpler than a watcher and plenty for a page
        // that changes as often as a search progresses.
        let poll = cx.spawn({
            let root = root.clone();
            async move |this, cx| {
                loop {
                    cx.background_executor().timer(Duration::from_secs(2)).await;
                    let root = root.clone();
                    let latest = cx.background_spawn(async move { latest_results(&root) }).await;
                    let alive = this
                        .update(cx, |this, cx| {
                            let changed = latest.as_ref().map(|(path, modified)| (path, *modified))
                                != this.results_path.as_ref().zip(this.modified);
                            if changed {
                                match &latest {
                                    Some((path, modified)) => {
                                        this.results = std::fs::read_to_string(path)
                                            .ok()
                                            .and_then(|json| serde_json::from_str(&json).ok());
                                        this.results_path = Some(path.clone());
                                        this.modified = Some(*modified);
                                    }
                                    None => {
                                        this.results = None;
                                        this.results_path = None;
                                        this.modified = None;
                                    }
                                }
                                cx.notify();
                            }
                        })
                        .is_ok();
                    if !alive {
                        break;
                    }
                }
            }
        });
        Self {
            _workspace: workspace,
            focus_handle: cx.focus_handle(),
            position: DockPosition::Right,
            root,
            results: None,
            results_path: None,
            modified: None,
            _poll: poll,
        }
    }

    fn open_source(url: &str, window: &mut Window, cx: &mut App) {
        if url.starts_with("http://") || url.starts_with("https://") || url.starts_with("file://") {
            window.dispatch_action(
                Box::new(zed_actions::OpenInBrowserRoom {
                    url: url.to_string(),
                }),
                cx,
            );
        }
    }

    fn render_graph(&self, results: &Results, cx: &Context<Self>) -> impl IntoElement {
        // Request in the centre, claims on an inner ring, sources on an
        // outer ring. Positions are computed here for a fixed square, so the
        // labels (ordinary elements) and the edges (a canvas) agree.
        const SIDE: f32 = 300.0;
        let center = point(px(SIDE / 2.0), px(SIDE / 2.0));
        let place = |index: usize, count: usize, radius: f32| -> Point<Pixels> {
            let angle = std::f32::consts::TAU * index as f32 / count.max(1) as f32
                - std::f32::consts::FRAC_PI_2;
            point(
                px(SIDE / 2.0 + radius * angle.cos()),
                px(SIDE / 2.0 + radius * angle.sin()),
            )
        };
        let claim_positions: Vec<(String, Point<Pixels>)> = results
            .claims
            .iter()
            .enumerate()
            .map(|(index, claim)| (claim.id.clone(), place(index, results.claims.len(), SIDE * 0.22)))
            .collect();
        let source_positions: Vec<(String, Point<Pixels>)> = results
            .sources
            .iter()
            .enumerate()
            .map(|(index, source)| (source.id.clone(), place(index, results.sources.len(), SIDE * 0.42)))
            .collect();
        let position_of = |id: &str| -> Option<Point<Pixels>> {
            claim_positions
                .iter()
                .chain(source_positions.iter())
                .find(|(node, _)| node == id)
                .map(|(_, position)| *position)
        };
        let mut edges: Vec<(Point<Pixels>, Point<Pixels>)> = Vec::new();
        for (_, position) in &claim_positions {
            edges.push((center, *position));
        }
        for claim in &results.claims {
            if let Some(from) = position_of(&claim.id) {
                for source in &claim.sources {
                    if let Some(to) = position_of(source) {
                        edges.push((from, to));
                    }
                }
            }
        }
        for link in &results.links {
            if let (Some(from), Some(to)) = (position_of(&link.from), position_of(&link.to)) {
                edges.push((from, to));
            }
        }

        let colors = cx.theme().colors();
        let edge_color = colors.border_variant;
        let node_color = colors.text;
        let muted = colors.text_muted;

        let node = |position: Point<Pixels>, label: String, size: f32, color: gpui::Hsla| {
            div()
                .absolute()
                .left(position.x - px(size / 2.0))
                .top(position.y - px(size / 2.0))
                .size(px(size))
                .rounded_full()
                .bg(color)
                .child(
                    div()
                        .absolute()
                        .left(px(size + 4.0))
                        .top(px(-2.0))
                        .w(px(96.0))
                        .child(Label::new(label).size(LabelSize::XSmall).color(Color::Muted).truncate()),
                )
        };

        div()
            .relative()
            .size(px(SIDE))
            .child(canvas(
                move |_, _, _| edges,
                move |bounds, edges, window, _| {
                    for (from, to) in edges {
                        let mut builder = PathBuilder::stroke(px(1.0));
                        builder.move_to(point(bounds.origin.x + from.x, bounds.origin.y + from.y));
                        builder.line_to(point(bounds.origin.x + to.x, bounds.origin.y + to.y));
                        if let Ok(path) = builder.build() {
                            window.paint_path(path, edge_color);
                        }
                    }
                },
            ).size_full())
            .child(node(center, "you".into(), 10.0, node_color))
            .children(results.claims.iter().zip(claim_positions.iter()).map(|(claim, (_, position))| {
                node(*position, claim.id.clone(), 7.0, muted)
            }))
            .children(results.sources.iter().zip(source_positions.iter()).map(|(source, (_, position))| {
                let title = if source.title.is_empty() { source.url.clone() } else { source.title.clone() };
                node(*position, title, 6.0, node_color)
            }))
    }
}

/// The newest results file under `findings/`, by modification time.
fn latest_results(root: &Path) -> Option<(PathBuf, SystemTime)> {
    let findings = root.join("findings");
    let mut newest: Option<(PathBuf, SystemTime)> = None;
    for entry in std::fs::read_dir(&findings).ok()?.flatten() {
        let path = entry.path().join("results.json");
        let Ok(metadata) = std::fs::metadata(&path) else {
            continue;
        };
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        if newest.as_ref().is_none_or(|(_, best)| modified > *best) {
            newest = Some((path, modified));
        }
    }
    newest
}

impl EventEmitter<PanelEvent> for SearchResultsPanel {}

impl Focusable for SearchResultsPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Panel for SearchResultsPanel {
    fn persistent_name() -> &'static str {
        "AsherinSearchResults"
    }

    fn panel_key() -> &'static str {
        "AsherinSearchResults"
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
        px(420.)
    }

    fn icon(&self, _window: &Window, _cx: &App) -> Option<IconName> {
        Some(IconName::MagnifyingGlass)
    }

    fn icon_tooltip(&self, _window: &Window, _cx: &App) -> Option<&'static str> {
        Some("asherin.search results")
    }

    fn toggle_action(&self) -> Box<dyn Action> {
        Box::new(ToggleResults)
    }

    fn activation_priority(&self) -> u32 {
        24
    }
}

impl Render for SearchResultsPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        let heading = |text: &str| Label::new(text.to_string()).size(LabelSize::XSmall).color(Color::Muted);
        let body = v_flex()
            .id("asherin-search-results")
            .size_full()
            .p_3()
            .gap_3()
            .overflow_y_scroll()
            .track_focus(&self.focus_handle)
            .key_context("AsherinSearchResults");
        let Some(results) = self.results.clone() else {
            return body
                .child(Label::new("asherin.search").size(LabelSize::Small))
                .child(
                    Label::new(
                        "ask for anything in the conversation. as shepherd works, what it finds \
                         appears here: the answer, the sources with a line each, and a map of how \
                         they connect.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(
                    Label::new(format!("findings are kept in {}", self.root.join("findings").display()))
                        .size(LabelSize::XSmall)
                        .color(Color::Muted),
                );
        };
        body.child(Label::new(results.request.clone()).size(LabelSize::XSmall).color(Color::Muted))
            .when(!results.answer.is_empty(), |this| {
                this.child(Label::new(results.answer.clone()).size(LabelSize::Default))
            })
            .when(!results.sources.is_empty() || !results.claims.is_empty(), |this| {
                this.child(heading("map")).child(self.render_graph(&results, cx))
            })
            .when(!results.sources.is_empty(), |this| {
                this.child(heading("sources")).children(results.sources.iter().enumerate().map(
                    |(index, source)| {
                        let url = source.url.clone();
                        let title = if source.title.is_empty() { source.url.clone() } else { source.title.clone() };
                        v_flex()
                            .id(("search-source", index))
                            .py_1()
                            .gap_0p5()
                            .border_b_1()
                            .border_color(colors.border_variant)
                            .cursor_pointer()
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(Label::new(source.id.clone()).size(LabelSize::XSmall).color(Color::Muted))
                                    .child(Label::new(title).size(LabelSize::Small).truncate())
                                    .when(!source.kind.is_empty(), |this| {
                                        this.child(
                                            Label::new(source.kind.clone()).size(LabelSize::XSmall).color(Color::Muted),
                                        )
                                    }),
                            )
                            .when(!source.url.is_empty(), |this| {
                                this.child(Label::new(source.url.clone()).size(LabelSize::XSmall).color(Color::Muted).truncate())
                            })
                            .when(!source.preview.is_empty(), |this| {
                                this.child(Label::new(source.preview.clone()).size(LabelSize::Small).color(Color::Muted))
                            })
                            .tooltip(Tooltip::text("open in the browser room"))
                            .on_click(move |_, window, cx| Self::open_source(&url, window, cx))
                    },
                ))
            })
            .when(!results.claims.is_empty(), |this| {
                this.child(heading("what the answer rests on")).children(results.claims.iter().map(|claim| {
                    Label::new(format!("{} {} ({})", claim.id, claim.text, claim.sources.join(", ")))
                        .size(LabelSize::Small)
                }))
            })
            .when(!results.links.is_empty(), |this| {
                this.child(heading("how the sources relate")).children(results.links.iter().map(|link| {
                    Label::new(format!("{} → {}: {}", link.from, link.to, link.why))
                        .size(LabelSize::XSmall)
                        .color(Color::Muted)
                }))
            })
            .when(!results.open.is_empty(), |this| {
                this.child(heading("still open")).children(
                    results
                        .open
                        .iter()
                        .map(|item| Label::new(item.clone()).size(LabelSize::Small).color(Color::Muted)),
                )
            })
    }
}
