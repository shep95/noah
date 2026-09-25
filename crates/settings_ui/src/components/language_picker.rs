use std::sync::Arc;

use gpui::{AnyElement, App, Context, DismissEvent, SharedString, Task, Window};
use noah_i18n::{LANGUAGES, SYSTEM};
use picker::{Picker, PickerDelegate};
use ui::{ListItem, ListItemSpacing, prelude::*};

type LanguagePicker = Picker<LanguagePickerDelegate>;

struct LanguageEntry {
    code: SharedString,
    label: SharedString,
    /// Lowercased code, English name and native name, so people can find
    /// their language by any of them.
    search_text: String,
}

/// How a `language` setting value is shown: the language's own name, with
/// its English name for everyone else.
pub fn language_picker_label(code: &str) -> SharedString {
    if code.eq_ignore_ascii_case(SYSTEM) {
        return "System language".into();
    }
    match noah_i18n::find_language(code) {
        Some(language) if language.native_name == language.english_name => {
            language.native_name.into()
        }
        Some(language) => format!("{} — {}", language.native_name, language.english_name).into(),
        None => code.to_string().into(),
    }
}

pub struct LanguagePickerDelegate {
    entries: Vec<LanguageEntry>,
    matches: Vec<usize>,
    selected_index: usize,
    current_code: SharedString,
    on_language_changed: Arc<dyn Fn(SharedString, &mut Window, &mut App) + 'static>,
}

impl LanguagePickerDelegate {
    fn new(
        current_code: SharedString,
        on_language_changed: impl Fn(SharedString, &mut Window, &mut App) + 'static,
    ) -> Self {
        let system = LanguageEntry {
            code: SYSTEM.into(),
            label: language_picker_label(SYSTEM),
            search_text: "system automatic default".to_string(),
        };
        let entries: Vec<LanguageEntry> = std::iter::once(system)
            .chain(LANGUAGES.iter().map(|language| LanguageEntry {
                code: language.code.into(),
                label: language_picker_label(language.code),
                search_text: format!(
                    "{} {} {}",
                    language.code, language.english_name, language.native_name
                )
                .to_lowercase(),
            }))
            .collect();
        let matches: Vec<usize> = (0..entries.len()).collect();
        let selected_index = Self::position_of(&entries, &matches, &current_code);
        Self {
            entries,
            matches,
            selected_index,
            current_code,
            on_language_changed: Arc::new(on_language_changed),
        }
    }

    fn position_of(entries: &[LanguageEntry], matches: &[usize], code: &str) -> usize {
        matches
            .iter()
            .position(|index| {
                entries
                    .get(*index)
                    .is_some_and(|entry| entry.code.eq_ignore_ascii_case(code))
            })
            .unwrap_or(0)
    }
}

impl PickerDelegate for LanguagePickerDelegate {
    type ListItem = AnyElement;

    fn name() -> &'static str {
        "language picker"
    }

    fn match_count(&self) -> usize {
        self.matches.len()
    }

    fn selected_index(&self) -> usize {
        self.selected_index
    }

    fn set_selected_index(
        &mut self,
        index: usize,
        _: &mut Window,
        cx: &mut Context<LanguagePicker>,
    ) {
        self.selected_index = index.min(self.matches.len().saturating_sub(1));
        cx.notify();
    }

    fn placeholder_text(&self, _window: &mut Window, _cx: &mut App) -> Arc<str> {
        "Search languages…".into()
    }

    fn update_matches(
        &mut self,
        query: String,
        _window: &mut Window,
        cx: &mut Context<LanguagePicker>,
    ) -> Task<()> {
        let query = query.trim().to_lowercase();
        self.matches = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| query.is_empty() || entry.search_text.contains(&query))
            .map(|(index, _)| index)
            .collect();
        self.selected_index = Self::position_of(&self.entries, &self.matches, &self.current_code);
        cx.notify();
        Task::ready(())
    }

    fn confirm(&mut self, _secondary: bool, window: &mut Window, cx: &mut Context<LanguagePicker>) {
        let chosen = self
            .matches
            .get(self.selected_index)
            .and_then(|index| self.entries.get(*index));
        if let Some(entry) = chosen {
            (self.on_language_changed)(entry.code.clone(), window, cx);
        }
    }

    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<LanguagePicker>) {
        cx.defer_in(window, |picker, window, cx| {
            picker.set_query("", window, cx);
        });
        cx.emit(DismissEvent);
    }

    fn render_match(
        &self,
        index: usize,
        selected: bool,
        _window: &mut Window,
        _cx: &mut Context<LanguagePicker>,
    ) -> Option<Self::ListItem> {
        let entry = self.entries.get(*self.matches.get(index)?)?;
        Some(
            ListItem::new(index)
                .inset(true)
                .spacing(ListItemSpacing::Sparse)
                .toggle_state(selected)
                .child(Label::new(entry.label.clone()))
                .into_any_element(),
        )
    }
}

pub fn language_picker(
    current_code: SharedString,
    on_language_changed: impl Fn(SharedString, &mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut Context<LanguagePicker>,
) -> LanguagePicker {
    let delegate = LanguagePickerDelegate::new(current_code, on_language_changed);

    Picker::uniform_list(delegate, window, cx)
        .show_scrollbar(true)
        .initial_width(rems_from_px(260_f32))
        .max_height(rems(20.))
        .popover()
}
