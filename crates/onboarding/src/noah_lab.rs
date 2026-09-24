use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use extension_host::ExtensionStore;
use fs::Fs;
use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, ParentElement,
    Render, Styled, WeakEntity, Window, actions,
};
use ui::{ButtonLike, Divider, DividerColor, Vector, VectorName, prelude::*};
use workspace::{
    OpenMode, OpenOptions, Workspace,
    item::{Item, ItemEvent},
    with_active_or_new_workspace,
};
use zed_actions::{AGENT_SKILLS_SETTINGS_PATH, Extensions, OpenSettingsAt};

actions!(
    noah_lab,
    [
        /// Opens noah lab, where you build add-ons for noah together with shepherd.
        Open
    ]
);

pub fn init(cx: &mut App) {
    cx.on_action(|_: &Open, cx| {
        with_active_or_new_workspace(cx, |workspace, window, cx| {
            let existing = workspace
                .active_pane()
                .read(cx)
                .items()
                .find_map(|item| item.downcast::<NoahLab>());
            if let Some(existing) = existing {
                workspace.activate_item(&existing, true, true, window, cx);
            } else {
                let lab = cx.new(|cx| NoahLab::new(workspace.weak_handle(), cx));
                workspace.add_item_to_active_pane(Box::new(lab), None, true, window, cx);
            }
        });
    });
}

#[derive(Clone, Copy)]
enum Starter {
    Theme,
    Extension,
}

impl Starter {
    fn folder_prefix(self) -> &'static str {
        match self {
            Starter::Theme => "noah-theme",
            Starter::Extension => "noah-extension",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Starter::Theme => "theme",
            Starter::Extension => "extension",
        }
    }

    // A theme is pure data, so it can be installed the moment it exists. A
    // Rust extension has to be written and compiled first.
    fn installs_immediately(self) -> bool {
        matches!(self, Starter::Theme)
    }

    fn files(self, name: &str) -> Vec<(PathBuf, String)> {
        let manifest_header = format!(
            "id = \"{name}\"\n\
             name = \"{name}\"\n\
             version = \"0.0.1\"\n\
             schema_version = 1\n\
             authors = [\"you\"]\n\
             description = \"Made in noah lab.\"\n"
        );
        match self {
            Starter::Theme => vec![
                (
                    PathBuf::from("extension.toml"),
                    format!("{manifest_header}themes = [\"themes/{name}.json\"]\n"),
                ),
                (
                    PathBuf::from(format!("themes/{name}.json")),
                    NOAH_THEME.replacen("\"name\": \"noah\"", &format!("\"name\": \"{name}\""), 2),
                ),
                (PathBuf::from("AGENTS.md"), theme_guide(name)),
            ],
            Starter::Extension => {
                let crate_name = name.replace('-', "_");
                vec![
                    (PathBuf::from("extension.toml"), manifest_header),
                    (
                        PathBuf::from("Cargo.toml"),
                        format!(
                            "[package]\n\
                             name = \"{crate_name}\"\n\
                             version = \"0.0.1\"\n\
                             edition = \"2021\"\n\
                             \n\
                             [lib]\n\
                             crate-type = [\"cdylib\"]\n\
                             \n\
                             [dependencies]\n\
                             zed_extension_api = \"0.7.0\"\n"
                        ),
                    ),
                    (PathBuf::from("src/lib.rs"), EXTENSION_LIB.to_string()),
                    (PathBuf::from("AGENTS.md"), extension_guide(name)),
                ]
            }
        }
    }
}

const NOAH_THEME: &str = include_str!("../../../assets/themes/noah/noah.json");

const EXTENSION_LIB: &str = "use zed_extension_api as zed;

struct Extension;

impl zed::Extension for Extension {
    fn new() -> Self {
        Extension
    }
}

zed::register_extension!(Extension);
";

fn theme_guide(name: &str) -> String {
    format!(
        "# {name}: a noah theme

This folder is a noah theme extension, made in noah lab. It is already
installed, so changes show up as soon as it is rebuilt.

- `themes/{name}.json` holds the colors. It starts as a copy of the noah theme.
  Keys follow the theme schema linked in its `$schema` field: interface colors
  under `style`, code colors under `style.syntax`, collaborator colors under
  `style.players`. Colors are `#rrggbbaa`; a low alpha lets the wallpaper
  show through.
- `extension.toml` names the extension and lists the theme files.

To see a change: save the file, run `noah: rebuild dev extension` from the
command palette, then pick the theme with `theme selector: toggle`.
"
    )
}

fn extension_guide(name: &str) -> String {
    format!(
        "# {name}: a noah extension

This folder is a noah extension, made in noah lab. Extensions are Rust compiled
to WebAssembly and loaded by noah without rebuilding noah itself.

- `src/lib.rs` implements `zed_extension_api::Extension` (version 0.7.0). It
  can provide language servers, debug adapters, context servers (tools for the
  agent, over MCP) and slash commands.
- `extension.toml` declares what the extension provides, for example
  `[language_servers.<id>]` with `languages = [...]`, or
  `[context_servers.<id>]`.
- Languages and themes can live alongside the code in `languages/` and
  `themes/`, listed in `extension.toml`.

Building needs Rust installed through rustup. To try it: run
`noah: install dev extension` and choose this folder. After changes, run
`noah: rebuild dev extension`. Errors appear in the log (`noah: open log`).
"
    )
}

pub struct NoahLab {
    workspace: WeakEntity<Workspace>,
    focus_handle: FocusHandle,
    status: Option<SharedString>,
}

impl NoahLab {
    fn new(workspace: WeakEntity<Workspace>, cx: &mut Context<Self>) -> Self {
        Self {
            workspace,
            focus_handle: cx.focus_handle(),
            status: None,
        }
    }

    fn start(&mut self, starter: Starter, window: &mut Window, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let app_state = workspace.read(cx).app_state().clone();
        let fs = app_state.fs.clone();
        self.status = Some(format!("creating a {}…", starter.label()).into());
        cx.notify();

        cx.spawn_in(window, async move |this, cx| {
            let result = async {
                let folder = create_starter(fs, starter).await?;
                if starter.installs_immediately() {
                    cx.update(|_, cx| {
                        ExtensionStore::global(cx).update(cx, |store, cx| {
                            store.install_dev_extension(folder.clone(), cx)
                        })
                    })?
                    .await?;
                }
                cx.update(|_, cx| {
                    workspace::open_paths(
                        &[folder.clone()],
                        app_state,
                        OpenOptions {
                            open_mode: OpenMode::NewWindow,
                            ..OpenOptions::default()
                        },
                        cx,
                    )
                })?
                .await?;
                anyhow::Ok(folder)
            }
            .await;

            this.update(cx, |this, cx| {
                this.status = Some(match result {
                    Ok(folder) => format!(
                        "{} opened in a new window. tell shepherd what you want it to become.",
                        folder.display()
                    )
                    .into(),
                    Err(error) => format!("couldn't create the {}: {error:#}", starter.label()).into(),
                });
                cx.notify();
            })
        })
        .detach_and_log_err(cx);
    }

    fn render_row(
        &self,
        id: &'static str,
        icon: IconName,
        title: &'static str,
        description: &'static str,
        on_click: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        ButtonLike::new(id)
            .full_width()
            .size(ButtonSize::Large)
            .child(
                h_flex()
                    .w_full()
                    .gap_3()
                    .py_1()
                    .child(Icon::new(icon).color(Color::Muted).size(IconSize::Small))
                    .child(
                        v_flex()
                            .child(Label::new(title))
                            .child(
                                Label::new(description)
                                    .size(LabelSize::Small)
                                    .color(Color::Muted),
                            ),
                    ),
            )
            .on_click(cx.listener(move |this, _, window, cx| on_click(this, window, cx)))
    }
}

async fn create_starter(fs: Arc<dyn Fs>, starter: Starter) -> Result<PathBuf> {
    let lab_dir = paths::home_dir().join("noah-lab");
    fs.create_dir(&lab_dir).await?;

    let mut number = 1;
    let (name, folder) = loop {
        let name = format!("{}-{number}", starter.folder_prefix());
        let folder = lab_dir.join(&name);
        if fs.metadata(&folder).await?.is_none() {
            break (name, folder);
        }
        number += 1;
    };

    for (relative_path, contents) in starter.files(&name) {
        let path = folder.join(relative_path);
        if let Some(parent) = path.parent() {
            fs.create_dir(parent).await?;
        }
        fs.atomic_write(path, contents).await?;
    }
    Ok(folder)
}

impl Render for NoahLab {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .key_context("NoahLab")
            .track_focus(&self.focus_handle)
            .size_full()
            .bg(cx.theme().colors().editor_background)
            .justify_center()
            .child(
                v_flex()
                    .id("noah-lab")
                    .p_8()
                    .max_w_128()
                    .size_full()
                    .gap_4()
                    .justify_center()
                    .overflow_y_scroll()
                    .child(
                        h_flex()
                            .gap_4()
                            .mb_4()
                            .child(Vector::square(VectorName::ZedLogo, rems_from_px(36.)))
                            .child(
                                v_flex().child(Headline::new("noah lab")).child(
                                    Label::new("build on top of noah. add-ons load live, no rebuild.")
                                        .size(LabelSize::Small)
                                        .color(Color::Muted),
                                ),
                            ),
                    )
                    .child(self.render_row(
                        "lab-skill",
                        IconName::ZedAgent,
                        "a skill for shepherd",
                        "teach shepherd a new ability, in plain words",
                        |_, window, cx| {
                            window.dispatch_action(
                                Box::new(OpenSettingsAt {
                                    path: AGENT_SKILLS_SETTINGS_PATH.to_string(),
                                    target: None,
                                }),
                                cx,
                            );
                        },
                        cx,
                    ))
                    .child(self.render_row(
                        "lab-theme",
                        IconName::Image,
                        "a theme",
                        "your own colors, installed the moment it is created",
                        |this, window, cx| this.start(Starter::Theme, window, cx),
                        cx,
                    ))
                    .child(self.render_row(
                        "lab-extension",
                        IconName::Code,
                        "an extension",
                        "languages, language servers, tools for shepherd. rust, compiled to wasm",
                        |this, window, cx| this.start(Starter::Extension, window, cx),
                        cx,
                    ))
                    .child(Divider::horizontal().color(DividerColor::BorderVariant))
                    .child(self.render_row(
                        "lab-install",
                        IconName::Folder,
                        "install from a folder",
                        "load an add-on you already have",
                        |_, window, cx| {
                            window.dispatch_action(Box::new(extensions_ui::InstallDevExtension), cx);
                        },
                        cx,
                    ))
                    .child(self.render_row(
                        "lab-browse",
                        IconName::Blocks,
                        "browse extensions",
                        "everything already built for noah",
                        |_, window, cx| {
                            window.dispatch_action(Box::new(Extensions::default()), cx);
                        },
                        cx,
                    ))
                    .when_some(self.status.clone(), |this, status| {
                        this.child(
                            Label::new(status)
                                .size(LabelSize::Small)
                                .color(Color::Muted),
                        )
                    }),
            )
    }
}

impl EventEmitter<ItemEvent> for NoahLab {}

impl Focusable for NoahLab {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Item for NoahLab {
    type Event = ItemEvent;

    fn tab_content_text(&self, _detail: usize, _cx: &App) -> SharedString {
        "noah lab".into()
    }

    fn telemetry_event_text(&self) -> Option<&'static str> {
        None
    }

    fn show_toolbar(&self) -> bool {
        false
    }

    fn to_item_events(event: &Self::Event, f: &mut dyn FnMut(ItemEvent)) {
        f(*event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_starter_renames_the_copied_theme() {
        let files = Starter::Theme.files("noah-theme-3");
        let theme = files
            .iter()
            .find(|(path, _)| path == &PathBuf::from("themes/noah-theme-3.json"))
            .map(|(_, contents)| contents.as_str())
            .unwrap_or_default();
        assert_eq!(theme.matches("\"name\": \"noah-theme-3\"").count(), 2);
        let manifest = &files[0].1;
        assert!(manifest.contains("themes = [\"themes/noah-theme-3.json\"]"));
    }

    #[test]
    fn extension_starter_uses_a_valid_crate_name() {
        let files = Starter::Extension.files("noah-extension-1");
        let cargo = files
            .iter()
            .find(|(path, _)| path == &PathBuf::from("Cargo.toml"))
            .map(|(_, contents)| contents.as_str())
            .unwrap_or_default();
        assert!(cargo.contains("name = \"noah_extension_1\""));
    }
}
