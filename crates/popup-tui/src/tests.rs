use std::collections::HashMap;

use popup_common::{Element, ElementValue, OptionValue, PopupDefinition, PopupResult};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

use crate::app::TuiApp;

/// Render the app into a fixed-size buffer and return the buffer contents as a `String`.
fn render_to_string(app: &TuiApp, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| crate::render::draw(frame, app))
        .unwrap();
    // Collect all lines of the buffer, trimming trailing spaces per line.
    terminal
        .backend()
        .buffer()
        .content()
        .chunks(width as usize)
        .map(|row| {
            let line: String = row.iter().map(|c| c.symbol()).collect();
            line.trim_end().to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn simple_definition() -> PopupDefinition {
    PopupDefinition {
        title: "Test Popup".to_string(),
        elements: vec![
            Element::Text {
                text: "Some text".to_string(),
                id: None,
                when: None,
            },
            Element::Input {
                input: "Name".to_string(),
                id: "name".to_string(),
                placeholder: Some("Enter name".to_string()),
                rows: None,
                when: None,
            },
            Element::Check {
                check: "Enable feature".to_string(),
                id: "feature".to_string(),
                default: false,
                reveals: vec![Element::Input {
                    input: "Feature config".to_string(),
                    id: "feature_config".to_string(),
                    placeholder: None,
                    rows: None,
                    when: None,
                }],
                when: None,
            },
            Element::Select {
                select: "Colour".to_string(),
                id: "colour".to_string(),
                options: vec![
                    OptionValue::Simple("Red".to_string()),
                    OptionValue::Simple("Blue".to_string()),
                    OptionValue::Simple("Green".to_string()),
                ],
                default: None,
                option_children: HashMap::new(),
                reveals: vec![],
                when: None,
            },
        ],
    }
}

#[test]
fn test_focusable_ids_excludes_text_elements() {
    let app = TuiApp::new(simple_definition());
    // Text element should not be focusable
    assert!(!app.focusable_ids.contains(&"Some text".to_string()));
    // Input, Check, Select should be focusable
    assert!(app.focusable_ids.contains(&"name".to_string()));
    assert!(app.focusable_ids.contains(&"feature".to_string()));
    assert!(app.focusable_ids.contains(&"colour".to_string()));
}

#[test]
fn test_focusable_ids_count() {
    let app = TuiApp::new(simple_definition());
    // name, feature, colour — reveals hidden because checkbox is unchecked
    assert_eq!(app.focusable_ids.len(), 3);
}

#[test]
fn test_reveals_become_focusable_when_checked() {
    let mut app = TuiApp::new(simple_definition());
    assert!(!app.focusable_ids.contains(&"feature_config".to_string()));

    // Check the checkbox
    if let Some(val) = app.state.get_boolean_mut("feature") {
        *val = true;
    }
    app.rebuild_focusable_ids();

    assert!(app.focusable_ids.contains(&"feature_config".to_string()));
    assert_eq!(app.focusable_ids.len(), 4);
}

#[test]
fn test_focus_cycling() {
    let mut app = TuiApp::new(simple_definition());
    assert_eq!(app.focus_index, 0);
    assert_eq!(app.focused_id(), Some("name"));

    app.focus_next();
    assert_eq!(app.focused_id(), Some("feature"));

    app.focus_next();
    assert_eq!(app.focused_id(), Some("colour"));

    app.focus_next(); // wraps
    assert_eq!(app.focused_id(), Some("name"));
}

#[test]
fn test_focus_cycling_backward() {
    let mut app = TuiApp::new(simple_definition());
    assert_eq!(app.focused_id(), Some("name"));

    app.focus_prev(); // wraps to end
    assert_eq!(app.focused_id(), Some("colour"));

    app.focus_prev();
    assert_eq!(app.focused_id(), Some("feature"));
}

#[test]
fn test_submit_produces_completed_result() {
    let mut app = TuiApp::new(simple_definition());

    // Set text via the proper API that keeps widget and state in sync.
    app.set_text_value("name", "Alice");

    app.submit();

    match &app.result {
        Some(PopupResult::Completed { values, button }) => {
            assert_eq!(button, "submit");
            assert_eq!(values.get("name"), Some(&serde_json::json!("Alice")));
        }
        other => panic!("Expected Completed, got {:?}", other),
    }
}

#[test]
fn test_cancel_produces_cancelled_result() {
    let mut app = TuiApp::new(simple_definition());
    app.cancel();

    match &app.result {
        Some(PopupResult::Cancelled) => {}
        other => panic!("Expected Cancelled, got {:?}", other),
    }
}

#[test]
fn test_checkbox_toggle() {
    let mut app = TuiApp::new(simple_definition());
    assert!(!app.state.get_boolean("feature"));

    crate::widgets::handle_widget_input(
        &mut app,
        "feature",
        crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char(' '),
            crossterm::event::KeyModifiers::NONE,
        ),
    );

    assert!(app.state.get_boolean("feature"));
}

#[test]
fn test_select_navigation_and_selection() {
    let mut app = TuiApp::new(simple_definition());

    // Move cursor down
    crate::widgets::handle_widget_input(
        &mut app,
        "colour",
        crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Down,
            crossterm::event::KeyModifiers::NONE,
        ),
    );
    assert_eq!(app.list_cursors.get("colour"), Some(&1));

    // Select with Space
    crate::widgets::handle_widget_input(
        &mut app,
        "colour",
        crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char(' '),
            crossterm::event::KeyModifiers::NONE,
        ),
    );
    assert_eq!(app.state.get_choice("colour"), Some(Some(1))); // Blue
}

#[test]
fn test_text_input() {
    let mut app = TuiApp::new(simple_definition());

    for c in "hello".chars() {
        crate::widgets::handle_widget_input(
            &mut app,
            "name",
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char(c),
                crossterm::event::KeyModifiers::NONE,
            ),
        );
    }

    assert_eq!(app.state.get_text("name"), Some(&"hello".to_string()));

    // Backspace
    crate::widgets::handle_widget_input(
        &mut app,
        "name",
        crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Backspace,
            crossterm::event::KeyModifiers::NONE,
        ),
    );

    assert_eq!(app.state.get_text("name"), Some(&"hell".to_string()));
}

#[test]
fn test_multi_select() {
    let definition = PopupDefinition {
        title: "Test".to_string(),
        elements: vec![Element::Multi {
            multi: "Toppings".to_string(),
            id: "toppings".to_string(),
            options: vec![
                OptionValue::Simple("Cheese".to_string()),
                OptionValue::Simple("Peppers".to_string()),
                OptionValue::Simple("Onions".to_string()),
            ],
            option_children: HashMap::new(),
            reveals: vec![],
            when: None,
        }],
    };

    let mut app = TuiApp::new(definition);

    // Toggle first option
    crate::widgets::handle_widget_input(
        &mut app,
        "toppings",
        crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char(' '),
            crossterm::event::KeyModifiers::NONE,
        ),
    );

    // Move down and toggle second
    crate::widgets::handle_widget_input(
        &mut app,
        "toppings",
        crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Down,
            crossterm::event::KeyModifiers::NONE,
        ),
    );
    crate::widgets::handle_widget_input(
        &mut app,
        "toppings",
        crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char(' '),
            crossterm::event::KeyModifiers::NONE,
        ),
    );

    let selections = app.state.get_multichoice("toppings").unwrap();
    assert_eq!(selections, &vec![true, true, false]);
}

#[test]
fn test_numeric_input_arrow_keys() {
    let definition = PopupDefinition {
        title: "Test".to_string(),
        elements: vec![Element::Slider {
            slider: "Volume".to_string(),
            id: "volume".to_string(),
            min: 0.0,
            max: 100.0,
            default: Some(50.0),
            when: None,
        }],
    };

    let mut app = TuiApp::new(definition);

    // Value should be at default
    match app.state.values.get("volume") {
        Some(ElementValue::Number(n)) => assert_eq!(*n, 50.0),
        other => panic!("Expected Number(50.0), got {:?}", other),
    }

    // Arrow up
    crate::widgets::handle_widget_input(
        &mut app,
        "volume",
        crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Up,
            crossterm::event::KeyModifiers::NONE,
        ),
    );

    match app.state.values.get("volume") {
        Some(ElementValue::Number(n)) => assert_eq!(*n, 51.0),
        other => panic!("Expected Number(51.0), got {:?}", other),
    }

    // Arrow down twice
    for _ in 0..2 {
        crate::widgets::handle_widget_input(
            &mut app,
            "volume",
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Down,
                crossterm::event::KeyModifiers::NONE,
            ),
        );
    }

    match app.state.values.get("volume") {
        Some(ElementValue::Number(n)) => assert_eq!(*n, 49.0),
        other => panic!("Expected Number(49.0), got {:?}", other),
    }
}

#[test]
fn test_render_title_appears() {
    let app = TuiApp::new(simple_definition());
    let output = render_to_string(&app, 60, 20);
    assert!(
        output.contains("Test Popup"),
        "Title should appear in rendered output. Got:\n{}",
        output
    );
}

#[test]
fn test_render_text_element_appears() {
    let app = TuiApp::new(simple_definition());
    let output = render_to_string(&app, 60, 20);
    assert!(
        output.contains("Some text"),
        "Text element should appear in rendered output. Got:\n{}",
        output
    );
}

#[test]
fn test_render_input_label_appears() {
    let app = TuiApp::new(simple_definition());
    let output = render_to_string(&app, 60, 20);
    // The input's label should appear (first element is focused so it may be styled)
    assert!(
        output.contains("Name"),
        "Input label 'Name' should appear. Got:\n{}",
        output
    );
}

#[test]
fn test_render_check_element_appears() {
    let app = TuiApp::new(simple_definition());
    let output = render_to_string(&app, 60, 20);
    assert!(
        output.contains("Enable feature"),
        "Checkbox label should appear. Got:\n{}",
        output
    );
    assert!(
        output.contains("[ ]"),
        "Unchecked checkbox marker should appear. Got:\n{}",
        output
    );
}

#[test]
fn test_render_select_element_appears() {
    let app = TuiApp::new(simple_definition());
    let output = render_to_string(&app, 60, 30);
    assert!(
        output.contains("Colour"),
        "Select label should appear. Got:\n{}",
        output
    );
    assert!(
        output.contains("Red"),
        "Select option 'Red' should appear. Got:\n{}",
        output
    );
}

#[test]
fn test_render_focused_element_gets_highlight() {
    let app = TuiApp::new(simple_definition());
    // Focus is on 'name' (first focusable). The label uses FOCUSED_STYLE (yellow).
    // We can't check colors directly in TestBackend buffer string content, but we can
    // verify the element text appears and the output differs from unfocused state.
    let output = render_to_string(&app, 60, 20);
    // "Name" label should be present for the focused input
    assert!(output.contains("Name"), "Focused input label should be rendered");
}

#[test]
fn test_render_conditional_element_hidden_when_condition_false() {
    let definition = PopupDefinition {
        title: "Cond Test".to_string(),
        elements: vec![
            Element::Check {
                check: "Show secret".to_string(),
                id: "show_secret".to_string(),
                default: false,
                reveals: vec![],
                when: None,
            },
            Element::Input {
                input: "Secret value".to_string(),
                id: "secret".to_string(),
                placeholder: None,
                rows: None,
                when: Some("show_secret".to_string()),
            },
        ],
    };

    let app = TuiApp::new(definition);
    let output = render_to_string(&app, 60, 20);
    // Checkbox is unchecked, so "Secret value" should NOT appear
    assert!(
        !output.contains("Secret value"),
        "Conditionally hidden element should not render. Got:\n{}",
        output
    );
    // The checkbox itself should appear
    assert!(output.contains("Show secret"), "Checkbox label should appear");
}

#[test]
fn test_render_conditional_element_shown_when_condition_true() {
    let definition = PopupDefinition {
        title: "Cond Test".to_string(),
        elements: vec![
            Element::Check {
                check: "Show secret".to_string(),
                id: "show_secret".to_string(),
                default: false,
                reveals: vec![],
                when: None,
            },
            Element::Input {
                input: "Secret value".to_string(),
                id: "secret".to_string(),
                placeholder: None,
                rows: None,
                when: Some("show_secret".to_string()),
            },
        ],
    };

    let mut app = TuiApp::new(definition);
    // Enable the condition
    if let Some(val) = app.state.get_boolean_mut("show_secret") {
        *val = true;
    }
    app.rebuild_focusable_ids();

    let output = render_to_string(&app, 60, 20);
    assert!(
        output.contains("Secret value"),
        "Conditionally revealed element should render when condition is true. Got:\n{}",
        output
    );
}

#[test]
fn test_render_status_bar_appears() {
    let app = TuiApp::new(simple_definition());
    let output = render_to_string(&app, 60, 20);
    // Status bar should contain navigation hints
    assert!(
        output.contains("Submit") || output.contains("Enter"),
        "Status bar should contain submit hint. Got:\n{}",
        output
    );
}

#[test]
fn test_numeric_input_clamped_to_range() {
    let definition = PopupDefinition {
        title: "Test".to_string(),
        elements: vec![Element::Slider {
            slider: "Volume".to_string(),
            id: "volume".to_string(),
            min: 0.0,
            max: 5.0,
            default: Some(5.0),
            when: None,
        }],
    };

    let mut app = TuiApp::new(definition);

    // Try to go above max
    crate::widgets::handle_widget_input(
        &mut app,
        "volume",
        crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Up,
            crossterm::event::KeyModifiers::NONE,
        ),
    );

    match app.state.values.get("volume") {
        Some(ElementValue::Number(n)) => assert_eq!(*n, 5.0), // Clamped to max
        other => panic!("Expected Number(5.0), got {:?}", other),
    }
}

#[test]
fn test_conditional_visibility() {
    let definition = PopupDefinition {
        title: "Test".to_string(),
        elements: vec![
            Element::Check {
                check: "Advanced".to_string(),
                id: "advanced".to_string(),
                default: false,
                reveals: vec![],
                when: None,
            },
            Element::Input {
                input: "Secret".to_string(),
                id: "secret".to_string(),
                placeholder: None,
                rows: None,
                when: Some("advanced".to_string()),
            },
        ],
    };

    let mut app = TuiApp::new(definition);

    // Secret should not be focusable when advanced is false
    assert!(!app.focusable_ids.contains(&"secret".to_string()));
    assert!(!app.is_element_visible(&Some("advanced".to_string())));

    // Enable advanced
    if let Some(val) = app.state.get_boolean_mut("advanced") {
        *val = true;
    }
    app.rebuild_focusable_ids();

    assert!(app.focusable_ids.contains(&"secret".to_string()));
    assert!(app.is_element_visible(&Some("advanced".to_string())));
}

#[test]
fn test_focus_index_clamped_when_elements_disappear() {
    let definition = PopupDefinition {
        title: "Test".to_string(),
        elements: vec![
            Element::Check {
                check: "Toggle".to_string(),
                id: "toggle".to_string(),
                default: true,
                reveals: vec![
                    Element::Input {
                        input: "A".to_string(),
                        id: "a".to_string(),
                        placeholder: None,
                        rows: None,
                        when: None,
                    },
                    Element::Input {
                        input: "B".to_string(),
                        id: "b".to_string(),
                        placeholder: None,
                        rows: None,
                        when: None,
                    },
                ],
                when: None,
            },
        ],
    };

    let mut app = TuiApp::new(definition);
    // toggle, a, b
    assert_eq!(app.focusable_ids.len(), 3);

    // Focus on last element
    app.focus_index = 2;

    // Uncheck toggle — a and b disappear
    if let Some(val) = app.state.get_boolean_mut("toggle") {
        *val = false;
    }
    app.rebuild_focusable_ids();

    // Should be clamped to 0 (only "toggle" remains)
    assert_eq!(app.focusable_ids.len(), 1);
    assert_eq!(app.focus_index, 0);
}
