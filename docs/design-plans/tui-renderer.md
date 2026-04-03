# TUI Renderer for popup-mcp

## Summary

Add a ratatui-based TUI renderer to popup-mcp for use over SSH sessions. The binary auto-detects the environment (GUI display vs zellij terminal) and dispatches to the appropriate renderer. Agents don't need to know or care which renderer is used — same JSON in, same JSON out.

## Motivation

popup-mcp currently requires a display server (Wayland/X11) for its egui GUI. When working over SSH, there's no display. A TUI renderer in a zellij floating pane provides the same interactive popup experience in terminal-only environments.

## Constraints

- Zellij is a hard requirement for TUI mode (raw terminal would conflict with Claude Code's terminal usage)
- Slider element becomes a numeric input with min/max validation
- No two-column layout — single column, scrollable
- Markdown rendering is best-effort via `tui-markdown`

## Architecture

### Crate Structure

```
popup-mcp/
  crates/
    popup-common/    # unchanged — data model, conditions, state, results
    popup-gui/       # egui renderer + binary entry point + dispatch logic
    popup-tui/       # NEW — ratatui renderer library
```

`popup-gui` gains a dependency on `popup-tui`. The binary in `popup-gui` owns the auto-detection and dispatch. Both renderers are consumers of `popup-common`.

### Auto-Detection (in binary)

```
1. --tui or --gui flag present?  → use that
2. WAYLAND_DISPLAY or DISPLAY set?  → egui renderer
3. ZELLIJ set?  → zellij floating pane + TUI renderer
4. none of the above  → error: "No display server and no zellij session"
```

### New CLI Flags

- `--tui` — force TUI renderer
- `--gui` — force GUI renderer
- `--result-pipe <path>` — write result JSON to this path (FIFO) instead of stdout

### popup-tui Public API

```rust
pub fn render_popup_tui(definition: PopupDefinition) -> Result<PopupResult>
```

Mirrors `popup_gui::render_popup()`. Takes ownership of a `PopupDefinition`, runs a crossterm event loop, returns the result when the user submits or cancels.

### TUI App Structure

```rust
struct TuiApp {
    definition: PopupDefinition,
    state: PopupState,
    focus_index: usize,
    focusable_ids: Vec<String>,
    condition_cache: HashMap<String, ConditionExpr>,
    result: Option<PopupResult>,
}
```

Standard ratatui event loop: draw frame, read key event, handle input, break when result is set.

### Element Mapping

| Element | TUI Widget | Interaction |
|---------|-----------|-------------|
| Text | `Paragraph` | not focusable |
| Markdown | `tui-markdown` → `Text` | not focusable |
| Input (single-line) | `tui-input` | typing, cursor |
| Input (multi-line) | `ratatui-textarea` | typing, multi-line |
| Check | `[x]`/`[ ]` + label | Space/Enter toggle |
| Select | `List` with highlight | Up/Down, Enter |
| Multi | `List` with checkmarks | Up/Down, Space toggle |
| Group | `Block` with border | renders children indented |
| Slider | `tui-input` numeric | typing, min/max validated |

### Layout

- Single column, vertically scrollable
- Title bar at top
- Elements top-to-bottom with spacing
- Labels above their widgets
- Submit bar at bottom: `[Submit (Ctrl+Enter)]  [Cancel (Esc)]`
- Scroll follows focused element

### Focus Management

- Tab / Shift+Tab cycles focusable elements
- Focusable list rebuilt each frame after evaluating `when` conditions
- Each widget type handles its own keys when focused

### Conditional Visibility

Same condition engine from `popup-common`. Evaluated each frame against current state. Elements with false `when` clauses don't render and drop from the focus ring. `reveals` and `option_children` work identically to egui.

### Option Descriptions

`OptionValue::WithDescription` renders inline: `option_name  (description)` with dimmed description style. No hover tooltips in TUI.

### "Other (please specify)"

Handled by existing `inject_other_options` transform before the renderer sees the definition. Free for TUI.

## Zellij Integration

### Spawn Sequence

```
popup --stdin (parent, receives JSON)
  │
  ├─ create FIFO: /tmp/popup-mcp-{uuid}
  ├─ write popup JSON to temp file: /tmp/popup-mcp-{uuid}.json
  │
  ├─ zellij action new-pane --floating --close-on-exit \
  │    -- popup --tui --file /tmp/popup-mcp-{uuid}.json \
  │                   --result-pipe /tmp/popup-mcp-{uuid}
  │
  ├─ block reading FIFO
  │
  │   child (in floating pane):
  │     reads JSON from --file
  │     renders TUI, user interacts
  │     writes result JSON to FIFO
  │     exits → pane auto-closes
  │
  ├─ read result JSON from FIFO
  ├─ clean up FIFO + temp file
  └─ write result to stdout
```

Uses `--file` because `zellij action new-pane` doesn't support piping stdin to the spawned process.

### Cancellation

If user closes the pane or presses Esc, child exits without writing to FIFO (or writes `Cancelled`). Parent has a timeout on FIFO read. If child exits without result, return `PopupResult::Cancelled`.

### Cleanup

FIFO and temp file deleted via `Drop` guard for panic safety.

## Dependencies (popup-tui)

- `popup-common` (workspace)
- `ratatui`
- `crossterm`
- `tui-markdown`
- `tui-input`
- `ratatui-textarea`
- `anyhow`
- `serde_json`
- `uuid` (v4)

## Testing

### Unit (popup-tui)

- Element rendering via ratatui `TestBackend` + `Buffer` assertions
- Focus cycling, including elements entering/leaving focus ring
- State mutations per widget type
- Numeric input min/max validation
- Conditional visibility (`when`, `reveals`, `option_children`)

### Integration

- Round-trip: JSON → parse → render → simulated input → verify `PopupResult`
- Shared test fixtures between egui and TUI renderers

### Manual

- Zellij floating pane lifecycle (spawn, interact, submit, result return)
- Cancellation (close pane, verify `Cancelled`)
- FIFO cleanup (no leftover `/tmp` files)

## Decisions

- **Zellij required for TUI** — raw terminal would conflict with Claude Code's terminal
- **Slider → numeric input** — loses visual scrubbing, keeps functionality
- **Single column layout** — two-column is painful in terminals
- **Binary keeps dispatch** — no `popup-cli` crate for now, revisit if this works well
- **FIFO for result return** — blocks reader, one-shot, no polling needed
- **Temp file for definition** — zellij can't pipe stdin to spawned process
