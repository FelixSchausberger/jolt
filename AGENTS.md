# AGENTS.md - Coding Agent Guidelines for jolt

A terminal-based battery and energy monitor TUI for macOS Apple Silicon.

## Build Commands

```bash
cargo build                    # Development build
cargo build --release          # Release build (optimized, stripped)
./target/debug/jolt            # Run TUI
./target/debug/jolt debug      # Print system/battery info
./target/debug/jolt pipe --samples 2  # JSON output
```

## Lint & Check Commands

```bash
cargo fmt --all --check                                      # Format check (CI enforces)
cargo fmt --all                                              # Format code
cargo clippy --all-targets --all-features -- -D warnings     # Clippy (CI enforces)
cargo check --all-targets --all-features                     # Type check
```

## Test Commands

```bash
cargo test                     # Run all tests
cargo test test_name           # Run single test by name
cargo test module_name::       # Run tests in module
cargo test -- --nocapture      # Run with output
```

## Project Structure

```text
src/
├── main.rs           # CLI entry, clap args
├── app.rs            # App state, actions, event handling
├── config.rs         # User config, themes
├── input.rs          # Key bindings
├── data/
│   ├── mod.rs        # Re-exports
│   ├── battery.rs    # Battery from pmset/ioreg
│   ├── power.rs      # Power from IOReport
│   ├── processes.rs  # Process data from sysinfo
│   └── history.rs    # Time-series for graphs
└── ui/
    ├── mod.rs        # Main render, layout
    ├── battery.rs    # Battery gauge
    ├── processes.rs  # Process table
    ├── graphs.rs     # History charts
    └── help.rs       # Help/About dialogs
```

## Code Style

### Imports (three groups, blank line separated)

```rust
use std::collections::HashMap;

use color_eyre::eyre::Result;
use ratatui::prelude::*;

use crate::config::Theme;
```

### Error Handling

- Use `color_eyre::eyre::Result` as default Result type
- Propagate with `?`, fallback to defaults for non-critical failures
- Prefer `unwrap_or_default()` over `unwrap()`

```rust
pub fn refresh(&mut self) -> Result<()> {
    self.battery.refresh()?;
    Ok(())
}

let config = toml::from_str(&content).unwrap_or_default();
```

### Naming

- **Types/Enums**: `PascalCase` - `BatteryData`, `AppView`
- **Functions**: `snake_case` - `get_visible_processes`
- **Constants**: `SCREAMING_SNAKE_CASE` - `MAX_REFRESH_MS`

### Struct Definitions

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    #[default]
    Auto,
    Dark,
    Light,
}
```

### Module Re-exports

```rust
// data/mod.rs
pub use battery::BatteryData;
pub use history::{HistoryData, HistoryMetric};
```

### UI Rendering Pattern

```rust
pub fn render(frame: &mut Frame, area: Rect, app: &App, theme: &Theme) {
    let block = Block::default()
        .title(" Title ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border));

    let inner = block.inner(area);
    frame.render_widget(block, area);
    // Render content in `inner`
}
```

### Action Pattern

```rust
pub enum Action {
    Quit,
    ToggleHelp,
    None,
}

pub fn handle_action(&mut self, action: Action) -> bool {
    match action {
        Action::Quit => return false,
        Action::ToggleHelp => { self.view = AppView::Help; }
        Action::None => {}
    }
    true
}
```

## File Organization

- **Scratch files**: Store all plans, task lists, and temporary files in `./scratchpad/`
- This directory is gitignored - use it for drafts, notes, and generated content
- Keep the main repo clean of non-source files

## Platform Notes

- **macOS only**: Uses `ioreg`, `pmset`, IOReport APIs
- **Apple Silicon**: Power metrics require M-series chips
- **Rust 1.77.2+**: Some deps pinned for compatibility

## Common Tasks

### Adding a Config Option

1. Add field to `UserConfig` in `config.rs` with Default
2. Add to config editor in `ui/config_editor.rs`
3. Add handler in `App::toggle_config_value`

### Adding a View/Modal

1. Add variant to `AppView` enum
2. Add `Action::Toggle*` variant
3. Add key handler in `input.rs`
4. Add render function, match arm in `ui/mod.rs`

### Adding a Data Source

1. Create struct in `data/` with `new()` and `refresh()`
2. Re-export in `data/mod.rs`
3. Add to `App` struct, init in `App::new()`, call in `App::tick()`
