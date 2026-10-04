use super::{
    ButtonStyle, Color, Icon, IconButton, LabelSize, MenuEntry, Spacing, Tree, menu_entries, wit_ui,
};

#[derive(Debug, Clone)]
enum ElementKind {
    VStack(Spacing),
    HStack(Spacing),
    Label(wit_ui::Label),
    Button(wit_ui::Button),
    IconButton(wit_ui::IconButton),
    TextInput(wit_ui::TextInput),
    Checkbox(wit_ui::Checkbox),
    ListItem(wit_ui::ListItem),
    Divider,
    Spacer,
}

/// A UI element, which may contain nested child elements.
#[derive(Debug, Clone)]
pub struct Element {
    kind: ElementKind,
    children: Vec<Element>,
}

impl Element {
    fn new(kind: ElementKind) -> Self {
        Self {
            kind,
            children: Vec::new(),
        }
    }

    /// Adds a child element. Only has an effect on stacks.
    pub fn child(mut self, child: Element) -> Self {
        if self.is_stack() {
            self.children.push(child);
        }
        self
    }

    /// Adds several child elements. Only has an effect on stacks.
    pub fn children(mut self, children: impl IntoIterator<Item = Element>) -> Self {
        if self.is_stack() {
            self.children.extend(children);
        }
        self
    }

    /// Sets the space between the children of a stack. Only has an effect on stacks.
    ///
    /// Stacks default to [`Spacing::Small`].
    pub fn gap(mut self, gap: Spacing) -> Self {
        if let ElementKind::VStack(spacing) | ElementKind::HStack(spacing) = &mut self.kind {
            *spacing = gap;
        }
        self
    }

    /// Sets whether a button or icon button is disabled. Only has an effect on buttons.
    pub fn disabled(mut self, disabled: bool) -> Self {
        match &mut self.kind {
            ElementKind::Button(button) => button.disabled = disabled,
            ElementKind::IconButton(button) => button.disabled = disabled,
            _ => {}
        }
        self
    }

    /// Sets the text displayed on hover. Only has an effect on buttons, icon buttons and
    /// list items.
    pub fn tooltip(mut self, tooltip: impl Into<String>) -> Self {
        let tooltip = Some(tooltip.into());
        match &mut self.kind {
            ElementKind::Button(button) => button.tooltip = tooltip,
            ElementKind::IconButton(button) => button.tooltip = tooltip,
            ElementKind::ListItem(item) => item.tooltip = tooltip,
            _ => {}
        }
        self
    }

    /// Sets the color of a label's or list item's text. Only has an effect on labels and
    /// list items.
    pub fn color(mut self, color: Color) -> Self {
        match &mut self.kind {
            ElementKind::Label(label) => label.color = color,
            ElementKind::ListItem(item) => item.label_color = color,
            _ => {}
        }
        self
    }

    /// Sets whether a label's or list item's text is struck through. Only has an effect on
    /// labels and list items.
    pub fn strikethrough(mut self, strikethrough: bool) -> Self {
        match &mut self.kind {
            ElementKind::Label(label) => label.strikethrough = strikethrough,
            ElementKind::ListItem(item) => item.strikethrough = strikethrough,
            _ => {}
        }
        self
    }

    /// Sets the size of a label's text. Only has an effect on labels.
    pub fn size(mut self, size: LabelSize) -> Self {
        if let ElementKind::Label(label) = &mut self.kind {
            label.size = size;
        }
        self
    }

    /// Sets whether a label's text is bold. Only has an effect on labels.
    pub fn bold(mut self, bold: bool) -> Self {
        if let ElementKind::Label(label) = &mut self.kind {
            label.bold = bold;
        }
        self
    }

    /// Sets the style of a button. Only has an effect on buttons.
    pub fn style(mut self, style: ButtonStyle) -> Self {
        if let ElementKind::Button(button) = &mut self.kind {
            button.style = style;
        }
        self
    }

    /// Sets whether a button stretches to fill its container. Only has an effect on buttons.
    pub fn full_width(mut self, full_width: bool) -> Self {
        if let ElementKind::Button(button) = &mut self.kind {
            button.full_width = full_width;
        }
        self
    }

    /// Sets whether a text input accepts several lines of text. Only has an effect on text
    /// inputs.
    ///
    /// A multi-line input is submitted with ctrl-enter (cmd-enter on macOS).
    pub fn multi_line(mut self, multi_line: bool) -> Self {
        if let ElementKind::TextInput(input) = &mut self.kind {
            input.multi_line = multi_line;
        }
        self
    }

    /// Sets the indentation level of a list item. Only has an effect on list items.
    pub fn indent(mut self, indent: u32) -> Self {
        if let ElementKind::ListItem(item) = &mut self.kind {
            item.indent = indent;
        }
        self
    }

    /// Sets the icon of a button or list item.
    ///
    /// Buttons only support [`Icon::Named`]. Has no effect on other elements.
    pub fn icon(mut self, icon: impl Into<Icon>) -> Self {
        match (&mut self.kind, icon.into()) {
            (ElementKind::ListItem(item), icon) => item.icon = Some(icon.into()),
            (ElementKind::Button(button), Icon::Named(name)) => button.icon = Some(name),
            _ => {}
        }
        self
    }

    /// Sets whether a list item is selected. Only has an effect on list items.
    pub fn selected(mut self, selected: bool) -> Self {
        if let ElementKind::ListItem(item) = &mut self.kind {
            item.selected = selected;
        }
        self
    }

    /// Sets whether a list item is expanded.
    ///
    /// `None` marks a leaf item, while `Some(expanded)` marks an item that can be
    /// expanded or collapsed. Only has an effect on list items.
    pub fn expanded(mut self, expanded: Option<bool>) -> Self {
        if let ElementKind::ListItem(item) = &mut self.kind {
            item.expanded = expanded;
        }
        self
    }

    /// Sets the dimmed text displayed after a list item's label. Only has an effect on
    /// list items.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        if let ElementKind::ListItem(item) = &mut self.kind {
            item.description = Some(description.into());
        }
        self
    }

    /// Sets the short text displayed at the end of a list item. Only has an effect on list
    /// items.
    pub fn decoration(mut self, text: impl Into<String>, color: Color) -> Self {
        if let ElementKind::ListItem(item) = &mut self.kind {
            item.decoration = Some(wit_ui::Decoration {
                text: text.into(),
                color,
            });
        }
        self
    }

    /// Attaches a menu to a button or icon button.
    ///
    /// A button gets an arrow at its end that opens the menu, while an icon button opens
    /// the menu when clicked instead of producing [`Event::Clicked`]. Has no effect on
    /// other elements.
    pub fn menu(mut self, entries: impl IntoIterator<Item = MenuEntry>) -> Self {
        match &mut self.kind {
            ElementKind::Button(button) => button.menu = menu_entries(entries),
            ElementKind::IconButton(button) => button.menu = menu_entries(entries),
            _ => {}
        }
        self
    }

    /// Sets the menu opened by right-clicking a list item. Only has an effect on list
    /// items.
    pub fn context_menu(mut self, entries: impl IntoIterator<Item = MenuEntry>) -> Self {
        if let ElementKind::ListItem(item) = &mut self.kind {
            item.context_menu = menu_entries(entries);
        }
        self
    }

    /// Adds a button that is displayed at the end of a list item while it is hovered.
    /// Only has an effect on list items.
    pub fn action(mut self, action: IconButton) -> Self {
        if let ElementKind::ListItem(item) = &mut self.kind {
            item.actions.push(action.0);
        }
        self
    }

    /// Flattens this element and its descendants into a [`Tree`] rooted at this element.
    pub fn build(self) -> Tree {
        let mut nodes = Vec::new();
        let root = self.flatten(&mut nodes);
        Tree {
            nodes,
            root,
            badge: None,
        }
    }

    fn is_stack(&self) -> bool {
        matches!(self.kind, ElementKind::VStack(_) | ElementKind::HStack(_))
    }

    fn flatten(self, nodes: &mut Vec<wit_ui::Node>) -> u32 {
        let index = nodes.len();
        // Reserve the slot before visiting children so that a parent always
        // precedes its descendants in the arena.
        nodes.push(wit_ui::Node::Divider);
        let node = match self.kind {
            ElementKind::VStack(gap) => wit_ui::Node::VStack(wit_ui::Stack {
                children: Self::flatten_children(self.children, nodes),
                gap,
            }),
            ElementKind::HStack(gap) => wit_ui::Node::HStack(wit_ui::Stack {
                children: Self::flatten_children(self.children, nodes),
                gap,
            }),
            ElementKind::Label(label) => wit_ui::Node::Label(label),
            ElementKind::Button(button) => wit_ui::Node::Button(button),
            ElementKind::IconButton(button) => wit_ui::Node::IconButton(button),
            ElementKind::TextInput(input) => wit_ui::Node::TextInput(input),
            ElementKind::Checkbox(checkbox) => wit_ui::Node::Checkbox(checkbox),
            ElementKind::ListItem(item) => wit_ui::Node::ListItem(item),
            ElementKind::Divider => wit_ui::Node::Divider,
            ElementKind::Spacer => wit_ui::Node::Spacer,
        };
        if let Some(slot) = nodes.get_mut(index) {
            *slot = node;
        }
        index as u32
    }

    fn flatten_children(children: Vec<Element>, nodes: &mut Vec<wit_ui::Node>) -> Vec<u32> {
        children
            .into_iter()
            .map(|child| child.flatten(nodes))
            .collect()
    }
}

/// Returns a vertical stack.
pub fn v_stack() -> Element {
    Element::new(ElementKind::VStack(Spacing::Small))
}

/// Returns a horizontal stack.
pub fn h_stack() -> Element {
    Element::new(ElementKind::HStack(Spacing::Small))
}

/// Returns a text label.
pub fn label(text: impl Into<String>) -> Element {
    Element::new(ElementKind::Label(wit_ui::Label {
        text: text.into(),
        color: Color::Default,
        size: LabelSize::Default,
        bold: false,
        strikethrough: false,
    }))
}

/// Returns a button that produces [`Event::Clicked`] with the given `id` when clicked.
pub fn button(id: impl Into<String>, label: impl Into<String>) -> Element {
    Element::new(ElementKind::Button(wit_ui::Button {
        id: id.into(),
        label: label.into(),
        disabled: false,
        icon: None,
        style: ButtonStyle::Default,
        full_width: false,
        tooltip: None,
        menu: Vec::new(),
    }))
}

/// Returns a button that only displays the named icon and produces [`Event::Clicked`]
/// with the given `id` when clicked.
pub fn icon_button(id: impl Into<String>, icon: impl Into<String>) -> Element {
    Element::new(ElementKind::IconButton(wit_ui::IconButton {
        id: id.into(),
        icon: icon.into(),
        tooltip: None,
        disabled: false,
        menu: Vec::new(),
    }))
}

/// Returns a single-line text input.
pub fn text_input(
    id: impl Into<String>,
    placeholder: impl Into<String>,
    value: impl Into<String>,
) -> Element {
    Element::new(ElementKind::TextInput(wit_ui::TextInput {
        id: id.into(),
        placeholder: placeholder.into(),
        value: value.into(),
        multi_line: false,
    }))
}

/// Returns a checkbox with a label.
pub fn checkbox(id: impl Into<String>, label: impl Into<String>, checked: bool) -> Element {
    Element::new(ElementKind::Checkbox(wit_ui::Checkbox {
        id: id.into(),
        label: label.into(),
        checked,
    }))
}

/// Returns a list item.
pub fn list_item(id: impl Into<String>, label: impl Into<String>) -> Element {
    Element::new(ElementKind::ListItem(wit_ui::ListItem {
        id: id.into(),
        label: label.into(),
        indent: 0,
        icon: None,
        selected: false,
        expanded: None,
        description: None,
        label_color: Color::Default,
        strikethrough: false,
        decoration: None,
        actions: Vec::new(),
        tooltip: None,
        context_menu: Vec::new(),
    }))
}

/// Returns a visual separator.
pub fn divider() -> Element {
    Element::new(ElementKind::Divider)
}

/// Returns empty space that grows to fill its stack, pushing later siblings to the end.
pub fn spacer() -> Element {
    Element::new(ElementKind::Spacer)
}
