use std::collections::HashMap;
use std::io;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::execute;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use ratatui_textarea::TextArea;
use tui_input::Input as TuiInput;

use popup_common::{
    evaluate_condition, parse_condition, ConditionExpr, Element, PopupDefinition,
    PopupResult, PopupState,
};

use crate::render;

/// Backing widget for a text input element.
///
/// Single-line inputs use `tui-input::Input` for proper cursor movement,
/// word navigation, and horizontal scrolling.
/// Multi-line inputs (rows > 1) use `ratatui-textarea::TextArea` which
/// additionally supports vertical navigation and selection.
pub enum InputWidget<'a> {
    SingleLine(TuiInput),
    // Boxed to reduce enum size: TextArea is ~496 bytes vs TuiInput at ~64 bytes.
    MultiLine(Box<TextArea<'a>>),
}

impl InputWidget<'_> {
    /// Return the current text content as a single `String`.
    pub fn value(&self) -> String {
        match self {
            InputWidget::SingleLine(input) => input.value().to_string(),
            InputWidget::MultiLine(ta) => ta.lines().join("\n"),
        }
    }
}

pub struct TuiApp {
    pub definition: PopupDefinition,
    pub state: PopupState,
    pub focus_index: usize,
    pub focusable_ids: Vec<String>,
    pub condition_cache: HashMap<String, ConditionExpr>,
    pub result: Option<PopupResult>,
    pub scroll_offset: u16,
    /// Per-element state for list widgets (select/multi cursor position)
    pub list_cursors: HashMap<String, usize>,
    /// Backing input widgets for text input elements.
    /// These are the authoritative source for text content while the popup is
    /// running. Values are synced back to `state` when submitting.
    pub input_widgets: HashMap<String, InputWidget<'static>>,
}

impl TuiApp {
    pub fn new(definition: PopupDefinition) -> Self {
        let state = PopupState::new(&definition);
        let input_widgets = collect_input_widgets(&definition.elements, &state);
        let mut app = Self {
            definition,
            state,
            focus_index: 0,
            focusable_ids: Vec::new(),
            condition_cache: HashMap::new(),
            result: None,
            scroll_offset: 0,
            list_cursors: HashMap::new(),
            input_widgets,
        };
        app.rebuild_focusable_ids();
        app
    }

    pub fn rebuild_focusable_ids(&mut self) {
        let value_map = self.state.to_value_map(&self.definition.elements);
        self.focusable_ids.clear();
        collect_focusable_elements(
            &self.definition.elements,
            &value_map,
            &mut self.condition_cache,
            &self.state,
            &mut self.focusable_ids,
        );

        if self.focus_index >= self.focusable_ids.len() {
            self.focus_index = self.focusable_ids.len().saturating_sub(1);
        }
    }

    pub fn focused_id(&self) -> Option<&str> {
        self.focusable_ids.get(self.focus_index).map(|s| s.as_str())
    }

    /// Set a text input value, keeping both the backing widget and `state` in sync.
    /// Use this in tests and programmatic scenarios instead of mutating `state` directly.
    pub fn set_text_value(&mut self, id: &str, value: &str) {
        if let Some(text) = self.state.get_text_mut(id) {
            *text = value.to_string();
        }
        if let Some(widget) = self.input_widgets.get_mut(id) {
            match widget {
                InputWidget::SingleLine(input) => {
                    *input = TuiInput::from(value.to_string());
                }
                InputWidget::MultiLine(ta) => {
                    let lines: Vec<String> = value.lines().map(|l| l.to_string()).collect();
                    **ta = TextArea::from(lines);
                    ta.move_cursor(ratatui_textarea::CursorMove::End);
                }
            }
        }
    }

    /// Adjust `scroll_offset` so the focused element is visible within `viewport_height` lines.
    /// Call this after any focus change or layout rebuild. Uses the same element height
    /// estimates as the renderer so scroll tracking stays consistent.
    pub fn update_scroll(&mut self, viewport_height: u16, viewport_width: u16) {
        let Some(focused_id) = self.focused_id().map(|s| s.to_string()) else {
            return;
        };

        // Walk elements to find the virtual y-start of the focused element.
        let mut result = None;
        let mut virtual_y = 0u16;
        find_element_virtual_y(
            &self.definition.elements,
            self,
            &focused_id,
            viewport_width,
            &mut virtual_y,
            &mut result,
        );

        let Some((elem_y, elem_h)) = result else {
            return;
        };

        let bottom = self.scroll_offset.saturating_add(viewport_height);
        if elem_y + elem_h > bottom {
            self.scroll_offset = (elem_y + elem_h).saturating_sub(viewport_height);
        }
        if elem_y < self.scroll_offset {
            self.scroll_offset = elem_y;
        }
    }

    pub fn focus_next(&mut self) {
        if !self.focusable_ids.is_empty() {
            self.focus_index = (self.focus_index + 1) % self.focusable_ids.len();
        }
    }

    pub fn focus_prev(&mut self) {
        if !self.focusable_ids.is_empty() {
            self.focus_index = if self.focus_index == 0 {
                self.focusable_ids.len() - 1
            } else {
                self.focus_index - 1
            };
        }
    }

    /// Sync all input widget values back into `state`, then collect the result.
    pub fn submit(&mut self) {
        // Sync widget values → state so submitted result reflects in-widget edits.
        for (id, widget) in &self.input_widgets {
            if let Some(text) = self.state.get_text_mut(id) {
                *text = widget.value();
            }
        }
        self.state.button_clicked = Some("submit".to_string());
        let active_ids = self.collect_active_ids();
        self.result = Some(PopupResult::from_state_with_active_elements(
            &self.state,
            &self.definition,
            &active_ids,
        ));
    }

    pub fn cancel(&mut self) {
        self.result = Some(PopupResult::Cancelled);
    }

    fn collect_active_ids(&self) -> Vec<String> {
        let value_map = self.state.to_value_map(&self.definition.elements);
        let mut active = Vec::new();
        collect_active_element_ids(
            &self.definition.elements,
            &value_map,
            &self.condition_cache,
            &self.state,
            &mut active,
        );
        active
    }

    pub fn is_element_visible(&self, when: &Option<String>) -> bool {
        let value_map = self.state.to_value_map(&self.definition.elements);
        check_visibility(when, &value_map, &self.condition_cache)
    }
}

fn collect_focusable_elements(
    elements: &[Element],
    value_map: &HashMap<String, serde_json::Value>,
    condition_cache: &mut HashMap<String, ConditionExpr>,
    state: &PopupState,
    out: &mut Vec<String>,
) {
    for element in elements {
        let when = element_when(element);
        if !check_visibility_caching(when, value_map, condition_cache) {
            continue;
        }

        match element {
            Element::Text { .. } | Element::Markdown { .. } => {}
            Element::Slider { id, .. }
            | Element::Input { id, .. } => {
                out.push(id.clone());
            }
            Element::Check { id, reveals, .. } => {
                out.push(id.clone());
                if state.get_boolean(id) {
                    collect_focusable_elements(reveals, value_map, condition_cache, state, out);
                }
            }
            Element::Select {
                id,
                options,
                option_children,
                reveals,
                ..
            } => {
                out.push(id.clone());
                if let Some(Some(idx)) = state.get_choice(id) {
                    if let Some(opt) = options.get(idx) {
                        if let Some(children) = option_children.get(opt.value()) {
                            collect_focusable_elements(children, value_map, condition_cache, state, out);
                        }
                    }
                }
                if state.get_choice(id).is_some_and(|c| c.is_some()) {
                    collect_focusable_elements(reveals, value_map, condition_cache, state, out);
                }
            }
            Element::Multi {
                id,
                options,
                option_children,
                reveals,
                ..
            } => {
                out.push(id.clone());
                if let Some(selections) = state.get_multichoice(id) {
                    for (i, &selected) in selections.iter().enumerate() {
                        if selected {
                            if let Some(opt) = options.get(i) {
                                if let Some(children) = option_children.get(opt.value()) {
                                    collect_focusable_elements(children, value_map, condition_cache, state, out);
                                }
                            }
                        }
                    }
                    if selections.iter().any(|&s| s) {
                        collect_focusable_elements(reveals, value_map, condition_cache, state, out);
                    }
                }
            }
            Element::Group { elements, .. } => {
                collect_focusable_elements(elements, value_map, condition_cache, state, out);
            }
        }
    }
}

fn collect_active_element_ids(
    elements: &[Element],
    value_map: &HashMap<String, serde_json::Value>,
    condition_cache: &HashMap<String, ConditionExpr>,
    state: &PopupState,
    out: &mut Vec<String>,
) {
    for element in elements {
        let when = element_when(element);
        if !check_visibility(when, value_map, condition_cache) {
            continue;
        }

        match element {
            Element::Text { .. } | Element::Markdown { .. } => {}
            Element::Slider { id, .. }
            | Element::Input { id, .. } => {
                out.push(id.clone());
            }
            Element::Check { id, reveals, .. } => {
                out.push(id.clone());
                if state.get_boolean(id) {
                    collect_active_element_ids(reveals, value_map, condition_cache, state, out);
                }
            }
            Element::Select {
                id,
                options,
                option_children,
                reveals,
                ..
            } => {
                out.push(id.clone());
                if let Some(Some(idx)) = state.get_choice(id) {
                    if let Some(opt) = options.get(idx) {
                        if let Some(children) = option_children.get(opt.value()) {
                            collect_active_element_ids(children, value_map, condition_cache, state, out);
                        }
                    }
                }
                if state.get_choice(id).is_some_and(|c| c.is_some()) {
                    collect_active_element_ids(reveals, value_map, condition_cache, state, out);
                }
            }
            Element::Multi {
                id,
                options,
                option_children,
                reveals,
                ..
            } => {
                out.push(id.clone());
                if let Some(selections) = state.get_multichoice(id) {
                    for (i, &selected) in selections.iter().enumerate() {
                        if selected {
                            if let Some(opt) = options.get(i) {
                                if let Some(children) = option_children.get(opt.value()) {
                                    collect_active_element_ids(children, value_map, condition_cache, state, out);
                                }
                            }
                        }
                    }
                    if selections.iter().any(|&s| s) {
                        collect_active_element_ids(reveals, value_map, condition_cache, state, out);
                    }
                }
            }
            Element::Group { elements, .. } => {
                collect_active_element_ids(elements, value_map, condition_cache, state, out);
            }
        }
    }
}

pub(crate) fn element_when(element: &Element) -> &Option<String> {
    match element {
        Element::Text { when, .. }
        | Element::Markdown { when, .. }
        | Element::Slider { when, .. }
        | Element::Check { when, .. }
        | Element::Input { when, .. }
        | Element::Multi { when, .. }
        | Element::Select { when, .. }
        | Element::Group { when, .. } => when,
    }
}

/// Check whether an element's `when` condition is satisfied.
///
/// This version takes a shared reference to the cache and parses on-the-fly for any condition
/// not already in the cache (parse results are not stored back). The mutable version used during
/// `rebuild_focusable_ids` populates the cache, so by the time this is called for rendering or
/// result collection the cache is already warm.
fn check_visibility(
    when: &Option<String>,
    value_map: &HashMap<String, serde_json::Value>,
    condition_cache: &HashMap<String, ConditionExpr>,
) -> bool {
    match when {
        None => true,
        Some(condition_str) => {
            let parsed;
            let expr = if let Some(cached) = condition_cache.get(condition_str) {
                cached
            } else {
                match parse_condition(condition_str) {
                    Ok(p) => {
                        parsed = p;
                        &parsed
                    }
                    Err(_) => return true, // Parse failure = always visible
                }
            };
            evaluate_condition(expr, value_map)
        }
    }
}

/// Variant of `check_visibility` that also caches newly parsed conditions.
/// Used during focusable element collection to build up the condition cache.
fn check_visibility_caching(
    when: &Option<String>,
    value_map: &HashMap<String, serde_json::Value>,
    condition_cache: &mut HashMap<String, ConditionExpr>,
) -> bool {
    match when {
        None => true,
        Some(condition_str) => {
            let expr = if let Some(cached) = condition_cache.get(condition_str) {
                cached
            } else {
                match parse_condition(condition_str) {
                    Ok(parsed) => {
                        condition_cache.insert(condition_str.clone(), parsed);
                        condition_cache.get(condition_str).unwrap()
                    }
                    Err(_) => return true, // Parse failure = always visible
                }
            };
            evaluate_condition(expr, value_map)
        }
    }
}

pub fn run(definition: PopupDefinition) -> Result<PopupResult> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = TuiApp::new(definition);

    let result = loop {
        app.rebuild_focusable_ids();

        // Update scroll before drawing so the focused element is always visible.
        let size = terminal.size()?;
        let viewport_height = size.height.saturating_sub(4); // minus title+statusbar
        let viewport_width = size.width;
        app.update_scroll(viewport_height, viewport_width);

        terminal.draw(|frame| {
            render::draw(frame, &app);
        })?;

        if let Event::Key(key) = event::read()? {
            handle_key_event(&mut app, key);
        }

        if let Some(result) = app.result.take() {
            break result;
        }
    };

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    Ok(result)
}

fn handle_key_event(app: &mut TuiApp, key: KeyEvent) {
    // Global keybindings
    match (key.modifiers, key.code) {
        (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
            app.cancel();
            return;
        }
        (_, KeyCode::Esc) => {
            app.cancel();
            return;
        }
        _ => {}
    }

    // Enter submits the form globally.
    // Shift+Enter inserts a newline in multi-line text areas.
    if key.code == KeyCode::Enter && !key.modifiers.contains(KeyModifiers::SHIFT) {
        app.submit();
        return;
    }

    // Tab navigation
    match (key.modifiers, key.code) {
        (KeyModifiers::SHIFT, KeyCode::BackTab) => {
            app.focus_prev();
            return;
        }
        (_, KeyCode::Tab) => {
            app.focus_next();
            return;
        }
        _ => {}
    }

    // Delegate to focused widget
    let focused_id = match app.focused_id() {
        Some(id) => id.to_string(),
        None => return,
    };

    crate::widgets::handle_widget_input(app, &focused_id, key);
}

/// Walk the element tree and create an `InputWidget` for every `Input` element,
/// initialising it with the current value from `state`.
fn collect_input_widgets(
    elements: &[Element],
    state: &PopupState,
) -> HashMap<String, InputWidget<'static>> {
    let mut map = HashMap::new();
    collect_input_widgets_inner(elements, state, &mut map);
    map
}

fn collect_input_widgets_inner(
    elements: &[Element],
    state: &PopupState,
    map: &mut HashMap<String, InputWidget<'static>>,
) {
    for element in elements {
        match element {
            Element::Input { id, rows, .. } => {
                let initial = state
                    .get_text(id)
                    .map(|s| s.as_str())
                    .unwrap_or("")
                    .to_string();
                let multiline = rows.is_some_and(|r| r > 1);
                let widget = if multiline {
                    let mut ta = TextArea::from(initial.lines());
                    // Position cursor at end of last line
                    ta.move_cursor(ratatui_textarea::CursorMove::End);
                    InputWidget::MultiLine(Box::new(ta))
                } else {
                    InputWidget::SingleLine(TuiInput::from(initial))
                };
                map.insert(id.clone(), widget);
            }
            Element::Check { reveals, .. } => {
                collect_input_widgets_inner(reveals, state, map);
            }
            Element::Select {
                option_children,
                reveals,
                ..
            } => {
                for children in option_children.values() {
                    collect_input_widgets_inner(children, state, map);
                }
                collect_input_widgets_inner(reveals, state, map);
            }
            Element::Multi {
                option_children,
                reveals,
                ..
            } => {
                for children in option_children.values() {
                    collect_input_widgets_inner(children, state, map);
                }
                collect_input_widgets_inner(reveals, state, map);
            }
            Element::Group { elements, .. } => {
                collect_input_widgets_inner(elements, state, map);
            }
            _ => {}
        }
    }
}

/// Walk the element tree tracking cumulative virtual y, and when `target_id` is found
/// set `result` to `(virtual_y, element_height)`. Stops early once found.
/// Gap logic matches draw_elements_with_offset: gap BETWEEN elements, not after last.
fn find_element_virtual_y(
    elements: &[Element],
    app: &TuiApp,
    target_id: &str,
    width: u16,
    virtual_y: &mut u16,
    result: &mut Option<(u16, u16)>,
) {
    let mut first_visible = true;

    for element in elements {
        if result.is_some() {
            return;
        }

        let when = element_when(element);
        if !app.is_element_visible(when) {
            continue;
        }

        // Gap between elements (not before the first)
        if !first_visible {
            *virtual_y += 1;
        }
        first_visible = false;

        let elem_height = estimate_element_height_for_scroll(element, app, width);

        let elem_id: Option<&str> = match element {
            Element::Slider { id, .. }
            | Element::Input { id, .. }
            | Element::Check { id, .. }
            | Element::Select { id, .. }
            | Element::Multi { id, .. } => Some(id.as_str()),
            _ => None,
        };

        if elem_id == Some(target_id) {
            *result = Some((*virtual_y, elem_height));
            return;
        }

        // Recurse into children to find target within nested elements
        let child_y_start = *virtual_y;
        match element {
            Element::Check { id, reveals, .. } => {
                *virtual_y += 1 + 1; // checkbox line + gap before reveals
                if app.state.get_boolean(id) {
                    find_element_virtual_y(reveals, app, target_id, width, virtual_y, result);
                }
                *virtual_y = child_y_start + elem_height;
            }
            Element::Select {
                id,
                options,
                option_children,
                reveals,
                ..
            } => {
                let list_h = options.len() as u16;
                *virtual_y += 1 + list_h; // label + list
                if let Some(Some(idx)) = app.state.get_choice(id) {
                    if let Some(opt) = options.get(idx) {
                        if let Some(children) = option_children.get(opt.value()) {
                            find_element_virtual_y(children, app, target_id, width, virtual_y, result);
                        }
                    }
                }
                if result.is_none() && app.state.get_choice(id).is_some_and(|c| c.is_some()) {
                    find_element_virtual_y(reveals, app, target_id, width, virtual_y, result);
                }
                *virtual_y = child_y_start + elem_height;
            }
            Element::Multi {
                id,
                options,
                option_children,
                reveals,
                ..
            } => {
                let list_h = options.len() as u16;
                *virtual_y += 1 + list_h;
                if let Some(selections) = app.state.get_multichoice(id) {
                    for (i, &selected) in selections.iter().enumerate() {
                        if selected {
                            if let Some(opt) = options.get(i) {
                                if let Some(children) = option_children.get(opt.value()) {
                                    find_element_virtual_y(children, app, target_id, width, virtual_y, result);
                                }
                            }
                        }
                        if result.is_some() { break; }
                    }
                    if result.is_none() && selections.iter().any(|&s| s) {
                        find_element_virtual_y(reveals, app, target_id, width, virtual_y, result);
                    }
                }
                *virtual_y = child_y_start + elem_height;
            }
            Element::Group { elements, .. } => {
                *virtual_y += 1; // border top
                find_element_virtual_y(elements, app, target_id, width, virtual_y, result);
                *virtual_y = child_y_start + elem_height;
            }
            _ => {
                *virtual_y += elem_height;
            }
        }
    }
}

/// Delegate to the single source of truth in render.rs.
fn estimate_element_height_for_scroll(element: &Element, app: &TuiApp, width: u16) -> u16 {
    crate::render::estimate_single_element_height(element, app, width)
}
