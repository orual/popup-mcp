use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;

use popup_common::Element;

use crate::app::{element_when, InputWidget, TuiApp};

const LABEL_STYLE: Style = Style::new().fg(Color::Cyan);
const FOCUSED_STYLE: Style = Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD);
const DIM_STYLE: Style = Style::new().fg(Color::DarkGray);
const SELECTED_STYLE: Style = Style::new().fg(Color::Green).add_modifier(Modifier::BOLD);

pub fn draw(frame: &mut Frame, app: &TuiApp) {
    let area = frame.area();

    let chunks = Layout::vertical([
        Constraint::Length(3), // title
        Constraint::Min(1),   // content
        Constraint::Length(1), // status bar
    ])
    .split(area);

    draw_title(frame, chunks[0], app);
    draw_elements(frame, chunks[1], app, &app.definition.elements, 0);
    draw_status_bar(frame, chunks[2]);
}

fn draw_title(frame: &mut Frame, area: Rect, app: &TuiApp) {
    let title = Paragraph::new(app.definition.effective_title())
        .style(Style::new().fg(Color::Magenta).add_modifier(Modifier::BOLD))
        .block(Block::default().borders(Borders::BOTTOM));
    frame.render_widget(title, area);
}

fn draw_status_bar(frame: &mut Frame, area: Rect) {
    let bar = Paragraph::new(Line::from(vec![
        Span::styled("Enter", Style::new().fg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::raw(" Submit  "),
        Span::styled("Esc", Style::new().fg(Color::Red).add_modifier(Modifier::BOLD)),
        Span::raw(" Cancel  "),
        Span::styled("Tab", Style::new().fg(Color::Yellow)),
        Span::raw("/"),
        Span::styled("S-Tab", Style::new().fg(Color::Yellow)),
        Span::raw(" Navigate  "),
        Span::styled("Space", Style::new().fg(Color::Yellow)),
        Span::raw(" Select"),
    ]))
    .style(DIM_STYLE);
    frame.render_widget(bar, area);
}

fn draw_elements(
    frame: &mut Frame,
    area: Rect,
    app: &TuiApp,
    elements: &[Element],
    indent: u16,
) {
    draw_elements_with_offset(frame, area, app, elements, indent, app.scroll_offset);
}

fn draw_elements_with_offset(
    frame: &mut Frame,
    area: Rect,
    app: &TuiApp,
    elements: &[Element],
    indent: u16,
    scroll_offset: u16,
) {
    // `virtual_y` tracks the absolute y position as if the content area were unlimited.
    // Elements above the scroll_offset are skipped; elements below the visible window are clipped.
    let mut virtual_y: i32 = 0;

    for element in elements {
        let when = element_when(element);
        if !app.is_element_visible(when) {
            continue;
        }

        let element_height = estimate_single_element_height(element, app, area.width.saturating_sub(indent)) as i32;
        let top = virtual_y - scroll_offset as i32;

        if top + element_height > 0 && top < area.height as i32 {
            // Some part of this element is visible
            let screen_y = area.y as i32 + top;
            let clipped_y = screen_y.max(area.y as i32) as u16;
            let clipped_height = (screen_y + element_height)
                .min(area.y as i32 + area.height as i32)
                - clipped_y as i32;

            if clipped_height > 0 {
                let remaining = Rect::new(
                    area.x + indent,
                    clipped_y,
                    area.width.saturating_sub(indent),
                    clipped_height as u16,
                );
                draw_single_element(frame, remaining, app, element, indent);
            }
        }

        virtual_y += element_height as i32 + 1; // +1 for gap
    }
}

fn draw_single_element(
    frame: &mut Frame,
    area: Rect,
    app: &TuiApp,
    element: &Element,
    indent: u16,
) -> u16 {
    match element {
        Element::Text { text, .. } => draw_text(frame, area, text),
        Element::Markdown { markdown, .. } => draw_markdown(frame, area, markdown),
        Element::Check {
            check, id, reveals, ..
        } => draw_check(frame, area, app, check, id, reveals, indent),
        Element::Input {
            input,
            id,
            placeholder,
            rows,
            ..
        } => draw_input(frame, area, app, input, id, placeholder.as_deref(), *rows),
        Element::Select {
            select,
            id,
            options,
            option_children,
            reveals,
            ..
        } => draw_select(frame, area, app, select, id, options, option_children, reveals, indent),
        Element::Multi {
            multi,
            id,
            options,
            option_children,
            reveals,
            ..
        } => draw_multi(frame, area, app, multi, id, options, option_children, reveals, indent),
        Element::Slider {
            slider,
            id,
            min,
            max,
            ..
        } => draw_numeric(frame, area, app, slider, id, *min, *max),
        Element::Group {
            group, elements, ..
        } => draw_group(frame, area, app, group, elements, indent),
    }
}

fn draw_text(frame: &mut Frame, area: Rect, text: &str) -> u16 {
    let paragraph = Paragraph::new(text).wrap(Wrap { trim: false });
    let height = paragraph.line_count(area.width) as u16;
    let render_area = Rect::new(area.x, area.y, area.width, height.min(area.height));
    frame.render_widget(paragraph, render_area);
    height
}

fn draw_markdown(frame: &mut Frame, area: Rect, markdown: &str) -> u16 {
    let text = tui_markdown::from_str(markdown);
    let paragraph = Paragraph::new(text).wrap(Wrap { trim: false });
    let height = paragraph.line_count(area.width) as u16;
    let render_area = Rect::new(area.x, area.y, area.width, height.min(area.height));
    frame.render_widget(paragraph, render_area);
    height
}

fn draw_check(
    frame: &mut Frame,
    area: Rect,
    app: &TuiApp,
    label: &str,
    id: &str,
    reveals: &[Element],
    indent: u16,
) -> u16 {
    let checked = app.state.get_boolean(id);
    let is_focused = app.focused_id() == Some(id);

    let marker = if checked { "[x]" } else { "[ ]" };
    let style = if is_focused { FOCUSED_STYLE } else { LABEL_STYLE };

    let line = Line::from(vec![
        Span::styled(marker, style),
        Span::raw(" "),
        Span::styled(label, style),
    ]);
    let paragraph = Paragraph::new(line);
    let render_area = Rect::new(area.x, area.y, area.width, 1.min(area.height));
    frame.render_widget(paragraph, render_area);

    let mut total_height = 1u16;

    // Render reveals if checked
    if checked && !reveals.is_empty() {
        let reveals_area = Rect::new(
            area.x,
            area.y + total_height,
            area.width,
            area.height.saturating_sub(total_height),
        );
        draw_elements(frame, reveals_area, app, reveals, indent + 2);
        // Estimate height of reveals (simplified — accurate tracking would need layout pass)
        total_height += estimate_elements_height(reveals, app) + 1;
    }

    total_height
}

fn draw_input(
    frame: &mut Frame,
    area: Rect,
    app: &TuiApp,
    label: &str,
    id: &str,
    placeholder: Option<&str>,
    rows: Option<u32>,
) -> u16 {
    let is_focused = app.focused_id() == Some(id);
    let label_style = if is_focused { FOCUSED_STYLE } else { LABEL_STYLE };

    // Label
    let label_line = Paragraph::new(Span::styled(label, label_style));
    if area.height < 2 {
        return 0;
    }
    frame.render_widget(label_line, Rect::new(area.x, area.y, area.width, 1));

    let input_height = rows.unwrap_or(1).max(1) as u16;
    let border_style = if is_focused {
        Style::new().fg(Color::Yellow)
    } else {
        Style::new().fg(Color::DarkGray)
    };
    let input_area = Rect::new(
        area.x,
        area.y + 1,
        area.width,
        (input_height + 2).min(area.height.saturating_sub(1)), // +2 for borders
    );

    match app.input_widgets.get(id) {
        Some(InputWidget::SingleLine(input)) => {
            // tui-input renders as a scrolled Paragraph; we position the terminal cursor.
            let width = input_area.width.saturating_sub(2); // subtract borders
            let scroll = input.visual_scroll(width as usize);
            let text = input.value();
            let (display, style) = if text.is_empty() {
                (
                    placeholder.unwrap_or("").to_string(),
                    DIM_STYLE,
                )
            } else {
                (text.to_string(), Style::default())
            };

            let content = Paragraph::new(display)
                .style(style)
                .scroll((0, scroll as u16))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(border_style),
                );
            frame.render_widget(content, input_area);

            if is_focused {
                let cursor_x = input.visual_cursor().max(scroll) - scroll;
                frame.set_cursor_position((
                    input_area.x + 1 + cursor_x as u16,
                    input_area.y + 1,
                ));
            }
        }
        Some(InputWidget::MultiLine(textarea)) => {
            // ratatui-textarea implements Widget for &TextArea directly.
            // We clone to set a block style without mutating the stored widget.
            let mut ta: ratatui_textarea::TextArea = (**textarea).clone();
            ta.set_block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(border_style),
            );
            if textarea.lines().iter().all(|l| l.is_empty()) {
                if let Some(ph) = placeholder {
                    ta.set_placeholder_text(ph);
                    ta.set_placeholder_style(DIM_STYLE);
                }
            }
            frame.render_widget(&ta, input_area);
        }
        None => {
            // Fallback: plain paragraph (widget not yet initialized)
            let text = app.state.get_text(id).map(|s| s.as_str()).unwrap_or("");
            let (display, style) = if text.is_empty() {
                (placeholder.unwrap_or(""), DIM_STYLE)
            } else {
                (text, Style::default())
            };
            let content = Paragraph::new(display)
                .style(style)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(border_style),
                )
                .wrap(Wrap { trim: false });
            frame.render_widget(content, input_area);
        }
    }

    1 + input_height + 2 // label + content + borders
}

#[allow(clippy::too_many_arguments)]
fn draw_select(
    frame: &mut Frame,
    area: Rect,
    app: &TuiApp,
    label: &str,
    id: &str,
    options: &[popup_common::OptionValue],
    option_children: &std::collections::HashMap<String, Vec<Element>>,
    reveals: &[Element],
    indent: u16,
) -> u16 {
    let is_focused = app.focused_id() == Some(id);
    let label_style = if is_focused { FOCUSED_STYLE } else { LABEL_STYLE };
    let selected_idx = app.state.get_choice(id).flatten();
    let cursor = *app.list_cursors.get(id).unwrap_or(&0);

    // Label
    frame.render_widget(
        Paragraph::new(Span::styled(label, label_style)),
        Rect::new(area.x, area.y, area.width, 1.min(area.height)),
    );

    if area.height < 2 {
        return 1;
    }

    // Options list
    let items: Vec<ListItem> = options
        .iter()
        .enumerate()
        .map(|(i, opt)| {
            let marker = if selected_idx == Some(i) { "● " } else { "○ " };
            let mut spans = vec![Span::raw(marker), Span::raw(opt.value())];
            if let Some(desc) = opt.description() {
                spans.push(Span::styled(format!("  ({})", desc), DIM_STYLE));
            }
            let style = if selected_idx == Some(i) {
                SELECTED_STYLE
            } else {
                Style::default()
            };
            ListItem::new(Line::from(spans)).style(style)
        })
        .collect();

    let list_height = (options.len() as u16).min(area.height.saturating_sub(1));
    let list_area = Rect::new(area.x, area.y + 1, area.width, list_height);

    let highlight_style = if is_focused {
        Style::new().bg(Color::DarkGray)
    } else {
        Style::default()
    };

    let list = List::new(items).highlight_style(highlight_style);
    let mut list_state = ListState::default().with_selected(Some(cursor));
    frame.render_stateful_widget(list, list_area, &mut list_state);

    let mut total_height = 1 + list_height;

    // Option children for selected option
    if let Some(idx) = selected_idx {
        if let Some(opt) = options.get(idx) {
            if let Some(children) = option_children.get(opt.value()) {
                if !children.is_empty() {
                    let children_area = Rect::new(
                        area.x,
                        area.y + total_height,
                        area.width,
                        area.height.saturating_sub(total_height),
                    );
                    draw_elements(frame, children_area, app, children, indent + 2);
                    total_height += estimate_elements_height(children, app);
                }
            }
        }
    }

    // Reveals
    if selected_idx.is_some() && !reveals.is_empty() {
        let reveals_area = Rect::new(
            area.x,
            area.y + total_height,
            area.width,
            area.height.saturating_sub(total_height),
        );
        draw_elements(frame, reveals_area, app, reveals, indent + 2);
        total_height += estimate_elements_height(reveals, app);
    }

    total_height
}

#[allow(clippy::too_many_arguments)]
fn draw_multi(
    frame: &mut Frame,
    area: Rect,
    app: &TuiApp,
    label: &str,
    id: &str,
    options: &[popup_common::OptionValue],
    option_children: &std::collections::HashMap<String, Vec<Element>>,
    reveals: &[Element],
    indent: u16,
) -> u16 {
    let is_focused = app.focused_id() == Some(id);
    let label_style = if is_focused { FOCUSED_STYLE } else { LABEL_STYLE };
    let selections = app.state.get_multichoice(id).cloned().unwrap_or_default();
    let cursor = *app.list_cursors.get(id).unwrap_or(&0);

    // Label
    frame.render_widget(
        Paragraph::new(Span::styled(label, label_style)),
        Rect::new(area.x, area.y, area.width, 1.min(area.height)),
    );

    if area.height < 2 {
        return 1;
    }

    // Options with checkboxes
    let items: Vec<ListItem> = options
        .iter()
        .enumerate()
        .map(|(i, opt)| {
            let checked = selections.get(i).copied().unwrap_or(false);
            let marker = if checked { "[x] " } else { "[ ] " };
            let mut spans = vec![Span::raw(marker), Span::raw(opt.value())];
            if let Some(desc) = opt.description() {
                spans.push(Span::styled(format!("  ({})", desc), DIM_STYLE));
            }
            let style = if checked {
                SELECTED_STYLE
            } else {
                Style::default()
            };
            ListItem::new(Line::from(spans)).style(style)
        })
        .collect();

    let list_height = (options.len() as u16).min(area.height.saturating_sub(1));
    let list_area = Rect::new(area.x, area.y + 1, area.width, list_height);

    let highlight_style = if is_focused {
        Style::new().bg(Color::DarkGray)
    } else {
        Style::default()
    };

    let list = List::new(items).highlight_style(highlight_style);
    let mut list_state = ListState::default().with_selected(Some(cursor));
    frame.render_stateful_widget(list, list_area, &mut list_state);

    let mut total_height = 1 + list_height;

    // Option children for selected options
    for (i, &selected) in selections.iter().enumerate() {
        if selected {
            if let Some(opt) = options.get(i) {
                if let Some(children) = option_children.get(opt.value()) {
                    if !children.is_empty() {
                        let children_area = Rect::new(
                            area.x,
                            area.y + total_height,
                            area.width,
                            area.height.saturating_sub(total_height),
                        );
                        draw_elements(frame, children_area, app, children, indent + 2);
                        total_height += estimate_elements_height(children, app);
                    }
                }
            }
        }
    }

    // Reveals
    if selections.iter().any(|&s| s) && !reveals.is_empty() {
        let reveals_area = Rect::new(
            area.x,
            area.y + total_height,
            area.width,
            area.height.saturating_sub(total_height),
        );
        draw_elements(frame, reveals_area, app, reveals, indent + 2);
        total_height += estimate_elements_height(reveals, app);
    }

    total_height
}

fn draw_numeric(
    frame: &mut Frame,
    area: Rect,
    app: &TuiApp,
    label: &str,
    id: &str,
    min: f32,
    max: f32,
) -> u16 {
    let is_focused = app.focused_id() == Some(id);
    let label_style = if is_focused { FOCUSED_STYLE } else { LABEL_STYLE };

    // Label with range hint
    let label_text = format!("{} ({} - {})", label, min as i32, max as i32);
    frame.render_widget(
        Paragraph::new(Span::styled(&label_text, label_style)),
        Rect::new(area.x, area.y, area.width, 1.min(area.height)),
    );

    if area.height < 2 {
        return 1;
    }

    let val = app
        .state
        .values
        .get(id)
        .and_then(|v| match v {
            popup_common::ElementValue::Number(n) => Some(*n as i32),
            _ => None,
        })
        .unwrap_or(min as i32);

    let border_style = if is_focused {
        Style::new().fg(Color::Yellow)
    } else {
        Style::new().fg(Color::DarkGray)
    };

    let display = if is_focused {
        format!("{}█", val)
    } else {
        format!("{}", val)
    };

    let input = Paragraph::new(display)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style),
        );

    let input_area = Rect::new(area.x, area.y + 1, area.width.min(20), 3.min(area.height - 1));
    frame.render_widget(input, input_area);

    // Hint
    if is_focused && area.height > 4 {
        let hint = Paragraph::new(Span::styled("↑/↓ to adjust, type digits", DIM_STYLE));
        frame.render_widget(
            hint,
            Rect::new(area.x, area.y + 4, area.width, 1),
        );
        return 5;
    }

    4 // label + bordered input (3 lines)
}

fn draw_group(
    frame: &mut Frame,
    area: Rect,
    app: &TuiApp,
    label: &str,
    elements: &[Element],
    indent: u16,
) -> u16 {
    let block = Block::default()
        .title(label)
        .borders(Borders::ALL)
        .border_style(Style::new().fg(Color::DarkGray));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    draw_elements(frame, inner, app, elements, indent);

    // Return estimated height
    let content_height = estimate_elements_height(elements, app);
    content_height + 2 // +2 for borders
}

fn estimate_elements_height(elements: &[Element], app: &TuiApp) -> u16 {
    estimate_elements_height_width(elements, app, 80)
}

fn estimate_elements_height_width(elements: &[Element], app: &TuiApp, width: u16) -> u16 {
    let mut height = 0u16;
    for element in elements {
        let when = element_when(element);
        if !app.is_element_visible(when) {
            continue;
        }
        height += estimate_single_element_height(element, app, width) + 1; // +1 for gap
    }
    height
}

/// Estimate the rendered height of a single element given the available width.
/// Mirrors the actual draw functions so scroll tracking stays consistent.
pub(crate) fn estimate_single_element_height(element: &Element, app: &TuiApp, width: u16) -> u16 {
    match element {
        Element::Text { text, .. } => {
            let p = Paragraph::new(text.as_str()).wrap(Wrap { trim: false });
            (p.line_count(width) as u16).max(1)
        }
        Element::Markdown { markdown, .. } => {
            let text = tui_markdown::from_str(markdown);
            let p = Paragraph::new(text).wrap(Wrap { trim: false });
            (p.line_count(width) as u16).max(1)
        }
        Element::Check { id, reveals, .. } => {
            let mut h = 1u16;
            if app.state.get_boolean(id) && !reveals.is_empty() {
                h += estimate_elements_height_width(reveals, app, width) + 1;
            }
            h
        }
        Element::Input { rows, .. } => 1 + rows.unwrap_or(1).max(1) as u16 + 2,
        Element::Slider { .. } => 4,
        Element::Select { id, options, option_children, reveals, .. } => {
            let mut h = 1 + options.len() as u16;
            if let Some(Some(idx)) = app.state.get_choice(id) {
                if let Some(opt) = options.get(idx) {
                    if let Some(children) = option_children.get(opt.value()) {
                        h += estimate_elements_height_width(children, app, width);
                    }
                }
                if !reveals.is_empty() {
                    h += estimate_elements_height_width(reveals, app, width);
                }
            }
            h
        }
        Element::Multi { id, options, option_children, reveals, .. } => {
            let mut h = 1 + options.len() as u16;
            if let Some(selections) = app.state.get_multichoice(id) {
                for (i, &selected) in selections.iter().enumerate() {
                    if selected {
                        if let Some(opt) = options.get(i) {
                            if let Some(children) = option_children.get(opt.value()) {
                                h += estimate_elements_height_width(children, app, width);
                            }
                        }
                    }
                }
                if selections.iter().any(|&s| s) && !reveals.is_empty() {
                    h += estimate_elements_height_width(reveals, app, width);
                }
            }
            h
        }
        Element::Group { elements, .. } => {
            estimate_elements_height_width(elements, app, width.saturating_sub(2)) + 2
        }
    }
}

