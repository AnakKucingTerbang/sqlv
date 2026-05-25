# sqlv

A modern, zero-dependency terminal SQLite viewer written in Rust.

```
╭─────────────────────────╮╭─────────────────────────────────────────────────╮
│ sample.db               ││ users  (5 rows)                                  │
│─────────────────────────││─────────────────────────────────────────────────│
│   orders                ││ ID   NAME           EMAIL              CREATED   │
│   products              ││─────────────────────────────────────────────────│
│ › users                 ││  1   Alice Johnson  alice@example.com  2024-01   │
│                         ││  2   Bob Smith      bob@example.com    2024-01   │
│                         ││  3   Carol White    carol@example.com  2024-01   │
╰─────────────────────────╯╰─────────────────────────────────────────────────╯
  ↑↓ Scroll rows  ←→ Scroll cols  Tab Switch pane  q Quit         3/5
```

## Install

### From a .deb package (Ubuntu / Debian)

```bash
sudo dpkg -i sqlv_1.0.0_amd64.deb
sqlv my_database.db
```

### From source

```bash
# Prerequisites: Rust toolchain (https://rustup.rs)
cargo build --release
./target/release/sqlv my_database.db

# Or install system-wide:
make install        # copies to /usr/local/bin
```

### Build a .deb yourself

```bash
make deb
sudo dpkg -i dist/sqlv_1.0.0_amd64.deb
```

## Usage

```bash
sqlv <database.db>
```

## Keybindings

| Key              | Action                              |
|------------------|-------------------------------------|
| `↑` / `↓`        | Navigate tables / scroll rows       |
| `→` / `Enter`    | Enter table pane                    |
| `←`              | Scroll columns left (table pane)    |
| `→`              | Scroll columns right (table pane)   |
| `Tab`            | Toggle sidebar ↔ table focus        |
| `PgUp` / `PgDn`  | Page through rows                   |
| `Home`           | Jump to first row + first column    |
| `End`            | Jump to last row                    |
| `q`              | Quit                                |

## Features

- Sidebar listing all tables, keyboard navigable
- Auto-sized columns with horizontal scrolling when they overflow
- NULL values rendered in a distinct dim style
- Zebra-striped rows
- Row counter in status bar
- Overflow hint (← +N cols →) in table header
- 24-bit true-colour theme; graceful fallback on 8-colour terminals
- Single static binary — no runtime dependencies

## Dependencies (Rust crates)

| Crate       | Purpose                          |
|-------------|----------------------------------|
| `ratatui`   | TUI layout and widgets           |
| `crossterm` | Cross-platform terminal handling |
| `rusqlite`  | SQLite bindings (bundled SQLite) |
| `anyhow`    | Error handling                   |

`rusqlite` uses the `bundled` feature, so SQLite is statically linked — the
final binary has **no system library dependencies**.
