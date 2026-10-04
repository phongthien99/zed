use collections::{HashMap, HashSet};
use editor::{Editor, EditorEvent};
use extension::{UiNode, UiTree};
use gpui::{App, Context, Entity, SharedString, Subscription, Window};
use ui::prelude::*;

use crate::ExtensionPanel;

pub(crate) const TEXT_AREA_MAX_LINES: usize = 10;

pub(crate) struct TextInputState {
    editor: Entity<Editor>,
    placeholder: String,
    multi_line: bool,
    /// The last value either reported to the extension or set from its tree, used to
    /// avoid echoing programmatic edits back to the extension as `InputChanged`.
    reported_value: String,
    _subscription: Subscription,
}

/// Editors are kept across tree updates rather than recreated from each tree, because
/// they hold state the extension doesn't know about, such as cursor and undo history.
#[derive(Default)]
pub(crate) struct TextInputs {
    states: HashMap<SharedString, TextInputState>,
    needs_sync: bool,
    values_are_fresh: bool,
}

impl TextInputs {
    pub(crate) fn tree_changed(&mut self, values_are_fresh: bool) {
        self.needs_sync = true;
        self.values_are_fresh = values_are_fresh;
    }

    pub(crate) fn sync(
        &mut self,
        tree: &UiTree,
        window: &mut Window,
        cx: &mut Context<ExtensionPanel>,
    ) {
        if !std::mem::take(&mut self.needs_sync) {
            return;
        }

        let mut seen_ids = HashSet::default();
        for node in &tree.nodes {
            let UiNode::TextInput(input) = node else {
                continue;
            };
            let id = SharedString::from(input.id.clone());
            seen_ids.insert(id.clone());

            // Single-line and multi-line editors are created in different modes, so an
            // input that switches between them needs a new editor.
            if self
                .states
                .get(&id)
                .is_some_and(|state| state.multi_line != input.multi_line)
            {
                self.states.remove(&id);
            }

            if let Some(state) = self.states.get_mut(&id) {
                if state.placeholder != input.placeholder {
                    state.placeholder = input.placeholder.clone();
                    state.editor.update(cx, |editor, cx| {
                        editor.set_placeholder_text(&input.placeholder, window, cx)
                    });
                }
                if self.values_are_fresh && state.editor.read(cx).text(cx) != input.value {
                    state.reported_value = input.value.clone();
                    state.editor.update(cx, |editor, cx| {
                        editor.set_text(input.value.as_str(), window, cx)
                    });
                }
            } else {
                let editor = cx.new(|cx| {
                    let mut editor = if input.multi_line {
                        Editor::auto_height(1, TEXT_AREA_MAX_LINES, window, cx)
                    } else {
                        Editor::single_line(window, cx)
                    };
                    editor.set_placeholder_text(&input.placeholder, window, cx);
                    editor.set_text(input.value.as_str(), window, cx);
                    editor
                });
                let subscription = cx.subscribe(&editor, {
                    let id = id.clone();
                    move |this, _, event: &EditorEvent, cx| {
                        if matches!(event, EditorEvent::BufferEdited) {
                            this.input_edited(&id, cx);
                        }
                    }
                });
                self.states.insert(
                    id,
                    TextInputState {
                        editor,
                        placeholder: input.placeholder.clone(),
                        multi_line: input.multi_line,
                        reported_value: input.value.clone(),
                        _subscription: subscription,
                    },
                );
            }
        }

        self.states.retain(|id, _| seen_ids.contains(id));
    }

    pub(crate) fn take_unreported_value(&mut self, id: &SharedString, cx: &App) -> Option<String> {
        let state = self.states.get_mut(id)?;
        let value = state.editor.read(cx).text(cx);
        if state.reported_value == value {
            return None;
        }
        state.reported_value = value.clone();
        Some(value)
    }

    pub(crate) fn value(&self, id: &SharedString, cx: &App) -> Option<String> {
        Some(self.states.get(id)?.editor.read(cx).text(cx))
    }

    pub(crate) fn editor(&self, id: &SharedString) -> Option<&Entity<Editor>> {
        self.states.get(id).map(|state| &state.editor)
    }
}
