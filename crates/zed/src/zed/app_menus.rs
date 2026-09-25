use collab_ui::collab_panel;
use gpui::{App, Menu, MenuItem, OsAction};
use project::DisableAiSettings;
use release_channel::ReleaseChannel;
use settings::Settings;
use terminal_view::terminal_panel;
use zed_actions::{Quit, assistant, debug_panel, dev, git_panel, project_panel};

pub fn app_menus(cx: &mut App) -> Vec<Menu> {
    let mut view_items = vec![
        MenuItem::action(
            "zoom in",
            zed_actions::IncreaseBufferFontSize { persist: false },
        ),
        MenuItem::action(
            "zoom out",
            zed_actions::DecreaseBufferFontSize { persist: false },
        ),
        MenuItem::action(
            "reset zoom",
            zed_actions::ResetBufferFontSize { persist: false },
        ),
        MenuItem::action(
            "reset all zoom",
            zed_actions::ResetAllZoom { persist: false },
        ),
        MenuItem::separator(),
        MenuItem::action("toggle left dock", workspace::ToggleLeftDock),
        MenuItem::action("toggle right dock", workspace::ToggleRightDock),
        MenuItem::action("toggle bottom dock", workspace::ToggleBottomDock),
        MenuItem::action("toggle all docks", workspace::ToggleAllDocks),
        MenuItem::submenu(Menu {
            name: "editor layout".into(),
            disabled: false,
            items: vec![
                MenuItem::action("split up", workspace::SplitUp::default()),
                MenuItem::action("split down", workspace::SplitDown::default()),
                MenuItem::action("split left", workspace::SplitLeft::default()),
                MenuItem::action("split right", workspace::SplitRight::default()),
            ],
        }),
        MenuItem::separator(),
        MenuItem::action("project panel", project_panel::ToggleFocus),
        MenuItem::action("outline panel", outline_panel::ToggleFocus),
        MenuItem::action("collab panel", collab_panel::ToggleFocus),
        MenuItem::action("terminal panel", terminal_panel::Toggle),
        MenuItem::action("debugger panel", debug_panel::ToggleFocus),
    ];

    if !DisableAiSettings::get_global(cx).disable_ai {
        view_items.push(MenuItem::action("shepherd", assistant::ToggleFocus));
    }

    view_items.extend([
        MenuItem::action("git panel", git_panel::ToggleFocus),
        MenuItem::separator(),
        MenuItem::action("diagnostics", diagnostics::Deploy),
        MenuItem::separator(),
    ]);

    if ReleaseChannel::try_global(cx) == Some(ReleaseChannel::Dev) {
        view_items.push(MenuItem::action(
            "toggle GPUI inspector",
            dev::ToggleInspector,
        ));
        view_items.push(MenuItem::separator());
    }

    vec![
        Menu {
            name: "noah".into(),
            disabled: false,
            items: vec![
                MenuItem::action("about noah", zed_actions::About),
                MenuItem::separator(),
                MenuItem::submenu(Menu::new("settings").items([
                    MenuItem::action("open settings", zed_actions::OpenSettings),
                    MenuItem::action("open settings file", super::OpenSettingsFile),
                    MenuItem::action("open project settings", zed_actions::OpenProjectSettings),
                    MenuItem::action("open project settings file", super::OpenProjectSettingsFile),
                    MenuItem::action("open default settings", super::OpenDefaultSettings),
                    MenuItem::separator(),
                    MenuItem::action("open keymap", zed_actions::OpenKeymap),
                    MenuItem::action("open keymap file", zed_actions::OpenKeymapFile),
                    MenuItem::action("open default key bindings", zed_actions::OpenDefaultKeymap),
                    MenuItem::separator(),
                    MenuItem::action(
                        "select theme...",
                        zed_actions::theme_selector::Toggle::default(),
                    ),
                    MenuItem::action(
                        "select icon theme...",
                        zed_actions::icon_theme_selector::Toggle::default(),
                    ),
                ])),
                MenuItem::separator(),
                #[cfg(target_os = "macos")]
                MenuItem::os_submenu("Services", gpui::SystemMenuType::Services),
                MenuItem::separator(),
                MenuItem::action("extensions", zed_actions::Extensions::default()),
                MenuItem::action("noah lab", onboarding::noah_lab::Open),
                #[cfg(not(target_os = "windows"))]
                MenuItem::action("install CLI", install_cli::InstallCliBinary),
                MenuItem::separator(),
                #[cfg(target_os = "macos")]
                MenuItem::action("hide noah", super::Hide),
                #[cfg(target_os = "macos")]
                MenuItem::action("hide others", super::HideOthers),
                #[cfg(target_os = "macos")]
                MenuItem::action("show all", super::ShowAll),
                MenuItem::separator(),
                MenuItem::action("quit noah", Quit),
            ],
        },
        Menu {
            name: noah_i18n::t(cx, "file").to_string().into(),
            disabled: false,
            items: vec![
                MenuItem::action("new", workspace::NewFile),
                MenuItem::action("new window", workspace::NewWindow),
                MenuItem::separator(),
                #[cfg(not(target_os = "macos"))]
                MenuItem::action("open file...", workspace::OpenFiles),
                MenuItem::action(
                    if cfg!(not(target_os = "macos")) {
                        "Open Folder..."
                    } else {
                        "Open…"
                    },
                    workspace::Open::default(),
                ),
                MenuItem::action("open recent…", zed_actions::OpenRecent::default()),
                MenuItem::action("open remote…", zed_actions::OpenRemote::default()),
                MenuItem::separator(),
                MenuItem::action("add folder to project…", workspace::AddFolderToProject),
                MenuItem::separator(),
                MenuItem::action("save", workspace::Save { save_intent: None }),
                MenuItem::action("save as…", workspace::SaveAs),
                MenuItem::action("save all", workspace::SaveAll { save_intent: None }),
                MenuItem::separator(),
                MenuItem::action(
                    "close editor",
                    workspace::CloseActiveItem {
                        save_intent: None,
                        close_pinned: true,
                    },
                ),
                MenuItem::action("close project", workspace::CloseProject),
                MenuItem::action("close window", workspace::CloseWindow),
            ],
        },
        Menu {
            name: noah_i18n::t(cx, "edit").to_string().into(),
            disabled: false,
            items: vec![
                MenuItem::os_action("undo", editor::actions::Undo, OsAction::Undo),
                MenuItem::os_action("redo", editor::actions::Redo, OsAction::Redo),
                MenuItem::separator(),
                MenuItem::os_action("cut", editor::actions::Cut, OsAction::Cut),
                MenuItem::os_action("copy", editor::actions::Copy, OsAction::Copy),
                MenuItem::action("copy and trim", editor::actions::CopyAndTrim),
                MenuItem::os_action("paste", editor::actions::Paste, OsAction::Paste),
                MenuItem::separator(),
                MenuItem::action("find", search::buffer_search::Deploy::find()),
                MenuItem::action("find in project", workspace::DeploySearch::default()),
                MenuItem::separator(),
                MenuItem::action(
                    "toggle line comment",
                    editor::actions::ToggleComments::default(),
                ),
            ],
        },
        Menu {
            name: noah_i18n::t(cx, "selection").to_string().into(),
            disabled: false,
            items: vec![
                MenuItem::os_action(
                    "select all",
                    editor::actions::SelectAll,
                    OsAction::SelectAll,
                ),
                MenuItem::action("expand selection", editor::actions::SelectLargerSyntaxNode),
                MenuItem::action("shrink selection", editor::actions::SelectSmallerSyntaxNode),
                MenuItem::action("select next sibling", editor::actions::SelectNextSyntaxNode),
                MenuItem::action(
                    "select previous sibling",
                    editor::actions::SelectPreviousSyntaxNode,
                ),
                MenuItem::separator(),
                MenuItem::action(
                    "add cursor above",
                    editor::actions::AddSelectionAbove {
                        skip_soft_wrap: true,
                    },
                ),
                MenuItem::action(
                    "add cursor below",
                    editor::actions::AddSelectionBelow {
                        skip_soft_wrap: true,
                    },
                ),
                MenuItem::action(
                    "select next occurrence",
                    editor::actions::SelectNext {
                        replace_newest: false,
                    },
                ),
                MenuItem::action(
                    "select previous occurrence",
                    editor::actions::SelectPrevious {
                        replace_newest: false,
                    },
                ),
                MenuItem::action("select all occurrences", editor::actions::SelectAllMatches),
                MenuItem::separator(),
                MenuItem::action("move line up", editor::actions::MoveLineUp),
                MenuItem::action("move line down", editor::actions::MoveLineDown),
                MenuItem::action("duplicate selection", editor::actions::DuplicateLineDown),
            ],
        },
        Menu {
            name: noah_i18n::t(cx, "view").to_string().into(),
            disabled: false,
            items: view_items,
        },
        Menu {
            name: noah_i18n::t(cx, "go").to_string().into(),
            disabled: false,
            items: vec![
                MenuItem::action("back", workspace::GoBack),
                MenuItem::action("forward", workspace::GoForward),
                MenuItem::separator(),
                MenuItem::action("command palette...", zed_actions::command_palette::Toggle),
                MenuItem::separator(),
                MenuItem::action("go to file...", workspace::ToggleFileFinder::default()),
                // MenuItem::action("go to symbol in project", project_symbols::Toggle),
                MenuItem::action(
                    "go to symbol in editor...",
                    zed_actions::outline::ToggleOutline,
                ),
                MenuItem::action("go to line/column...", editor::actions::ToggleGoToLine),
                MenuItem::separator(),
                MenuItem::action(
                    "go to definition",
                    editor::actions::GoToDefinition::default(),
                ),
                MenuItem::action(
                    "go to declaration",
                    editor::actions::GoToDeclaration::default(),
                ),
                MenuItem::action(
                    "go to type definition",
                    editor::actions::GoToTypeDefinition::default(),
                ),
                MenuItem::action(
                    "find all references",
                    editor::actions::FindAllReferences::default(),
                ),
                MenuItem::action("show incoming calls", call_hierarchy::ShowIncomingCalls),
                MenuItem::action("show outgoing calls", call_hierarchy::ShowOutgoingCalls),
                MenuItem::separator(),
                MenuItem::action("next problem", editor::actions::GoToDiagnostic::default()),
                MenuItem::action(
                    "previous problem",
                    editor::actions::GoToPreviousDiagnostic::default(),
                ),
            ],
        },
        Menu {
            name: noah_i18n::t(cx, "run").to_string().into(),
            disabled: false,
            items: vec![
                MenuItem::action(
                    "spawn task",
                    zed_actions::Spawn::ViaModal {
                        reveal_target: None,
                    },
                ),
                MenuItem::action("start debugger", debugger_ui::Start),
                MenuItem::separator(),
                MenuItem::action("edit tasks.json…", zed_actions::OpenProjectTasks),
                MenuItem::action("edit debug.json…", zed_actions::OpenProjectDebugTasks),
                MenuItem::separator(),
                MenuItem::action("continue", debugger_ui::Continue),
                MenuItem::action("step over", debugger_ui::StepOver),
                MenuItem::action("step into", debugger_ui::StepInto),
                MenuItem::action("step out", debugger_ui::StepOut),
                MenuItem::separator(),
                MenuItem::action("toggle breakpoint", editor::actions::ToggleBreakpoint),
                MenuItem::action("edit breakpoint", editor::actions::EditLogBreakpoint),
                MenuItem::action("clear all breakpoints", debugger_ui::ClearAllBreakpoints),
            ],
        },
        Menu {
            name: noah_i18n::t(cx, "window").to_string().into(),
            disabled: false,
            items: vec![
                MenuItem::action("minimize", super::Minimize),
                MenuItem::action("zoom", super::Zoom),
                MenuItem::separator(),
            ],
        },
        Menu {
            name: noah_i18n::t(cx, "help").to_string().into(),
            disabled: false,
            items: vec![
                MenuItem::action("view dependency licenses", zed_actions::OpenLicenses),
                MenuItem::action("show welcome", onboarding::ShowWelcome),
                MenuItem::separator(),
                MenuItem::action(
                    "noah on GitHub",
                    super::OpenBrowser {
                        url: "https://github.com/shep95/noah".into(),
                    },
                ),
                MenuItem::action(
                    "report an issue",
                    super::OpenBrowser {
                        url: "https://github.com/shep95/noah/issues".into(),
                    },
                ),
                MenuItem::action(
                    "#houseofasher on Discord",
                    super::OpenBrowser {
                        url: "https://discord.gg/M9hnebRwvk".into(),
                    },
                ),
                MenuItem::action(
                    "asherin",
                    super::OpenBrowser {
                        url: "https://asherin.com/".into(),
                    },
                ),
                MenuItem::separator(),
                MenuItem::action(
                    "editor documentation (Zed)",
                    super::OpenBrowser {
                        url: "https://zed.dev/docs".into(),
                    },
                ),
            ],
        },
    ]
}
