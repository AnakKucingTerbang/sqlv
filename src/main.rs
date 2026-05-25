use std::{collections::HashMap, env, path::PathBuf, time::Duration};

use anyhow::{Context, Result};
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, List, ListItem, ListState, Paragraph, Row, Table, TableState, Wrap},
    Frame, Terminal,
};
use rusqlite::Connection;

// ── Database ──────────────────────────────────────────────────────────────────

fn list_tables(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name",
    )?;
    let names: Vec<String> = stmt
        .query_map([], |r| r.get(0))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(names)
}

fn list_columns(conn: &Connection, table: &str) -> Vec<String> {
    let sql = format!("PRAGMA table_info(\"{table}\")");
    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    stmt.query_map([], |r| r.get::<_, String>(1))
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default()
}

/// Build a map of table -> columns for autocomplete
fn build_schema(conn: &Connection, tables: &[String]) -> HashMap<String, Vec<String>> {
    tables.iter().map(|t| (t.clone(), list_columns(conn, t))).collect()
}

struct TableData {
    columns: Vec<String>,
    rows: Vec<Vec<String>>,
    total_rows: usize,
}

fn load_table(conn: &Connection, name: &str) -> Result<TableData> {
    let total_rows: usize = conn
        .query_row(&format!("SELECT COUNT(*) FROM \"{name}\""), [], |r| {
            r.get::<_, i64>(0)
        })
        .unwrap_or(0) as usize;

    let mut stmt = conn.prepare(&format!("SELECT * FROM \"{name}\" LIMIT 500"))?;
    let columns: Vec<String> = stmt.column_names().into_iter().map(|s| s.to_string()).collect();
    let col_count = columns.len();
    let rows: Vec<Vec<String>> = stmt
        .query_map([], |row| {
            Ok((0..col_count)
                .map(|i| {
                    row.get_ref(i)
                        .map(|v| match v {
                            rusqlite::types::ValueRef::Null => "NULL".to_string(),
                            rusqlite::types::ValueRef::Integer(n) => n.to_string(),
                            rusqlite::types::ValueRef::Real(f) => format!("{f}"),
                            rusqlite::types::ValueRef::Text(t) => String::from_utf8_lossy(t).to_string(),
                            rusqlite::types::ValueRef::Blob(_) => "<blob>".to_string(),
                        })
                        .unwrap_or_else(|_| "?".to_string())
                })
                .collect())
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(TableData { columns, rows, total_rows })
}

fn run_query(conn: &Connection, sql: &str) -> Result<TableData, String> {
    let trimmed = sql.trim();
    let upper = trimmed.to_uppercase();
    if !upper.starts_with("SELECT") && !upper.starts_with("WITH") {
        return Err("Only SELECT queries are allowed.".to_string());
    }
    let mut stmt = conn.prepare(trimmed).map_err(|e| e.to_string())?;
    let columns: Vec<String> = stmt.column_names().into_iter().map(|s| s.to_string()).collect();
    let col_count = columns.len();
    let rows: Vec<Vec<String>> = stmt
        .query_map([], |row| {
            Ok((0..col_count)
                .map(|i| {
                    row.get_ref(i)
                        .map(|v| match v {
                            rusqlite::types::ValueRef::Null => "NULL".to_string(),
                            rusqlite::types::ValueRef::Integer(n) => n.to_string(),
                            rusqlite::types::ValueRef::Real(f) => format!("{f}"),
                            rusqlite::types::ValueRef::Text(t) => String::from_utf8_lossy(t).to_string(),
                            rusqlite::types::ValueRef::Blob(_) => "<blob>".to_string(),
                        })
                        .unwrap_or_else(|_| "?".to_string())
                })
                .collect())
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    let total_rows = rows.len();
    Ok(TableData { columns, rows, total_rows })
}

// ── Autocomplete ──────────────────────────────────────────────────────────────

const SQL_KEYWORDS: &[&str] = &[
    "SELECT", "FROM", "WHERE", "AND", "OR", "NOT", "IN", "LIKE", "IS", "NULL",
    "JOIN", "INNER JOIN", "LEFT JOIN", "LEFT OUTER JOIN", "RIGHT JOIN", "FULL JOIN",
    "CROSS JOIN", "ON", "AS", "DISTINCT", "ALL", "UNION", "UNION ALL", "INTERSECT",
    "EXCEPT", "ORDER BY", "GROUP BY", "HAVING", "LIMIT", "OFFSET", "WITH",
    "CASE", "WHEN", "THEN", "ELSE", "END", "BETWEEN", "EXISTS", "ASC", "DESC",
    "COUNT", "SUM", "AVG", "MIN", "MAX", "COALESCE", "NULLIF", "LENGTH",
    "SUBSTR", "TRIM", "UPPER", "LOWER", "REPLACE", "ROUND", "ABS", "DATE",
    "DATETIME", "STRFTIME", "TYPEOF", "CAST", "IIF",
];

fn get_completions(
    word: &str,
    schema: &HashMap<String, Vec<String>>,
    tables: &[String],
) -> Vec<String> {
    if word.is_empty() {
        return vec![];
    }
    let upper_word = word.to_uppercase();
    let mut results: Vec<String> = vec![];

    // SQL keywords
    for &kw in SQL_KEYWORDS {
        if kw.starts_with(&upper_word) {
            results.push(kw.to_string());
        }
    }

    // Table names
    for t in tables {
        if t.to_uppercase().starts_with(&upper_word) {
            results.push(t.clone());
        }
    }

    // Column names (all tables, deduplicated)
    let mut seen = std::collections::HashSet::new();
    for cols in schema.values() {
        for col in cols {
            if col.to_uppercase().starts_with(&upper_word) && seen.insert(col.clone()) {
                results.push(col.clone());
            }
        }
    }

    // table.column — if word contains a dot
    if let Some(dot_pos) = word.find('.') {
        let tbl = &word[..dot_pos];
        let col_prefix = &word[dot_pos + 1..].to_uppercase();
        results.clear();
        if let Some(cols) = schema.get(tbl) {
            for col in cols {
                if col.to_uppercase().starts_with(col_prefix.as_str()) {
                    results.push(format!("{tbl}.{col}"));
                }
            }
        }
    }

    results.dedup();
    results.truncate(8);
    results
}

/// Extract the word currently being typed at cursor position
fn current_word(lines: &[String], row: usize, col: usize) -> String {
    let line = match lines.get(row) {
        Some(l) => l,
        None => return String::new(),
    };
    let before = &line[..col.min(line.len())];
    before
        .rsplit(|c: char| c.is_whitespace() || c == ',' || c == '(' || c == ')')
        .next()
        .unwrap_or("")
        .to_string()
}

// ── Multiline editor ──────────────────────────────────────────────────────────

#[derive(Default)]
struct Editor {
    lines: Vec<String>,
    cursor_row: usize,
    cursor_col: usize,
}

impl Editor {
    fn new() -> Self {
        Self { lines: vec![String::new()], cursor_row: 0, cursor_col: 0 }
    }

    fn set_text(&mut self, text: &str) {
        self.lines = text.lines().map(|l| l.to_string()).collect();
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.cursor_row = self.lines.len() - 1;
        self.cursor_col = self.lines[self.cursor_row].len();
    }

    fn text(&self) -> String {
        self.lines.join("\n")
    }

    fn insert_char(&mut self, c: char) {
        let line = &mut self.lines[self.cursor_row];
        let col = self.cursor_col.min(line.len());
        line.insert(col, c);
        self.cursor_col += 1;
    }

    fn insert_str(&mut self, s: &str) {
        for c in s.chars() {
            self.insert_char(c);
        }
    }

    fn backspace(&mut self) {
        if self.cursor_col > 0 {
            let line = &mut self.lines[self.cursor_row];
            let col = self.cursor_col - 1;
            // find char boundary
            let byte_pos = line.char_indices().nth(col).map(|(i, _)| i).unwrap_or(line.len());
            line.remove(byte_pos);
            self.cursor_col -= 1;
        } else if self.cursor_row > 0 {
            let line = self.lines.remove(self.cursor_row);
            self.cursor_row -= 1;
            let prev_len = self.lines[self.cursor_row].len();
            self.lines[self.cursor_row].push_str(&line);
            self.cursor_col = prev_len;
        }
    }

    fn newline(&mut self) {
        let line = &mut self.lines[self.cursor_row];
        let col = self.cursor_col.min(line.len());
        let byte_pos = line
            .char_indices()
            .nth(col)
            .map(|(i, _)| i)
            .unwrap_or(line.len());
        let rest = line[byte_pos..].to_string();
        line.truncate(byte_pos);
        self.cursor_row += 1;
        self.lines.insert(self.cursor_row, rest);
        self.cursor_col = 0;
    }

    fn move_left(&mut self) {
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        } else if self.cursor_row > 0 {
            self.cursor_row -= 1;
            self.cursor_col = self.lines[self.cursor_row].chars().count();
        }
    }

    fn move_right(&mut self) {
        let len = self.lines[self.cursor_row].chars().count();
        if self.cursor_col < len {
            self.cursor_col += 1;
        } else if self.cursor_row + 1 < self.lines.len() {
            self.cursor_row += 1;
            self.cursor_col = 0;
        }
    }

    fn move_up(&mut self) {
        if self.cursor_row > 0 {
            self.cursor_row -= 1;
            let len = self.lines[self.cursor_row].chars().count();
            self.cursor_col = self.cursor_col.min(len);
        }
    }

    fn move_down(&mut self) {
        if self.cursor_row + 1 < self.lines.len() {
            self.cursor_row += 1;
            let len = self.lines[self.cursor_row].chars().count();
            self.cursor_col = self.cursor_col.min(len);
        }
    }

    fn move_home(&mut self) { self.cursor_col = 0; }

    fn move_end(&mut self) {
        self.cursor_col = self.lines[self.cursor_row].chars().count();
    }

    fn clear(&mut self) {
        self.lines = vec![String::new()];
        self.cursor_row = 0;
        self.cursor_col = 0;
    }

    /// Replace the current partial word with the completion
    fn apply_completion(&mut self, completion: &str) {
        let word = current_word(&self.lines, self.cursor_row, self.cursor_col);
        // Remove the partial word
        let remove = word.len();
        for _ in 0..remove {
            self.backspace();
        }
        self.insert_str(completion);
        self.insert_char(' ');
    }
}

// ── App state ─────────────────────────────────────────────────────────────────

#[derive(PartialEq, Clone)]
enum Focus {
    Sidebar,
    Table,
    QueryEditor,
    QueryResults,
}

struct App {
    db_name:       String,
    tables:        Vec<String>,
    schema:        HashMap<String, Vec<String>>,
    sidebar_state: ListState,
    focus:         Focus,

    // browse mode
    table_name:  String,
    columns:     Vec<String>,
    rows:        Vec<Vec<String>>,
    total_rows:  usize,
    table_state: TableState,
    col_offset:  usize,

    // query mode
    query_mode:      bool,
    editor:          Editor,
    query_columns:   Vec<String>,
    query_rows:      Vec<Vec<String>>,
    query_error:     Option<String>,
    query_state:     TableState,
    query_col_offset: usize,

    // autocomplete
    completions:    Vec<String>,
    comp_selected:  usize,
    show_comp:      bool,
}

impl App {
    fn new(conn: &Connection, db_path: &str) -> Result<Self> {
        let tables = list_tables(conn)?;
        let schema = build_schema(conn, &tables);
        let db_name = PathBuf::from(db_path)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| db_path.to_string());

        let mut app = App {
            db_name,
            tables,
            schema,
            sidebar_state: ListState::default(),
            focus: Focus::Sidebar,
            table_name: String::new(),
            columns: vec![],
            rows: vec![],
            total_rows: 0,
            table_state: TableState::default(),
            col_offset: 0,
            query_mode: false,
            editor: Editor::new(),
            query_columns: vec![],
            query_rows: vec![],
            query_error: None,
            query_state: TableState::default(),
            query_col_offset: 0,
            completions: vec![],
            comp_selected: 0,
            show_comp: false,
        };

        if !app.tables.is_empty() {
            app.sidebar_state.select(Some(0));
            let data = load_table(conn, &app.tables[0])?;
            app.apply_table_data(&app.tables[0].clone(), data);
        }
        Ok(app)
    }

    fn apply_table_data(&mut self, name: &str, data: TableData) {
        self.table_name  = name.to_string();
        self.columns     = data.columns;
        self.rows        = data.rows;
        self.total_rows  = data.total_rows;
        self.table_state = TableState::default();
        if !self.rows.is_empty() { self.table_state.select(Some(0)); }
        self.col_offset  = 0;
    }

    fn enter_query_mode(&mut self, table_name: &str) {
        self.query_mode = true;
        self.focus = Focus::QueryEditor;
        let sql = format!("SELECT *\nFROM {table_name}\nLIMIT 100");
        self.editor.set_text(&sql);
        self.query_columns.clear();
        self.query_rows.clear();
        self.query_error = None;
        self.show_comp = false;
    }

    fn exit_query_mode(&mut self) {
        self.query_mode = false;
        self.focus = Focus::Table;
        self.show_comp = false;
    }

    fn update_completions(&mut self) {
        let word = current_word(&self.editor.lines, self.editor.cursor_row, self.editor.cursor_col);
        if word.len() >= 1 {
            self.completions = get_completions(&word, &self.schema, &self.tables);
            self.show_comp = !self.completions.is_empty();
            self.comp_selected = 0;
        } else {
            self.completions.clear();
            self.show_comp = false;
        }
    }

    fn accept_completion(&mut self) {
        if self.show_comp && !self.completions.is_empty() {
            let c = self.completions[self.comp_selected].clone();
            self.editor.apply_completion(&c);
            self.show_comp = false;
            self.completions.clear();
        }
    }

    fn sidebar_next(&mut self) {
        let i = match self.sidebar_state.selected() {
            Some(i) => (i + 1).min(self.tables.len().saturating_sub(1)),
            None => 0,
        };
        self.sidebar_state.select(Some(i));
    }

    fn sidebar_prev(&mut self) {
        let i = match self.sidebar_state.selected() {
            Some(0) | None => 0,
            Some(i) => i - 1,
        };
        self.sidebar_state.select(Some(i));
    }

    fn row_next(&mut self) {
        let (rows, state) = if self.query_mode {
            (&self.query_rows, &mut self.query_state)
        } else {
            (&self.rows, &mut self.table_state)
        };
        if rows.is_empty() { return; }
        let i = state.selected().map(|i| (i + 1).min(rows.len() - 1)).unwrap_or(0);
        state.select(Some(i));
    }

    fn row_prev(&mut self) {
        let (rows, state) = if self.query_mode {
            (&self.query_rows, &mut self.query_state)
        } else {
            (&self.rows, &mut self.table_state)
        };
        if rows.is_empty() { return; }
        let i = state.selected().map(|i| if i == 0 { 0 } else { i - 1 }).unwrap_or(0);
        state.select(Some(i));
    }

    fn row_page(&mut self, delta: isize) {
        let (rows, state) = if self.query_mode {
            (&self.query_rows, &mut self.query_state)
        } else {
            (&self.rows, &mut self.table_state)
        };
        if rows.is_empty() { return; }
        let cur = state.selected().unwrap_or(0) as isize;
        let next = (cur + delta).clamp(0, rows.len() as isize - 1) as usize;
        state.select(Some(next));
    }

    fn col_right(&mut self) {
        let (cols, offset) = if self.query_mode {
            (&self.query_columns, &mut self.query_col_offset)
        } else {
            (&self.columns, &mut self.col_offset)
        };
        if *offset + 1 < cols.len() { *offset += 1; }
    }

    fn col_left(&mut self) {
        let offset = if self.query_mode { &mut self.query_col_offset } else { &mut self.col_offset };
        *offset = offset.saturating_sub(1);
    }
}

// ── Colours ───────────────────────────────────────────────────────────────────

const C_BG:         Color = Color::Reset;
const C_SIDEBAR_BG: Color = Color::Rgb(18, 18, 24);
const C_HEADER_BG:  Color = Color::Rgb(30, 30, 42);
const C_SEL_BG:     Color = Color::Rgb(60, 80, 160);
const C_ACCENT:     Color = Color::Rgb(110, 160, 255);
const C_TEXT:       Color = Color::Rgb(210, 210, 220);
const C_NULL:       Color = Color::Rgb(90, 90, 110);
const C_STATUS_BG:  Color = Color::Rgb(60, 80, 160);
const C_STATUS_FG:  Color = Color::Rgb(230, 230, 255);
const C_BORDER:     Color = Color::Rgb(50, 50, 70);
const C_TITLE:      Color = Color::Rgb(255, 255, 255);
const C_EDITOR_BG:  Color = Color::Rgb(14, 14, 20);
const C_CURSOR:     Color = Color::Rgb(110, 160, 255);
const C_ERROR:      Color = Color::Rgb(255, 100, 100);
const C_COMP_BG:    Color = Color::Rgb(30, 35, 55);
const C_COMP_SEL:   Color = Color::Rgb(60, 80, 160);
const C_KW:         Color = Color::Rgb(180, 140, 255);
const C_QUERY_MODE: Color = Color::Rgb(255, 180, 80);

const SIDEBAR_W:    u16 = 26;
const EDITOR_H:     u16 = 8; // fixed editor height in query mode

fn truncate(s: &str, max_chars: usize) -> String {
    let mut chars = s.chars();
    let collected: String = chars.by_ref().take(max_chars.saturating_sub(1)).collect();
    if chars.next().is_some() { format!("{collected}…") } else { s.to_string() }
}

// ── Drawing ───────────────────────────────────────────────────────────────────

fn ui(f: &mut Frame, app: &mut App) {
    let area = f.size();

    let horiz = Layout::horizontal([Constraint::Length(SIDEBAR_W), Constraint::Min(0)]).split(area);
    let main_vert = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(horiz[1]);

    draw_sidebar(f, app, horiz[0]);

    if app.query_mode {
        let right = Layout::vertical([
            Constraint::Length(EDITOR_H),
            Constraint::Min(0),
        ]).split(main_vert[0]);
        draw_editor(f, app, right[0]);
        draw_query_results(f, app, right[1]);
    } else {
        draw_table(f, app, main_vert[0]);
    }

    draw_status(f, app, main_vert[1]);

    // Autocomplete popup (drawn last so it's on top)
    if app.show_comp && app.focus == Focus::QueryEditor {
        draw_autocomplete(f, app, horiz[1]);
    }
}

fn draw_sidebar(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Sidebar;
    let border_style = if focused {
        Style::default().fg(C_ACCENT)
    } else {
        Style::default().fg(C_BORDER)
    };

    let block = Block::default()
        .title(Span::styled(
            format!(" {} ", app.db_name),
            Style::default().fg(C_ACCENT).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(border_style)
        .style(Style::default().bg(C_SIDEBAR_BG));

    let items: Vec<ListItem> = app.tables.iter().enumerate().map(|(i, name)| {
        let is_sel = app.sidebar_state.selected() == Some(i);
        let prefix = if is_sel { " › " } else { "   " };
        let style = if is_sel {
            Style::default().fg(C_TITLE).bg(C_SEL_BG).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(C_TEXT).bg(C_SIDEBAR_BG)
        };
        let max_w = (area.width as usize).saturating_sub(5);
        let display = format!("{prefix}{}", truncate(name, max_w));
        ListItem::new(Line::from(Span::styled(display, style)))
    }).collect();

    let list = List::new(items).block(block).highlight_style(Style::default());
    f.render_stateful_widget(list, area, &mut app.sidebar_state);
}

fn draw_editor(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::QueryEditor;
    let border_style = Style::default().fg(if focused { C_QUERY_MODE } else { C_BORDER });

    let block = Block::default()
        .title(Span::styled(
            " SQL Query  ctrl+r Run   ctrl+x Clear   Esc Exit ",
            Style::default().fg(C_QUERY_MODE).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(border_style)
        .style(Style::default().bg(C_EDITOR_BG));

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Render lines with cursor
    let visible_rows = inner.height as usize;
    // scroll editor so cursor is visible
    let start_row = if app.editor.cursor_row >= visible_rows {
        app.editor.cursor_row - visible_rows + 1
    } else {
        0
    };

    let lines: Vec<Line> = app.editor.lines
        .iter()
        .enumerate()
        .skip(start_row)
        .take(visible_rows)
        .map(|(ri, line)| {
            if ri == app.editor.cursor_row && focused {
                // Render with cursor block
                let col = app.editor.cursor_col.min(line.chars().count());
                let before: String = line.chars().take(col).collect();
                let cursor_ch: String = line.chars().nth(col).map(|c| c.to_string()).unwrap_or(" ".to_string());
                let after: String = line.chars().skip(col + 1).collect();
                Line::from(vec![
                    Span::styled(before, Style::default().fg(C_TEXT)),
                    Span::styled(cursor_ch, Style::default().fg(C_EDITOR_BG).bg(C_CURSOR)),
                    Span::styled(after, Style::default().fg(C_TEXT)),
                ])
            } else {
                // Syntax highlight keywords
                highlight_sql_line(line)
            }
        })
        .collect();

    let para = Paragraph::new(lines).style(Style::default().bg(C_EDITOR_BG));
    f.render_widget(para, inner);
}

fn highlight_sql_line(line: &str) -> Line<'static> {
    let mut spans = vec![];
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    let mut plain_buf = String::new();

    while i < chars.len() {
        if chars[i].is_alphabetic() || chars[i] == '_' {
            // collect word
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            if SQL_KEYWORDS.contains(&word.to_uppercase().as_str()) {
                if !plain_buf.is_empty() {
                    spans.push(Span::styled(plain_buf.clone(), Style::default().fg(C_TEXT)));
                    plain_buf.clear();
                }
                spans.push(Span::styled(word, Style::default().fg(C_KW).add_modifier(Modifier::BOLD)));
            } else {
                plain_buf.push_str(&word);
            }
        } else {
            plain_buf.push(chars[i]);
            i += 1;
        }
    }
    if !plain_buf.is_empty() {
        spans.push(Span::styled(plain_buf, Style::default().fg(C_TEXT)));
    }

    Line::from(spans)
}

fn draw_autocomplete(f: &mut Frame, app: &App, right_area: Rect) {
    if app.completions.is_empty() { return; }

    let items: Vec<ListItem> = app.completions.iter().enumerate().map(|(i, c)| {
        let style = if i == app.comp_selected {
            Style::default().fg(C_TITLE).bg(C_COMP_SEL).add_modifier(Modifier::BOLD)
        } else {
            // colour keywords differently
            if SQL_KEYWORDS.contains(&c.as_str()) {
                Style::default().fg(C_KW).bg(C_COMP_BG)
            } else {
                Style::default().fg(C_TEXT).bg(C_COMP_BG)
            }
        };
        ListItem::new(Line::from(Span::styled(format!(" {c} "), style)))
    }).collect();

    let popup_h = (app.completions.len() as u16 + 2).min(12);
    let popup_w = app.completions.iter().map(|c| c.len()).max().unwrap_or(10) as u16 + 4;
    let popup_w = popup_w.max(20).min(right_area.width / 2);

    // Position popup below the editor
    let x = right_area.x + 2;
    let y = right_area.y + EDITOR_H;
    let popup_area = Rect {
        x: x.min(right_area.x + right_area.width.saturating_sub(popup_w)),
        y: y.min(right_area.y + right_area.height.saturating_sub(popup_h)),
        width: popup_w,
        height: popup_h,
    };

    f.render_widget(Clear, popup_area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(C_ACCENT))
        .style(Style::default().bg(C_COMP_BG));
    let list = List::new(items).block(block);
    f.render_widget(list, popup_area);
}

fn draw_table_inner(
    f: &mut Frame,
    area: Rect,
    columns: &[String],
    rows: &[Vec<String>],
    table_state: &mut TableState,
    col_offset: usize,
    title: String,
    border_style: Style,
    error: Option<&str>,
) {
    let block = Block::default()
        .title(Span::styled(title, Style::default().fg(C_ACCENT).add_modifier(Modifier::BOLD)))
        .borders(Borders::ALL)
        .border_style(border_style)
        .style(Style::default().bg(C_BG));

    if let Some(err) = error {
        let para = Paragraph::new(err.to_string())
            .block(block)
            .style(Style::default().fg(C_ERROR))
            .wrap(Wrap { trim: false });
        f.render_widget(para, area);
        return;
    }

    if columns.is_empty() {
        let para = Paragraph::new("Run a query to see results.")
            .block(block)
            .style(Style::default().fg(C_BORDER));
        f.render_widget(para, area);
        return;
    }

    let inner_w = area.width.saturating_sub(2) as usize;
    let col_count = columns.len();
    let col_widths: Vec<usize> = (0..col_count).map(|ci| {
        let header_w = columns[ci].len();
        let data_w = rows.iter().map(|r| r[ci].len()).max().unwrap_or(0);
        header_w.max(data_w).min(40)
    }).collect();

    let mut visible: Vec<usize> = vec![];
    let mut used = 0usize;
    for ci in col_offset..col_count {
        let w = col_widths[ci] + 2;
        if used + w > inner_w && !visible.is_empty() { break; }
        visible.push(ci);
        used += w;
    }

    let header_cells: Vec<Cell> = visible.iter().map(|&ci| {
        Cell::from(columns[ci].to_uppercase()).style(
            Style::default().fg(C_ACCENT).bg(C_HEADER_BG).add_modifier(Modifier::BOLD)
        )
    }).collect();
    let header = Row::new(header_cells).style(Style::default().bg(C_HEADER_BG)).height(1);

    let selected_idx = table_state.selected();
    let data_rows: Vec<Row> = rows.iter().enumerate().map(|(ri, row)| {
        let is_selected = selected_idx == Some(ri);
        let is_even = ri % 2 == 0;
        let row_bg = if is_selected { C_SEL_BG } else if is_even { C_BG } else { Color::Rgb(22, 22, 30) };

        let cells: Vec<Cell> = visible.iter().map(|&ci| {
            let val = &row[ci];
            let style = if val == "NULL" {
                Style::default().fg(C_NULL).bg(row_bg).add_modifier(Modifier::ITALIC)
            } else if is_selected {
                Style::default().fg(C_TITLE).bg(row_bg)
            } else {
                Style::default().fg(C_TEXT).bg(row_bg)
            };
            Cell::from(truncate(val, col_widths[ci])).style(style)
        }).collect();

        Row::new(cells).style(Style::default().bg(row_bg)).height(1)
    }).collect();

    let constraints: Vec<Constraint> = visible.iter()
        .map(|&ci| Constraint::Length((col_widths[ci] + 2) as u16))
        .collect();

    let overflow = col_count.saturating_sub(col_offset + visible.len());
    let overflow_hint = if col_offset > 0 || overflow > 0 {
        format!(
            "{}{}",
            if col_offset > 0 { "← " } else { "" },
            if overflow > 0 { format!("+{overflow} cols →") } else { String::new() }
        )
    } else { String::new() };

    // overflow hint is shown inline in the title passed by the caller

    let table_widget = Table::new(data_rows, constraints)
        .header(header)
        .block(block)
        .highlight_style(Style::default());

    f.render_stateful_widget(table_widget, area, table_state);
}

fn draw_table(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Table;
    let border_style = Style::default().fg(if focused { C_ACCENT } else { C_BORDER });
    let title = if app.table_name.is_empty() {
        " Select a table  /  Enter query mode ".to_string()
    } else {
        format!(" {}  ({} rows)   / Query mode ", app.table_name, app.total_rows)
    };

    let cols = app.columns.clone();
    let rows = app.rows.clone();
    let col_offset = app.col_offset;
    let mut state = app.table_state.clone();
    draw_table_inner(f, area, &cols, &rows, &mut state, col_offset, title, border_style, None);
    app.table_state = state;
}

fn draw_query_results(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::QueryResults;
    let border_style = Style::default().fg(if focused { C_ACCENT } else { C_BORDER });
    let row_count = app.query_rows.len();
    let title = if app.query_error.is_some() {
        " Error ".to_string()
    } else if app.query_columns.is_empty() {
        " Results ".to_string()
    } else {
        format!(" Results  ({row_count} rows) ")
    };

    let cols = app.query_columns.clone();
    let rows = app.query_rows.clone();
    let col_offset = app.query_col_offset;
    let error = app.query_error.clone();
    let mut state = app.query_state.clone();
    draw_table_inner(f, area, &cols, &rows, &mut state, col_offset, title, border_style, error.as_deref());
    app.query_state = state;
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let hints = match &app.focus {
        Focus::Sidebar      => " ↑↓ Navigate   Enter/→ Select   / Query mode   Tab Switch   q Quit ",
        Focus::Table        => " ↑↓ Rows   ←→ Cols   / Query mode   Tab Switch   q Quit ",
        Focus::QueryEditor  => " ctrl+r Run   Tab Complete   ↑↓ Autocomplete   ctrl+x Clear   Esc Exit query ",
        Focus::QueryResults => " ↑↓ Rows   ←→ Cols   Tab→Editor   Esc Exit query ",
    };

    let row_info = {
        let (rows, state) = if app.query_mode {
            (&app.query_rows, &app.query_state)
        } else {
            (&app.rows, &app.table_state)
        };
        if !rows.is_empty() {
            format!(" {}/{} ", state.selected().unwrap_or(0) + 1, rows.len())
        } else { String::new() }
    };

    let mode_indicator = if app.query_mode {
        Span::styled(" QUERY ", Style::default().fg(C_EDITOR_BG).bg(C_QUERY_MODE).add_modifier(Modifier::BOLD))
    } else {
        Span::styled(" BROWSE ", Style::default().fg(C_STATUS_FG).bg(C_STATUS_BG).add_modifier(Modifier::BOLD))
    };

    let pad = area.width
        .saturating_sub(hints.len() as u16 + row_info.len() as u16 + 9);

    let bar = Line::from(vec![
        mode_indicator,
        Span::styled(hints, Style::default().fg(C_STATUS_FG).bg(C_STATUS_BG)),
        Span::styled(" ".repeat(pad as usize), Style::default().bg(C_STATUS_BG)),
        Span::styled(row_info, Style::default().fg(C_STATUS_FG).bg(C_STATUS_BG).add_modifier(Modifier::BOLD)),
    ]);

    f.render_widget(Paragraph::new(bar), area);
}

// ── Main loop ─────────────────────────────────────────────────────────────────

fn run_app(conn: Connection, db_path: &str) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut term = Terminal::new(backend)?;

    let mut app = App::new(&conn, db_path)?;
    let page = 20isize;

    loop {
        term.draw(|f| ui(f, &mut app))?;

        if !event::poll(Duration::from_millis(50))? { continue; }
        let Event::Key(key) = event::read()? else { continue };
        if key.kind != KeyEventKind::Press { continue; }

        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        // ── Global ────────────────────────────────────────────────────────
        if ctrl && key.code == KeyCode::Char('c') { break; }
        match key.code {
            KeyCode::Char('q') if !app.query_mode => break,
            _ => {}
        }

        // ── Query editor focus ────────────────────────────────────────────
        if app.focus == Focus::QueryEditor {
            // Autocomplete navigation
            if app.show_comp {
                match key.code {
                    KeyCode::Up => {
                        if app.comp_selected > 0 { app.comp_selected -= 1; }
                        continue;
                    }
                    KeyCode::Down => {
                        if app.comp_selected + 1 < app.completions.len() { app.comp_selected += 1; }
                        continue;
                    }
                    KeyCode::Tab | KeyCode::Enter => {
                        app.accept_completion();
                        app.update_completions();
                        continue;
                    }
                    KeyCode::Esc => {
                        app.show_comp = false;
                        continue;
                    }
                    _ => {}
                }
            }

            match key.code {
                KeyCode::Esc => app.exit_query_mode(),

                KeyCode::Char('r') if ctrl => {
                    app.show_comp = false;
                    let sql = app.editor.text();
                    match run_query(&conn, &sql) {
                        Ok(data) => {
                            app.query_columns = data.columns;
                            app.query_rows    = data.rows;
                            app.query_error   = None;
                            app.query_state   = TableState::default();
                            if !app.query_rows.is_empty() { app.query_state.select(Some(0)); }
                            app.query_col_offset = 0;
                            app.focus = Focus::QueryResults;
                        }
                        Err(e) => {
                            app.query_error   = Some(e);
                            app.query_columns = vec![];
                            app.query_rows    = vec![];
                            app.focus = Focus::QueryResults;
                        }
                    }
                }

                KeyCode::Char('x') if ctrl => {
                    app.editor.clear();
                    app.show_comp = false;
                }

                KeyCode::Tab => {
                    if app.show_comp {
                        app.accept_completion();
                    } else {
                        app.focus = Focus::QueryResults;
                        app.show_comp = false;
                    }
                }

                KeyCode::Enter  => { app.editor.newline(); app.update_completions(); }
                KeyCode::Backspace => { app.editor.backspace(); app.update_completions(); }
                KeyCode::Left   => { app.editor.move_left();  app.update_completions(); }
                KeyCode::Right  => { app.editor.move_right(); app.update_completions(); }
                KeyCode::Up     => { app.editor.move_up();    app.update_completions(); }
                KeyCode::Down   => { app.editor.move_down();  app.update_completions(); }
                KeyCode::Home   => { app.editor.move_home();  app.show_comp = false; }
                KeyCode::End    => { app.editor.move_end();   app.show_comp = false; }

                KeyCode::Char(c) => {
                    app.editor.insert_char(c);
                    app.update_completions();
                }

                _ => {}
            }
            continue;
        }

        // ── Query results focus ───────────────────────────────────────────
        if app.focus == Focus::QueryResults {
            match key.code {
                KeyCode::Esc        => app.exit_query_mode(),
                KeyCode::Tab        => app.focus = Focus::QueryEditor,
                KeyCode::Up         => app.row_prev(),
                KeyCode::Down       => app.row_next(),
                KeyCode::Left       => app.col_left(),
                KeyCode::Right      => app.col_right(),
                KeyCode::PageUp     => app.row_page(-page),
                KeyCode::PageDown   => app.row_page(page),
                _ => {}
            }
            continue;
        }

        // ── Sidebar ───────────────────────────────────────────────────────
        if app.focus == Focus::Sidebar {
            match key.code {
                KeyCode::Tab | KeyCode::BackTab => {
                    app.focus = if app.query_mode { Focus::QueryEditor } else { Focus::Table };
                }
                KeyCode::Up => {
                    app.sidebar_prev();
                    if let Some(i) = app.sidebar_state.selected() {
                        let name = app.tables[i].clone();
                        if app.query_mode {
                            let sql = format!("SELECT *\nFROM {name}\nLIMIT 100");
                            app.editor.set_text(&sql);
                        } else {
                            let data = load_table(&conn, &name)?;
                            app.apply_table_data(&name, data);
                        }
                    }
                }
                KeyCode::Down => {
                    app.sidebar_next();
                    if let Some(i) = app.sidebar_state.selected() {
                        let name = app.tables[i].clone();
                        if app.query_mode {
                            let sql = format!("SELECT *\nFROM {name}\nLIMIT 100");
                            app.editor.set_text(&sql);
                        } else {
                            let data = load_table(&conn, &name)?;
                            app.apply_table_data(&name, data);
                        }
                    }
                }
                KeyCode::Enter | KeyCode::Right => {
                    app.focus = if app.query_mode { Focus::QueryEditor } else { Focus::Table };
                }
                KeyCode::Char('/') => {
                    let name = app.sidebar_state.selected()
                        .and_then(|i| app.tables.get(i))
                        .cloned()
                        .unwrap_or_default();
                    app.enter_query_mode(&name);
                }
                _ => {}
            }
            continue;
        }

        // ── Browse table ──────────────────────────────────────────────────
        if app.focus == Focus::Table {
            match key.code {
                KeyCode::Tab | KeyCode::BackTab => app.focus = Focus::Sidebar,
                KeyCode::Char('/') => {
                    let name = app.table_name.clone();
                    app.enter_query_mode(&name);
                }
                KeyCode::Up       => app.row_prev(),
                KeyCode::Down     => app.row_next(),
                KeyCode::Left     => app.col_left(),
                KeyCode::Right    => app.col_right(),
                KeyCode::PageUp   => app.row_page(-page),
                KeyCode::PageDown => app.row_page(page),
                KeyCode::Home => {
                    app.table_state.select(Some(0));
                    app.col_offset = 0;
                }
                KeyCode::End => {
                    let last = app.rows.len().saturating_sub(1);
                    app.table_state.select(Some(last));
                }
                _ => {}
            }
        }
    }

    disable_raw_mode()?;
    execute!(term.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    term.show_cursor()?;
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: sqlv <database.db>");
        std::process::exit(1);
    }
    let db_path = &args[1];
    let conn = Connection::open(db_path)
        .with_context(|| format!("Cannot open database: {db_path}"))?;
    run_app(conn, db_path)
}
