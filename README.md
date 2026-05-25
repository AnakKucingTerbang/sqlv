# sqlv

A modern, zero-dependency terminal SQLite viewer written in Rust. Browse tables or run SELECT queries — all from your terminal.

```
┌─────────────────────────┬───────────────────────────────────────────────────┐
│ sample.db               │ users  (5 rows)                                   │
│                         │───────────────────────────────────────────────────│
│   orders                │ ID   NAME           EMAIL              CREATED_AT │
│   products              │───────────────────────────────────────────────────│
│ › users                 │  1   Alice Johnson  alice@example.com  2024-01-02 │
│                         │  2   Bob Smith      bob@example.com    2024-01-05 │
│                         │  3   Carol White    carol@example.com  2024-01-08 │
└─────────────────────────┴───────────────────────────────────────────────────┘
 BROWSE  ↑↓ Rows  ←→ Cols  / Query mode  Tab Switch  q Quit         2/5
```

```
┌─────────────────────────┬───────────────────────────────────────────────────┐
│ sample.db               │ SQL Query  ctrl+r Run  ctrl+x Clear  Esc Exit     │
│                         │ SELECT u.name, COUNT(o.id) AS orders              │
│   orders                │ FROM users u                                      │
│   products              │ LEFT JOIN orders o ON o.user_id = u.id            │
│ › users                 │ GROUP BY u.id                                     │
│                         ├───────────────────────────────────────────────────┤
│                         │ NAME           ORDERS                             │
│                         │───────────────────────────────────────────────────│
│                         │ Alice Johnson  2                                  │
│                         │ Bob Smith      1                                  │
└─────────────────────────┴───────────────────────────────────────────────────┘
 QUERY  ctrl+r Run  Tab Complete  ↑↓ Autocomplete  ctrl+x Clear  Esc Exit
```

## Install

### apt (Ubuntu / Debian)

```bash
curl -s https://packagecloud.io/install/repositories/AnakKucingTerbang/sqlv/script.deb.sh | sudo bash
sudo apt-get install sqlv
```

### From a .deb directly

```bash
curl -LO https://github.com/AnakKucingTerbang/sqlv/releases/latest/download/sqlv_1.0.0_amd64.deb
sudo dpkg -i sqlv_1.0.0_amd64.deb
```

### macOS

```bash
# Apple Silicon
curl -LO https://github.com/AnakKucingTerbang/sqlv/releases/latest/download/sqlv-macos-arm64
chmod +x sqlv-macos-arm64 && sudo mv sqlv-macos-arm64 /usr/local/bin/sqlv

# Intel
curl -LO https://github.com/AnakKucingTerbang/sqlv/releases/latest/download/sqlv-macos-x86_64
chmod +x sqlv-macos-x86_64 && sudo mv sqlv-macos-x86_64 /usr/local/bin/sqlv
```

### From source

```bash
# Prerequisites: https://rustup.rs
cargo build --release
./target/release/sqlv my_database.db

# Install system-wide
make install
```

## Usage

```bash
sqlv <database.db>
```

## Modes

sqlv has two modes shown in the status bar:

**BROWSE** — the default. Navigate tables in the sidebar, scroll rows and columns in the right panel.

**QUERY** — press `/` to enter. Write a SELECT query in the editor (top), results appear below. Press `Esc` to return to browse mode.

## Keybindings

### Global

| Key        | Action              |
|------------|---------------------|
| `ctrl+c`   | Quit (from anywhere)|
| `q`        | Quit (browse mode)  |
| `Tab`      | Switch pane         |

### Browse mode

| Key             | Action                       |
|-----------------|------------------------------|
| `↑` / `↓`       | Navigate tables / scroll rows|
| `←` / `→`       | Scroll columns               |
| `Enter` / `→`   | Enter table pane             |
| `PgUp` / `PgDn` | Page through rows            |
| `Home` / `End`  | First / last row             |
| `/`             | Enter query mode             |

### Query mode — editor

| Key        | Action                              |
|------------|-------------------------------------|
| `ctrl+r`   | Run query                           |
| `ctrl+x`   | Clear editor                        |
| `Tab`      | Accept autocomplete / switch to results |
| `↑` / `↓`  | Navigate autocomplete suggestions   |
| `Esc`      | Exit query mode                     |

### Query mode — results

| Key             | Action              |
|-----------------|---------------------|
| `↑` / `↓`       | Scroll rows         |
| `←` / `→`       | Scroll columns      |
| `PgUp` / `PgDn` | Page through rows   |
| `Tab`           | Back to editor      |
| `Esc`           | Exit query mode     |

## Features

- Two modes: browse tables or write SELECT queries
- Multiline SQL editor with syntax highlighting
- Autocomplete for SQL keywords, table names, and column names
- Results panel with the same scrollable table view as browse mode
- Sidebar always visible — select a table to auto-fill a starter query
- Auto-sized columns with horizontal scrolling
- NULL values rendered in a distinct dim style
- Zebra-striped rows and row counter
- 24-bit true-colour theme; graceful fallback on 8-colour terminals
- Single static binary — no runtime dependencies

## Docker

Add sqlv to your Dockerfile for production debugging:

```dockerfile
RUN curl -s https://packagecloud.io/install/repositories/AnakKucingTerbang/sqlv/script.deb.sh | bash \
    && apt-get install -y sqlv
```

Then inspect a running container:

```bash
docker exec -it <container-id> sqlv /app/data/database.db
```

## Dependencies

| Crate       | Purpose                          |
|-------------|----------------------------------|
| `ratatui`   | TUI layout and widgets           |
| `crossterm` | Cross-platform terminal handling |
| `rusqlite`  | SQLite bindings (bundled SQLite) |
| `anyhow`    | Error handling                   |

`rusqlite` uses the `bundled` feature so SQLite is statically linked — the final binary has no system library dependencies.
