use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, BorderType, Borders, Clear, List, ListItem, ListState, Padding, Paragraph, Wrap,
};
use ratatui::Terminal;

use crate::document::{self, LoadedDocument};

const BODY: Color = Color::Rgb(232, 230, 228);
const MUTED: Color = Color::Rgb(148, 153, 163);
const ACCENT: Color = Color::Rgb(255, 207, 128);
const PANEL: Color = Color::Rgb(178, 183, 195);

pub fn run_reader(initial_path: Option<PathBuf>) -> Result<(), String> {
    enable_raw_mode().map_err(|error| format!("could not enable raw mode: {error}"))?;
    let mut stdout = io::stdout();
    if let Err(error) = execute!(stdout, EnterAlternateScreen, EnableMouseCapture) {
        let _ = disable_raw_mode();
        return Err(format!("could not initialize terminal: {error}"));
    }

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = match Terminal::new(backend) {
        Ok(terminal) => terminal,
        Err(error) => {
            let _ = disable_raw_mode();
            return Err(format!("could not create terminal backend: {error}"));
        }
    };
    let result = ReaderApp::new(initial_path).run(&mut terminal);

    let _ = disable_raw_mode();
    let _ = execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    );
    let _ = terminal.show_cursor();
    result
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Browser,
    Document,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputMode {
    Open,
    Search,
}

#[derive(Debug, Clone)]
struct FileEntry {
    path: PathBuf,
    is_dir: bool,
}

struct ReaderApp {
    root_dir: PathBuf,
    browser_dir: PathBuf,
    entries: Vec<FileEntry>,
    selected: usize,
    document: Option<LoadedDocument>,
    focus: Focus,
    show_browser: bool,
    input_mode: Option<InputMode>,
    input: String,
    input_cursor: usize,
    search_query: Option<String>,
    last_match_line: Option<usize>,
    scroll: usize,
    status: String,
    show_help: bool,
    browser_area: Rect,
    document_area: Rect,
}

impl ReaderApp {
    fn new(initial_path: Option<PathBuf>) -> Self {
        let root_dir = initial_directory();
        let mut app = Self {
            root_dir: root_dir.clone(),
            browser_dir: root_dir,
            entries: Vec::new(),
            selected: 0,
            document: None,
            focus: Focus::Document,
            show_browser: false,
            input_mode: None,
            input: String::new(),
            input_cursor: 0,
            search_query: None,
            last_match_line: None,
            scroll: 0,
            status: String::new(),
            show_help: false,
            browser_area: Rect::default(),
            document_area: Rect::default(),
        };
        app.refresh_entries();

        let explicit_path = initial_path.or_else(env_document_path);
        let has_explicit_path = explicit_path.is_some();

        if let Some(path) = explicit_path {
            app.open_path(&path);
        } else if let Some(path) = app.preferred_document() {
            app.open_path(&path);
        }

        if app.document.is_none() || !has_explicit_path {
            app.show_browser = true;
            app.focus = Focus::Browser;
        }
        app
    }

    fn run<B: ratatui::backend::Backend>(
        &mut self,
        terminal: &mut Terminal<B>,
    ) -> Result<(), String> {
        loop {
            terminal
                .draw(|frame| self.draw(frame))
                .map_err(|error| format!("could not draw reader: {error}"))?;

            if !event::poll(Duration::from_millis(120))
                .map_err(|error| format!("could not poll terminal events: {error}"))?
            {
                continue;
            }

            match event::read()
                .map_err(|error| format!("could not read terminal event: {error}"))?
            {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    if self.handle_key(key) {
                        break;
                    }
                }
                Event::Mouse(mouse) => self.handle_mouse(mouse),
                _ => {}
            }
        }
        Ok(())
    }

    fn draw(&mut self, frame: &mut ratatui::Frame<'_>) {
        let area = frame.area();
        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(1),
                Constraint::Length(3),
                Constraint::Length(1),
            ])
            .split(area);

        self.draw_header(frame, sections[0]);
        let body = if self.show_browser {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Length(28),
                    Constraint::Length(1),
                    Constraint::Min(1),
                ])
                .split(sections[1])
        } else {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Min(1)])
                .split(sections[1])
        };

        if self.show_browser {
            self.browser_area = body[0];
            self.document_area = body[2];
            self.draw_browser(frame, body[0]);
            frame.render_widget(
                Paragraph::new("│").style(Style::default().fg(Color::Rgb(75, 80, 90))),
                body[1],
            );
            self.draw_document(frame, body[2]);
        } else {
            self.browser_area = Rect::default();
            self.document_area = body[0];
            self.draw_document(frame, body[0]);
        }

        self.draw_prompt(frame, sections[2]);
        self.draw_footer(frame, sections[3]);

        if self.show_help {
            self.draw_help(frame, area);
        }
    }

    fn draw_header(&self, frame: &mut ratatui::Frame<'_>, area: Rect) {
        let (title, format) = self
            .document
            .as_ref()
            .map(|document| {
                (
                    document
                        .path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("document")
                        .to_string(),
                    document.format.clone(),
                )
            })
            .unwrap_or_else(|| ("Open a document".to_string(), "READER".to_string()));
        let path = self
            .document
            .as_ref()
            .map(|document| display_path(&document.path, &self.root_dir))
            .unwrap_or_else(|| display_path(&self.browser_dir, &self.root_dir));

        let title_line = Line::from(vec![
            Span::styled("▰ ", Style::default().fg(ACCENT)),
            Span::styled(
                title,
                Style::default().fg(BODY).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  {format}"),
                Style::default().fg(MUTED).add_modifier(Modifier::DIM),
            ),
        ]);
        let path_line = Line::from(vec![
            Span::styled("  main  ", Style::default().fg(MUTED)),
            Span::styled(path, Style::default().fg(MUTED).add_modifier(Modifier::DIM)),
        ]);

        frame.render_widget(
            Paragraph::new(Text::from(vec![title_line, path_line])).block(
                Block::default()
                    .borders(Borders::BOTTOM)
                    .border_style(Style::default().fg(Color::Rgb(70, 75, 84)))
                    .padding(Padding::horizontal(1)),
            ),
            area,
        );
    }

    fn draw_browser(&self, frame: &mut ratatui::Frame<'_>, area: Rect) {
        let title = format!(" Files · {} ", shorten_name(&self.browser_dir));
        let items: Vec<ListItem<'static>> = self
            .entries
            .iter()
            .map(|entry| {
                let is_parent = entry.is_dir
                    && self
                        .browser_dir
                        .parent()
                        .is_some_and(|parent| parent == entry.path);
                let name = if is_parent {
                    "↩ ..".to_string()
                } else {
                    entry
                        .path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("?")
                        .to_string()
                };
                let (prefix, style) = if entry.is_dir {
                    ("▸ ", Style::default().fg(PANEL))
                } else {
                    ("  ", Style::default().fg(BODY))
                };
                ListItem::new(Line::from(vec![
                    Span::styled(prefix, style),
                    Span::styled(name, style),
                ]))
            })
            .collect();

        if items.is_empty() {
            frame.render_widget(
                Paragraph::new(Text::from(vec![
                    Line::from(Span::styled(
                        "No readable documents here.",
                        Style::default().fg(MUTED),
                    )),
                    Line::from(""),
                    Line::from(Span::styled(
                        "Press o to enter a path.",
                        Style::default().fg(MUTED),
                    )),
                ]))
                .block(
                    Block::default()
                        .title(title)
                        .borders(Borders::TOP)
                        .border_style(Style::default().fg(Color::Rgb(70, 75, 84)))
                        .padding(Padding::top(1)),
                ),
                area,
            );
            return;
        }

        let list = List::new(items)
            .block(
                Block::default()
                    .title(title)
                    .borders(Borders::TOP)
                    .border_style(Style::default().fg(Color::Rgb(70, 75, 84))),
            )
            .highlight_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
            .highlight_symbol("› ");
        let mut state = ListState::default();
        state.select(Some(
            self.selected.min(self.entries.len().saturating_sub(1)),
        ));
        frame.render_stateful_widget(list, area, &mut state);
    }

    fn draw_document(&self, frame: &mut ratatui::Frame<'_>, area: Rect) {
        let inner = Rect {
            x: area.x.saturating_add(2),
            y: area.y.saturating_add(1),
            width: area.width.saturating_sub(4),
            height: area.height.saturating_sub(2),
        };

        let Some(document) = self.document.as_ref() else {
            let empty = Paragraph::new(Text::from(vec![
                Line::from(Span::styled(
                    "Open a document to begin",
                    Style::default().fg(BODY).add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "Tab  files     o  open path     q  close",
                    Style::default().fg(MUTED),
                )),
            ]))
            .alignment(ratatui::layout::Alignment::Center)
            .block(Block::default().padding(Padding::top(inner.height.saturating_div(3))));
            frame.render_widget(empty, area);
            return;
        };

        let paragraph = Paragraph::new(Text::from(document.lines.clone()))
            .wrap(Wrap { trim: false })
            .scroll((self.effective_scroll(document, inner), 0));
        frame.render_widget(paragraph, inner);
    }

    fn draw_prompt(&self, frame: &mut ratatui::Frame<'_>, area: Rect) {
        let content = if self.input_mode.is_some() {
            let before = &self.input[..self.input_cursor];
            let after = &self.input[self.input_cursor..];
            Line::from(vec![
                Span::styled(
                    "› ",
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                ),
                Span::styled(before.to_string(), Style::default().fg(BODY)),
                Span::styled("▌", Style::default().fg(ACCENT)),
                Span::styled(after.to_string(), Style::default().fg(BODY)),
            ])
        } else {
            let hint = if self.status.is_empty() {
                "Ask about this document with / to search, or press o to open another file"
            } else {
                &self.status
            };
            Line::from(vec![
                Span::styled(
                    "› ",
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                ),
                Span::styled(hint.to_string(), Style::default().fg(MUTED)),
            ])
        };

        let title = match self.input_mode {
            Some(InputMode::Open) => " Open document ",
            Some(InputMode::Search) => " Search document ",
            None => " Continue ",
        };
        frame.render_widget(
            Paragraph::new(content).block(
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::Rgb(120, 126, 138)))
                    .padding(Padding::horizontal(1)),
            ),
            area,
        );
    }

    fn draw_footer(&self, frame: &mut ratatui::Frame<'_>, area: Rect) {
        let keys = [
            ("tab", "files"),
            ("o", "open"),
            ("/", "search"),
            ("n", "next"),
            ("c", "copy"),
            ("r", "reload"),
            ("?", "help"),
            ("q", "close"),
        ];
        let mut spans = Vec::new();
        for (index, (key, label)) in keys.iter().enumerate() {
            if index > 0 {
                spans.push(Span::styled(
                    "  ·  ",
                    Style::default().fg(Color::Rgb(75, 80, 90)),
                ));
            }
            spans.push(Span::styled(
                (*key).to_string(),
                Style::default().fg(BODY).add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled(
                format!(" {label}"),
                Style::default().fg(MUTED),
            ));
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    fn draw_help(&self, frame: &mut ratatui::Frame<'_>, area: Rect) {
        let popup = centered_rect(72, 66, area);
        let lines = vec![
            Line::from(Span::styled(
                "A calm reader for everything your agents produce.",
                Style::default().fg(BODY),
            )),
            Line::from(""),
            help_line("j / k", "scroll"),
            help_line("space / PgDn", "page down"),
            help_line("g / G", "top / bottom"),
            help_line("Tab / b", "file browser"),
            help_line("o", "open a path"),
            help_line("/ / n", "search / next match"),
            help_line("c", "copy normalized LLM context"),
            help_line("r", "reload current file"),
            help_line("q / Esc", "close"),
        ];
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(Text::from(lines))
                .block(
                    Block::default()
                        .title(" Help ")
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .border_style(Style::default().fg(ACCENT))
                        .padding(Padding::horizontal(2)),
                )
                .wrap(Wrap { trim: false }),
            popup,
        );
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        if self.show_help {
            if matches!(
                key.code,
                KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q')
            ) {
                self.show_help = false;
            }
            return false;
        }

        if self.input_mode.is_some() {
            self.handle_input_key(key);
            return false;
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return true;
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return true,
            KeyCode::Char('?') => self.show_help = true,
            KeyCode::Tab => self.toggle_browser(true),
            KeyCode::Char('b') => self.toggle_browser(false),
            KeyCode::Char('o') => self.begin_input(InputMode::Open),
            KeyCode::Char('/') if self.document.is_some() => self.begin_input(InputMode::Search),
            KeyCode::Char('n') if self.search_query.is_some() => self.find_next_match(),
            KeyCode::Char('r') => self.reload_document(),
            KeyCode::Char('c') => self.copy_context(),
            KeyCode::Char('g') => self.scroll = 0,
            KeyCode::Char('G') => self.scroll_to_bottom(),
            KeyCode::Char('j') | KeyCode::Down => self.move_or_scroll(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_or_scroll(-1),
            KeyCode::PageDown | KeyCode::Char(' ') => self.page_scroll(1),
            KeyCode::PageUp => self.page_scroll(-1),
            KeyCode::Enter if self.focus == Focus::Browser => self.open_selected(),
            _ => {}
        }
        false
    }

    fn handle_input_key(&mut self, key: KeyEvent) {
        let Some(mode) = self.input_mode else {
            return;
        };
        match key.code {
            KeyCode::Esc => self.cancel_input(),
            KeyCode::Enter => {
                let value = self.input.trim().to_string();
                self.cancel_input();
                if value.is_empty() {
                    return;
                }
                match mode {
                    InputMode::Open => self.open_path(Path::new(&value)),
                    InputMode::Search => {
                        self.search_query = Some(value.to_ascii_lowercase());
                        self.last_match_line = None;
                        self.find_next_match();
                    }
                }
            }
            KeyCode::Backspace => {
                if self.input_cursor > 0 {
                    let previous = previous_char_boundary(&self.input, self.input_cursor);
                    self.input.drain(previous..self.input_cursor);
                    self.input_cursor = previous;
                }
            }
            KeyCode::Delete => {
                if self.input_cursor < self.input.len() {
                    let next = next_char_boundary(&self.input, self.input_cursor);
                    self.input.drain(self.input_cursor..next);
                }
            }
            KeyCode::Left => {
                self.input_cursor = previous_char_boundary(&self.input, self.input_cursor);
            }
            KeyCode::Right => {
                self.input_cursor = next_char_boundary(&self.input, self.input_cursor);
            }
            KeyCode::Home => self.input_cursor = 0,
            KeyCode::End => self.input_cursor = self.input.len(),
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.input.insert(self.input_cursor, character);
                self.input_cursor += character.len_utf8();
            }
            _ => {}
        }
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) {
        match mouse.kind {
            MouseEventKind::ScrollUp => {
                if self.show_browser && rect_contains(self.browser_area, mouse.column, mouse.row) {
                    self.move_selection(-3);
                } else {
                    self.scroll_by(-3);
                }
            }
            MouseEventKind::ScrollDown => {
                if self.show_browser && rect_contains(self.browser_area, mouse.column, mouse.row) {
                    self.move_selection(3);
                } else {
                    self.scroll_by(3);
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if self.show_browser && rect_contains(self.browser_area, mouse.column, mouse.row) {
                    let index = mouse
                        .row
                        .saturating_sub(self.browser_area.y.saturating_add(1))
                        as usize;
                    if index < self.entries.len() {
                        self.selected = index;
                        self.focus = Focus::Browser;
                    }
                } else if rect_contains(self.document_area, mouse.column, mouse.row) {
                    self.focus = Focus::Document;
                }
            }
            _ => {}
        }
    }

    fn toggle_browser(&mut self, focus_browser: bool) {
        if self.show_browser && !focus_browser {
            self.show_browser = false;
            self.focus = Focus::Document;
        } else if self.show_browser {
            self.focus = match self.focus {
                Focus::Browser => Focus::Document,
                Focus::Document => Focus::Browser,
            };
        } else {
            self.show_browser = true;
            self.focus = Focus::Browser;
        }
    }

    fn begin_input(&mut self, mode: InputMode) {
        self.input_mode = Some(mode);
        self.input.clear();
        self.input_cursor = 0;
        self.status.clear();
    }

    fn cancel_input(&mut self) {
        self.input_mode = None;
        self.input.clear();
        self.input_cursor = 0;
    }

    fn refresh_entries(&mut self) {
        let mut entries = Vec::new();
        match fs::read_dir(&self.browser_dir) {
            Ok(read_dir) => {
                for entry in read_dir.flatten() {
                    let path = entry.path();
                    let name = path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or_default();
                    if name.starts_with('.') {
                        continue;
                    }
                    let is_dir = path.is_dir();
                    if is_dir || document::is_supported_path(&path) {
                        entries.push(FileEntry { path, is_dir });
                    }
                }
            }
            Err(error) => {
                self.status = format!("Cannot read {}: {error}", self.browser_dir.display())
            }
        }
        if let Some(parent) = self.browser_dir.parent() {
            if parent != self.browser_dir {
                entries.push(FileEntry {
                    path: parent.to_path_buf(),
                    is_dir: true,
                });
            }
        }
        entries.sort_by(|left, right| {
            let left_is_parent = left.is_dir
                && self
                    .browser_dir
                    .parent()
                    .is_some_and(|parent| parent == left.path);
            let right_is_parent = right.is_dir
                && self
                    .browser_dir
                    .parent()
                    .is_some_and(|parent| parent == right.path);
            right_is_parent
                .cmp(&left_is_parent)
                .then_with(|| right.is_dir.cmp(&left.is_dir))
                .then_with(|| left.path.cmp(&right.path))
        });
        self.entries = entries;
        self.selected = self.selected.min(self.entries.len().saturating_sub(1));
    }

    fn preferred_document(&self) -> Option<PathBuf> {
        self.entries
            .iter()
            .find(|entry| {
                !entry.is_dir
                    && entry
                        .path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.eq_ignore_ascii_case("readme.md"))
            })
            .or_else(|| self.entries.iter().find(|entry| !entry.is_dir))
            .map(|entry| entry.path.clone())
    }

    fn open_selected(&mut self) {
        let Some(entry) = self.entries.get(self.selected) else {
            return;
        };
        let path = entry.path.clone();
        self.open_path(&path);
    }

    fn open_path(&mut self, raw_path: &Path) {
        let path = resolve_path(raw_path, &self.browser_dir);
        if path.is_dir() {
            self.browser_dir = path;
            self.refresh_entries();
            self.show_browser = true;
            self.focus = Focus::Browser;
            self.status.clear();
            return;
        }

        match document::load_document(&path) {
            Ok(document) => {
                self.browser_dir = path
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| self.browser_dir.clone());
                self.document = Some(document);
                self.refresh_entries();
                self.selected = self
                    .entries
                    .iter()
                    .position(|entry| entry.path == path)
                    .unwrap_or(0);
                self.scroll = 0;
                self.last_match_line = None;
                self.status.clear();
                self.focus = Focus::Document;
            }
            Err(error) => self.status = error,
        }
    }

    fn reload_document(&mut self) {
        let Some(path) = self.document.as_ref().map(|document| document.path.clone()) else {
            self.status = "No document is open".to_string();
            return;
        };
        match document::load_document(&path) {
            Ok(document) => {
                self.document = Some(document);
                self.scroll = 0;
                self.status = "Reloaded".to_string();
            }
            Err(error) => self.status = error,
        }
    }

    fn copy_context(&mut self) {
        let Some(document) = self.document.as_ref() else {
            self.status = "Open a document before copying context".to_string();
            return;
        };
        match document::copy_to_clipboard(&document::context_text(document)) {
            Ok(()) => self.status = "Copied normalized LLM context".to_string(),
            Err(error) => self.status = error,
        }
    }

    fn move_or_scroll(&mut self, amount: isize) {
        if self.focus == Focus::Browser {
            self.move_selection(amount);
        } else {
            self.scroll_by(amount);
        }
    }

    fn move_selection(&mut self, amount: isize) {
        if self.entries.is_empty() {
            return;
        }
        if amount < 0 {
            self.selected = self.selected.saturating_sub(amount.unsigned_abs());
        } else {
            self.selected = self
                .selected
                .saturating_add(amount as usize)
                .min(self.entries.len().saturating_sub(1));
        }
    }

    fn page_scroll(&mut self, direction: isize) {
        let amount = self.document_area.height.max(1) as usize;
        if direction < 0 {
            self.scroll = self.scroll.saturating_sub(amount);
        } else {
            self.scroll = self.scroll.saturating_add(amount);
        }
        self.scroll_to_valid_range();
    }

    fn scroll_by(&mut self, amount: isize) {
        if amount < 0 {
            self.scroll = self.scroll.saturating_sub(amount.unsigned_abs());
        } else {
            self.scroll = self.scroll.saturating_add(amount as usize);
        }
        self.scroll_to_valid_range();
    }

    fn scroll_to_bottom(&mut self) {
        if let Some(document) = self.document.as_ref() {
            self.scroll = self.max_scroll(
                document,
                self.document_content_width(),
                self.document_area.height.saturating_sub(2),
            );
        }
    }

    fn scroll_to_valid_range(&mut self) {
        if let Some(document) = self.document.as_ref() {
            self.scroll = self.scroll.min(self.max_scroll(
                document,
                self.document_content_width(),
                self.document_area.height.saturating_sub(2),
            ));
        }
    }

    fn effective_scroll(&self, document: &LoadedDocument, area: Rect) -> u16 {
        self.scroll
            .min(self.max_scroll(document, area.width, area.height))
            .min(u16::MAX as usize) as u16
    }

    fn max_scroll(&self, document: &LoadedDocument, width: u16, height: u16) -> usize {
        let paragraph =
            Paragraph::new(Text::from(document.lines.clone())).wrap(Wrap { trim: false });
        let total = paragraph.line_count(width.max(1));
        total.saturating_sub(height.max(1) as usize)
    }

    fn document_content_width(&self) -> u16 {
        self.document_area.width.saturating_sub(4).max(1)
    }

    fn find_next_match(&mut self) {
        let Some(query) = self.search_query.clone() else {
            return;
        };
        let Some(document) = self.document.as_ref() else {
            return;
        };
        let lines: Vec<&str> = document.source.lines().collect();
        if lines.is_empty() {
            self.status = "Document is empty".to_string();
            return;
        }
        let start = self
            .last_match_line
            .map_or(0, |line| line.saturating_add(1));
        let found = (0..lines.len())
            .map(|offset| (start + offset) % lines.len())
            .find(|index| lines[*index].to_ascii_lowercase().contains(&query));
        let max_scroll = self.max_scroll(
            document,
            self.document_content_width(),
            self.document_area.height.saturating_sub(2),
        );
        match found {
            Some(line) => {
                self.last_match_line = Some(line);
                self.scroll = line.min(max_scroll);
                self.status = format!("Match on line {}", line + 1);
            }
            None => self.status = format!("No match for {query}"),
        }
    }
}

fn initial_directory() -> PathBuf {
    if let Some(path) = std::env::var_os("HERDR_DOCS_ROOT") {
        let path = PathBuf::from(path);
        if path.is_dir() {
            return path;
        }
    }

    if let Some(context) = std::env::var_os("HERDR_PLUGIN_CONTEXT_JSON") {
        let context = context.to_string_lossy();
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&context) {
            for key in ["focused_pane_cwd", "workspace_cwd"] {
                if let Some(path) = value.get(key).and_then(serde_json::Value::as_str) {
                    let path = PathBuf::from(path);
                    if path.is_dir() {
                        return path;
                    }
                }
            }
            if let Some(path) = value
                .get("worktree")
                .and_then(|worktree| worktree.get("checkout_path"))
                .and_then(serde_json::Value::as_str)
            {
                let path = PathBuf::from(path);
                if path.is_dir() {
                    return path;
                }
            }
        }
    }

    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn env_document_path() -> Option<PathBuf> {
    std::env::var_os("HERDR_DOC_PATH").map(PathBuf::from)
}

fn resolve_path(raw_path: &Path, base: &Path) -> PathBuf {
    let raw = raw_path.to_string_lossy();
    if raw == "~" {
        return home_dir().unwrap_or_else(|| raw_path.to_path_buf());
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        if let Some(home) = home_dir() {
            return home.join(rest);
        }
    }
    if raw_path.is_absolute() {
        raw_path.to_path_buf()
    } else {
        base.join(raw_path)
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
}

fn display_path(path: &Path, base: &Path) -> String {
    if let Ok(relative) = path.strip_prefix(base) {
        if relative.as_os_str().is_empty() {
            return ".".to_string();
        }
        return format!("./{}", relative.display());
    }
    if let Some(home) = home_dir().and_then(|home| path.strip_prefix(home).ok()) {
        format!("~/{}", home.display())
    } else {
        path.display().to_string()
    }
}

fn shorten_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.to_string())
        .unwrap_or_else(|| path.display().to_string())
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

fn rect_contains(rect: Rect, column: u16, row: u16) -> bool {
    column >= rect.x
        && column < rect.x.saturating_add(rect.width)
        && row >= rect.y
        && row < rect.y.saturating_add(rect.height)
}

fn help_line(key: &str, description: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            key.to_string(),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("  {description}"), Style::default().fg(MUTED)),
    ])
}

fn previous_char_boundary(value: &str, index: usize) -> usize {
    value[..index]
        .char_indices()
        .last()
        .map(|(offset, _)| offset)
        .unwrap_or(0)
}

fn next_char_boundary(value: &str, index: usize) -> usize {
    value[index..]
        .chars()
        .next()
        .map(|character| index + character.len_utf8())
        .unwrap_or(index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_resolution_supports_home_and_relative_paths() {
        assert_eq!(
            resolve_path(Path::new("docs/a.md"), Path::new("/tmp/project")),
            PathBuf::from("/tmp/project/docs/a.md")
        );
        if let Some(home) = home_dir() {
            assert_eq!(
                resolve_path(Path::new("~/notes.md"), Path::new("/tmp")),
                home.join("notes.md")
            );
        }
    }

    #[test]
    fn character_cursor_moves_without_splitting_utf8() {
        let value = "あb";
        let cursor = value.len();
        let previous = previous_char_boundary(value, cursor);
        assert_eq!(&value[previous..], "b");
        assert_eq!(next_char_boundary(value, previous), cursor);
    }
}
