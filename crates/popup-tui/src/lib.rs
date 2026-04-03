pub mod app;
mod render;
pub mod widgets;

#[cfg(test)]
mod tests;

use anyhow::Result;
use popup_common::{PopupDefinition, PopupResult};

pub fn render_popup_tui(definition: PopupDefinition) -> Result<PopupResult> {
    app::run(definition)
}
