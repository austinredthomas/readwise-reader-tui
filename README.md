# Readwise Reader TUI

A Terminal User Interface (TUI) for Readwise Reader built in Rust using Ratatui.

![screenshot of the app](https://i.imgur.com/xtMhD4I.png)

## Features

- Columnar list view displaying Title, Author, and Source (Site Name or Hostname).
- Navigation through Inbox, Later, Archive, and Feed locations.
- Full article reading with cleanly formatted text and vertical scrolling.
- High-contrast UI with optimized color schemes for better visibility.
- Lag-free input handling for smooth navigation and scrolling.
- Secure configuration hierarchy supporting CLI flags, environment variables, and TOML files.
- Cursor-based pagination with history tracking.

## Prerequisites

- Rust toolchain (cargo, rustc)

## Installation

Build the binary from source:

```bash
cargo build --release
```

The binary will be located at `target/release/readwise-reader-tui`.

## Configuration

The application prioritizes configuration in the following order:

1. CLI Flags (e.g., `--token`, `--location`)
2. Environment Variables (`READWISE_TOKEN`)
3. Configuration File (`config.toml`)
4. Internal Defaults

### Environment Variables

- `READWISE_TOKEN`: Your Readwise Reader API token. You can obtain this at https://readwise.io/access_token.

### Configuration File

The `config.toml` file should be placed in your OS-specific configuration directory:
- Linux: `~/.config/readwise-reader-tui/config.toml`
- macOS: `~/Library/Application Support/readwise-reader-tui/config.toml`
- Windows: `%AppData%\readwise-reader-tui\config.toml`

Example `config.toml`:

```toml
default_location = "new"
```

## Usage

Run the application using cargo:

```bash
cargo run
```

### Keybindings

- **1, 2, 3, 4**: Switch between Inbox, Later, Archive, and Feed.
- **j, k** or **Down, Up**: Scroll through article list or article text.
- **Enter**: Open the selected article for reading.
- **n, p**: Navigate to the next or previous page of articles.
- **q, Esc**: Go back to the list view or quit the application.
