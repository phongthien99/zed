use zed_extension_api::{self as zed, ui};

struct TodoItem {
    title: String,
    done: bool,
}

struct FileTreeEntry {
    id: &'static str,
    name: &'static str,
    depth: u32,
    is_directory: bool,
    expanded: bool,
}

struct TestPanelExtension {
    count: i32,
    todos: Vec<TodoItem>,
    draft: String,
    file_tree: Vec<FileTreeEntry>,
    selected_file: Option<String>,
}

impl TestPanelExtension {
    fn render_counter(&self) -> ui::Element {
        let count_text = format!("Count: {}", self.count);
        ui::h_stack()
            .child(ui::label(count_text.as_str()))
            .child(ui::button("increment", "+1"))
    }

    fn render_todos(&self) -> ui::Element {
        let todo_checkboxes = self.todos.iter().enumerate().map(|(index, todo)| {
            let checkbox_id = format!("todo-{index}");
            ui::checkbox(checkbox_id.as_str(), todo.title.as_str(), todo.done)
        });

        ui::v_stack()
            .child(ui::label("Todos"))
            .child(ui::text_input("draft", "New todo…", self.draft.as_str()))
            .children(todo_checkboxes)
    }

    fn render_file_tree(&self) -> ui::Element {
        let mut rows = Vec::new();
        let mut hidden_below_depth: Option<u32> = None;

        for entry in &self.file_tree {
            if let Some(collapsed_depth) = hidden_below_depth {
                if entry.depth > collapsed_depth {
                    continue;
                }
                hidden_below_depth = None;
            }

            let is_selected = self.selected_file.as_deref() == Some(entry.id);
            let mut row = ui::list_item(entry.id, entry.name)
                .indent(entry.depth)
                .selected(is_selected);

            if entry.is_directory {
                row = row.icon("folder").expanded(Some(entry.expanded));
                if !entry.expanded {
                    hidden_below_depth = Some(entry.depth);
                }
            } else {
                row = row.icon("file").expanded(None);
            }

            rows.push(row);
        }

        ui::v_stack().child(ui::label("Files")).children(rows)
    }

    fn handle_list_item_toggled(&mut self, id: &str) {
        let Some(entry) = self.file_tree.iter_mut().find(|entry| entry.id == id) else {
            return;
        };

        if entry.is_directory {
            entry.expanded = !entry.expanded;
        } else {
            self.selected_file = Some(id.to_string());
        }
    }
}

impl zed::Extension for TestPanelExtension {
    fn new() -> Self {
        Self {
            count: 0,
            todos: Vec::new(),
            draft: String::new(),
            file_tree: vec![
                FileTreeEntry {
                    id: "src",
                    name: "src",
                    depth: 0,
                    is_directory: true,
                    expanded: true,
                },
                FileTreeEntry {
                    id: "src/main.rs",
                    name: "main.rs",
                    depth: 1,
                    is_directory: false,
                    expanded: false,
                },
                FileTreeEntry {
                    id: "src/ui",
                    name: "ui",
                    depth: 1,
                    is_directory: true,
                    expanded: false,
                },
                FileTreeEntry {
                    id: "src/ui/button.rs",
                    name: "button.rs",
                    depth: 2,
                    is_directory: false,
                    expanded: false,
                },
                FileTreeEntry {
                    id: "Cargo.toml",
                    name: "Cargo.toml",
                    depth: 0,
                    is_directory: false,
                    expanded: false,
                },
            ],
            selected_file: None,
        }
    }

    fn panel_render(
        &mut self,
        _panel_id: &str,
        _instance: ui::PanelInstance,
    ) -> zed::Result<ui::Tree> {
        Ok(ui::v_stack()
            .child(self.render_counter())
            .child(ui::divider())
            .child(self.render_todos())
            .child(ui::divider())
            .child(self.render_file_tree())
            .build())
    }

    fn panel_handle_event(
        &mut self,
        _panel_id: &str,
        _instance: ui::PanelInstance,
        event: ui::Event,
    ) -> zed::Result<()> {
        match event {
            ui::Event::Clicked(id) if id == "increment" => self.count += 1,
            ui::Event::InputChanged { id, value } if id == "draft" => self.draft = value,
            ui::Event::InputSubmitted { id, value } if id == "draft" => {
                let title = value.trim();
                if !title.is_empty() {
                    self.todos.push(TodoItem {
                        title: title.to_string(),
                        done: false,
                    });
                }
                self.draft.clear();
            }
            ui::Event::CheckboxToggled { id, checked } => {
                if let Some(todo) = id
                    .strip_prefix("todo-")
                    .and_then(|index| index.parse::<usize>().ok())
                    .and_then(|index| self.todos.get_mut(index))
                {
                    todo.done = checked;
                }
            }
            ui::Event::ListItemToggled(id) => self.handle_list_item_toggled(&id),
            _ => {}
        }
        Ok(())
    }
}

zed::register_extension!(TestPanelExtension);
