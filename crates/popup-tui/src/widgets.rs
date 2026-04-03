use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use popup_common::Element;
use tui_input::backend::crossterm::EventHandler as _;

use crate::app::{InputWidget, TuiApp};

pub fn handle_widget_input(app: &mut TuiApp, focused_id: &str, key: KeyEvent) {
    let element = find_element_by_id(&app.definition.elements, focused_id).cloned();
    let Some(element) = element else { return };

    match &element {
        Element::Check { id, .. } => handle_check_input(app, id, key),
        Element::Select { id, options, .. } => {
            handle_select_input(app, id, options.len(), key);
        }
        Element::Multi { id, options, .. } => {
            handle_multi_input(app, id, options.len(), key);
        }
        Element::Input { id, .. } => {
            handle_text_input(app, id, key);
        }
        Element::Slider { id, min, max, .. } => {
            handle_numeric_input(app, id, *min, *max, key);
        }
        _ => {}
    }
}

fn handle_check_input(app: &mut TuiApp, id: &str, key: KeyEvent) {
    match key.code {
        KeyCode::Char(' ') => {
            if let Some(val) = app.state.get_boolean_mut(id) {
                *val = !*val;
            }
        }
        _ => {}
    }
}

fn handle_select_input(app: &mut TuiApp, id: &str, option_count: usize, key: KeyEvent) {
    if option_count == 0 {
        return;
    }

    let cursor = app.list_cursors.entry(id.to_string()).or_insert(0);

    match key.code {
        KeyCode::Up => {
            *cursor = cursor.saturating_sub(1);
        }
        KeyCode::Down => {
            if *cursor + 1 < option_count {
                *cursor += 1;
            }
        }
        KeyCode::Char(' ') => {
            let idx = *cursor;
            if let Some(choice) = app.state.get_choice_mut(id) {
                *choice = Some(idx);
            }
        }
        _ => {}
    }
}

fn handle_multi_input(app: &mut TuiApp, id: &str, option_count: usize, key: KeyEvent) {
    if option_count == 0 {
        return;
    }

    let cursor = app.list_cursors.entry(id.to_string()).or_insert(0);

    match key.code {
        KeyCode::Up => {
            *cursor = cursor.saturating_sub(1);
        }
        KeyCode::Down => {
            if *cursor + 1 < option_count {
                *cursor += 1;
            }
        }
        KeyCode::Char(' ') => {
            let idx = *cursor;
            if let Some(selections) = app.state.get_multichoice_mut(id) {
                if let Some(val) = selections.get_mut(idx) {
                    *val = !*val;
                }
            }
        }
        _ => {}
    }
}

fn handle_text_input(app: &mut TuiApp, id: &str, key: KeyEvent) {
    if let Some(widget) = app.input_widgets.get_mut(id) {
        match widget {
            InputWidget::SingleLine(input) => {
                let event = Event::Key(key);
                input.handle_event(&event);
            }
            InputWidget::MultiLine(textarea) => {
                // Shift+Enter inserts a newline; bare Enter is handled globally as submit.
                if key.code == KeyCode::Enter && key.modifiers.contains(KeyModifiers::SHIFT) {
                    textarea.insert_newline();
                } else {
                    let event = Event::Key(key);
                    textarea.input(ratatui_textarea::Input::from(event));
                }
            }
        }
        // Keep state in sync so callers that read state.get_text() see the current value.
        if let Some(text) = app.state.get_text_mut(id) {
            *text = widget.value();
        }
    }
}

fn handle_numeric_input(app: &mut TuiApp, id: &str, min: f32, max: f32, key: KeyEvent) {
    // We store the numeric value as a Number in state, but for editing we convert
    // to/from a string representation stored in a scratch buffer.
    // For simplicity, we work with the numeric value directly via arrow keys and typed digits.
    let Some(val) = app.state.get_number_mut(id) else {
        return;
    };

    match key.code {
        KeyCode::Up => {
            *val = (*val + 1.0).min(max);
        }
        KeyCode::Down => {
            *val = (*val - 1.0).max(min);
        }
        KeyCode::Char(c) if c.is_ascii_digit() || c == '.' || c == '-' => {
            // Build string representation, append char, parse back
            let mut s = format!("{}", *val as i32);
            s.push(c);
            if let Ok(n) = s.parse::<f32>() {
                *val = n.clamp(min, max);
            }
        }
        KeyCode::Backspace => {
            let mut s = format!("{}", *val as i32);
            s.pop();
            if s.is_empty() || s == "-" {
                *val = min;
            } else if let Ok(n) = s.parse::<f32>() {
                *val = n.clamp(min, max);
            }
        }
        _ => {}
    }
}

fn find_element_by_id<'a>(elements: &'a [Element], target_id: &str) -> Option<&'a Element> {
    for element in elements {
        match element {
            Element::Slider { id, .. }
            | Element::Check { id, .. }
            | Element::Input { id, .. }
            | Element::Multi { id, .. }
            | Element::Select { id, .. }
                if id == target_id =>
            {
                return Some(element);
            }
            Element::Check { reveals, .. } => {
                if let Some(e) = find_element_by_id(reveals, target_id) {
                    return Some(e);
                }
            }
            Element::Multi {
                reveals,
                option_children,
                ..
            }
            | Element::Select {
                reveals,
                option_children,
                ..
            } => {
                if let Some(e) = find_element_by_id(reveals, target_id) {
                    return Some(e);
                }
                for children in option_children.values() {
                    if let Some(e) = find_element_by_id(children, target_id) {
                        return Some(e);
                    }
                }
            }
            Element::Group { elements, .. } => {
                if let Some(e) = find_element_by_id(elements, target_id) {
                    return Some(e);
                }
            }
            _ => {}
        }
    }
    None
}
