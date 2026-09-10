use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

#[derive(Debug, Clone)]
pub struct LoadedDocument {
    pub path: PathBuf,
    pub format: String,
    pub source: String,
    pub lines: Vec<Line<'static>>,
}

#[derive(Debug)]
pub struct DocumentError(String);

impl DocumentError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for DocumentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for DocumentError {}

const BODY: Color = Color::Rgb(232, 230, 228);
const MUTED: Color = Color::Rgb(148, 153, 163);
const ACCENT: Color = Color::Rgb(255, 207, 128);
const BLUE: Color = Color::Rgb(155, 196, 255);
const CODE: Color = Color::Rgb(192, 218, 211);

pub fn load_document(path: &Path) -> Result<LoadedDocument, String> {
    if !path.exists() {
        return Err(format!("file does not exist: {}", path.display()));
    }
    if path.is_dir() {
        return Err(format!("{} is a directory", path.display()));
    }

    let source = load_source(path).map_err(|error| error.to_string())?;
    let source = sanitize_terminal_text(&source);
    let format = format_label(path);
    let lines = render_document(&source);

    Ok(LoadedDocument {
        path: path.to_path_buf(),
        format,
        source,
        lines,
    })
}

pub fn is_supported_path(path: &Path) -> bool {
    if path.is_dir() {
        return true;
    }

    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if matches!(
        name.as_str(),
        "readme" | "license" | "makefile" | "dockerfile" | "justfile"
    ) {
        return true;
    }

    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(
        extension.as_str(),
        "adoc"
            | "asc"
            | "c"
            | "cc"
            | "cfg"
            | "conf"
            | "cpp"
            | "css"
            | "csv"
            | "docx"
            | "epub"
            | "fish"
            | "go"
            | "h"
            | "html"
            | "htm"
            | "ini"
            | "java"
            | "js"
            | "jsx"
            | "json"
            | "log"
            | "markdown"
            | "md"
            | "mjs"
            | "odp"
            | "ods"
            | "odt"
            | "org"
            | "pdf"
            | "php"
            | "pl"
            | "pptx"
            | "py"
            | "rb"
            | "rs"
            | "rst"
            | "rtf"
            | "sass"
            | "scala"
            | "scss"
            | "sh"
            | "sql"
            | "svg"
            | "swift"
            | "tex"
            | "toml"
            | "ts"
            | "tsv"
            | "tsx"
            | "txt"
            | "xml"
            | "xhtml"
            | "xlsx"
            | "yaml"
            | "yml"
            | "zsh"
    )
}

pub fn context_text(document: &LoadedDocument) -> String {
    let title = document
        .path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("document");
    format!(
        "# {title}\n\nSource: {}\nFormat: {}\n\n---\n\n{}\n",
        document.path.display(),
        document.format,
        document.source.trim_end()
    )
}

pub fn copy_to_clipboard(text: &str) -> Result<(), String> {
    let candidates: &[(&str, &[&str])] = if cfg!(target_os = "macos") {
        &[("pbcopy", &[])]
    } else if cfg!(target_os = "windows") {
        &[("clip", &[])]
    } else {
        &[
            ("wl-copy", &[]),
            ("xclip", &["-selection", "clipboard"]),
            ("xsel", &["--clipboard", "--input"]),
        ]
    };

    let mut failures = Vec::new();
    for (program, args) in candidates {
        let mut child = match Command::new(program)
            .args(args.iter().copied())
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                failures.push(format!("{program}: {error}"));
                continue;
            }
        };

        if let Some(mut stdin) = child.stdin.take() {
            if let Err(error) = stdin.write_all(text.as_bytes()) {
                failures.push(format!("{program}: {error}"));
                let _ = child.kill();
                let _ = child.wait();
                continue;
            }
        }

        match child.wait() {
            Ok(status) if status.success() => return Ok(()),
            Ok(status) => failures.push(format!("{program}: exited with {status}")),
            Err(error) => failures.push(format!("{program}: {error}")),
        }
    }

    Err(format!(
        "no clipboard command succeeded; install pbcopy, wl-copy, xclip, or xsel ({})",
        failures.join("; ")
    ))
}

pub fn print_doctor() {
    println!("herdr-docs converters:");
    println!(
        "  Markdown/text/HTML  built in\n  PDF                 {}",
        if command_available("pdftotext") {
            "pdftotext available"
        } else {
            "install poppler (pdftotext)"
        }
    );
    println!(
        "  Office/EPUB         {}",
        if command_available("pandoc") {
            "pandoc available"
        } else {
            "install pandoc for DOCX/PPTX/XLSX/EPUB"
        }
    );
    println!(
        "  Clipboard           {}",
        ["pbcopy", "wl-copy", "xclip", "xsel", "clip"]
            .iter()
            .find(|command| command_available(command))
            .copied()
            .unwrap_or("no clipboard command found")
    );
}

fn load_source(path: &Path) -> Result<String, DocumentError> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    match extension.as_str() {
        "pdf" => load_pdf(path),
        "docx" | "pptx" | "xlsx" | "epub" | "odt" | "odp" | "ods" => load_office_document(path),
        "rtf" => load_rtf(path),
        "html" | "htm" | "xhtml" | "md" | "markdown" => {
            read_text(path).map(|source| html_to_text(&source))
        }
        _ => read_text(path),
    }
}

fn load_pdf(path: &Path) -> Result<String, DocumentError> {
    let mut errors = Vec::new();

    match Command::new("pdftotext")
        .arg("-layout")
        .arg(path)
        .arg("-")
        .output()
    {
        Ok(output) if output.status.success() => return output_text(output.stdout),
        Ok(output) => errors.push(command_failure("pdftotext", &output)),
        Err(error) => errors.push(format!("pdftotext: {error}")),
    }

    match Command::new("mutool")
        .args(["draw", "-F", "txt"])
        .arg(path)
        .output()
    {
        Ok(output) if output.status.success() => return output_text(output.stdout),
        Ok(output) => errors.push(command_failure("mutool", &output)),
        Err(error) => errors.push(format!("mutool: {error}")),
    }

    Err(DocumentError::new(format!(
        "could not extract PDF text from {}. Install pdftotext (Poppler) or mutool. {}",
        path.display(),
        errors.join("; ")
    )))
}

fn load_office_document(path: &Path) -> Result<String, DocumentError> {
    let mut errors = Vec::new();

    match Command::new("pandoc")
        .args(["-t", "gfm", "--wrap=none"])
        .arg(path)
        .output()
    {
        Ok(output) if output.status.success() => return output_text(output.stdout),
        Ok(output) => errors.push(command_failure("pandoc", &output)),
        Err(error) => errors.push(format!("pandoc: {error}")),
    }

    match Command::new("markitdown").arg(path).output() {
        Ok(output) if output.status.success() => return output_text(output.stdout),
        Ok(output) => errors.push(command_failure("markitdown", &output)),
        Err(error) => errors.push(format!("markitdown: {error}")),
    }

    #[cfg(target_os = "macos")]
    if matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("docx" | "rtf" | "odt")
    ) {
        match Command::new("textutil")
            .args(["-convert", "txt", "-stdout"])
            .arg(path)
            .output()
        {
            Ok(output) if output.status.success() => return output_text(output.stdout),
            Ok(output) => errors.push(command_failure("textutil", &output)),
            Err(error) => errors.push(format!("textutil: {error}")),
        }
    }

    Err(DocumentError::new(format!(
        "no document converter could read {}. Install pandoc (recommended) or markitdown. {}",
        path.display(),
        errors.join("; ")
    )))
}

#[cfg(target_os = "macos")]
fn load_rtf(path: &Path) -> Result<String, DocumentError> {
    match Command::new("textutil")
        .args(["-convert", "txt", "-stdout"])
        .arg(path)
        .output()
    {
        Ok(output) if output.status.success() => output_text(output.stdout),
        Ok(output) => Err(DocumentError::new(command_failure("textutil", &output))),
        Err(error) => Err(DocumentError::new(format!("textutil: {error}"))),
    }
}

#[cfg(not(target_os = "macos"))]
fn load_rtf(path: &Path) -> Result<String, DocumentError> {
    load_office_document(path)
}

fn read_text(path: &Path) -> Result<String, DocumentError> {
    let bytes = std::fs::read(path).map_err(|error| {
        DocumentError::new(format!("could not read {}: {error}", path.display()))
    })?;
    if bytes.contains(&0) {
        return Err(DocumentError::new(format!(
            "{} looks like a binary file; supported converters do not recognize its format",
            path.display()
        )));
    }
    String::from_utf8(bytes).map_err(|error| {
        DocumentError::new(format!(
            "{} is not valid UTF-8 at byte {}; convert it to UTF-8 first",
            path.display(),
            error.utf8_error().valid_up_to()
        ))
    })
}

fn output_text(bytes: Vec<u8>) -> Result<String, DocumentError> {
    String::from_utf8(bytes)
        .map_err(|error| DocumentError::new(format!("converter returned invalid UTF-8: {error}")))
}

fn command_failure(program: &str, output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        format!("{program} exited with {}", output.status)
    } else {
        format!("{program}: {stderr}")
    }
}

fn command_available(program: &str) -> bool {
    Command::new(program)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

fn format_label(path: &Path) -> String {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_uppercase())
        .filter(|extension| !extension.is_empty())
        .unwrap_or_else(|| "TEXT".to_string())
}

fn render_document(source: &str) -> Vec<Line<'static>> {
    let source_lines: Vec<&str> = source.lines().collect();
    let mut rendered = Vec::new();
    let mut in_code = false;

    for (index, raw_line) in source_lines.iter().enumerate() {
        let line = raw_line.trim_end();
        let trimmed = line.trim_start();

        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_code = !in_code;
            let marker = if in_code { "┌" } else { "└" };
            rendered.push(Line::from(vec![
                Span::styled(format!("{marker} "), Style::default().fg(MUTED)),
                Span::styled(trimmed.to_string(), Style::default().fg(MUTED)),
            ]));
            continue;
        }

        if in_code {
            rendered.push(Line::from(vec![
                Span::styled("│ ", Style::default().fg(MUTED)),
                Span::styled(line.to_string(), Style::default().fg(CODE)),
            ]));
            continue;
        }

        if trimmed.is_empty() {
            rendered.push(Line::from(""));
            continue;
        }

        if let Some((level, heading)) = markdown_heading(trimmed) {
            push_blank_if_needed(&mut rendered);
            let (prefix, color) = match level {
                1 => ("◆ ", ACCENT),
                2 => ("◇ ", BLUE),
                _ => ("· ", Color::Rgb(196, 190, 255)),
            };
            let style = Style::default().fg(color).add_modifier(Modifier::BOLD);
            let mut spans = vec![Span::styled(prefix.to_string(), style)];
            spans.extend(inline_spans(heading, style));
            rendered.push(Line::from(spans));
            if source_lines
                .get(index + 1)
                .is_some_and(|next| !next.trim().is_empty())
            {
                rendered.push(Line::from(""));
            }
            continue;
        }

        if is_horizontal_rule(trimmed) {
            rendered.push(Line::from(Span::styled(
                "────────────────────────────────────────",
                Style::default().fg(MUTED),
            )));
            continue;
        }

        if let Some((prefix, text)) = list_prefix(trimmed) {
            let mut spans = vec![Span::styled(
                format!("{prefix} "),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            )];
            spans.extend(inline_spans(text, Style::default().fg(BODY)));
            rendered.push(Line::from(spans));
            continue;
        }

        if trimmed.starts_with('>') {
            let quote = trimmed.trim_start_matches('>').trim_start();
            let mut spans = vec![Span::styled("> ", Style::default().fg(MUTED))];
            spans.extend(inline_spans(
                quote,
                Style::default().fg(BODY).add_modifier(Modifier::ITALIC),
            ));
            rendered.push(Line::from(spans));
            continue;
        }

        let style = if trimmed.starts_with('|') && trimmed.ends_with('|') {
            Style::default().fg(Color::Rgb(214, 221, 235))
        } else {
            Style::default().fg(BODY)
        };
        rendered.push(Line::from(inline_spans(line, style)));
    }

    if rendered.is_empty() {
        rendered.push(Line::from(Span::styled(
            "(empty document)",
            Style::default().fg(MUTED),
        )));
    }
    rendered
}

fn markdown_heading(line: &str) -> Option<(usize, &str)> {
    let hashes = line
        .chars()
        .take_while(|character| *character == '#')
        .count();
    if !(1..=6).contains(&hashes) || !line[hashes..].starts_with(' ') {
        return None;
    }
    Some((hashes, line[hashes..].trim()))
}

fn list_prefix(line: &str) -> Option<(&str, &str)> {
    if line.starts_with("- [ ] ") || line.starts_with("- [x] ") || line.starts_with("- [X] ") {
        return Some(("□", line[6..].trim_start()));
    }
    if let Some(rest) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
        return Some(("•", rest));
    }

    let dot = line.find(". ")?;
    if dot > 0
        && line[..dot]
            .chars()
            .all(|character| character.is_ascii_digit())
    {
        return Some((&line[..=dot], line[dot + 2..].trim_start()));
    }
    None
}

fn is_horizontal_rule(line: &str) -> bool {
    let stripped: String = line
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();
    stripped.len() >= 3
        && stripped
            .chars()
            .all(|character| matches!(character, '-' | '*' | '_'))
}

fn push_blank_if_needed(lines: &mut Vec<Line<'static>>) {
    if !lines.is_empty() && lines.last().is_some_and(|line| line.width() != 0) {
        lines.push(Line::from(""));
    }
}

fn inline_spans(input: &str, base: Style) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut index = 0;
    let mut plain_start = 0;

    while index < input.len() {
        let remainder = &input[index..];
        if let Some((end, label)) = markdown_link(remainder) {
            push_plain_span(&mut spans, &input[plain_start..index], base);
            spans.push(Span::styled(
                label.to_string(),
                base.fg(BLUE).add_modifier(Modifier::UNDERLINED),
            ));
            index += end;
            plain_start = index;
            continue;
        }

        let (marker, style, marker_len) = if remainder.starts_with("**") {
            ("**", base.add_modifier(Modifier::BOLD), 2)
        } else if remainder.starts_with('`') {
            ("`", Style::default().fg(CODE), 1)
        } else if remainder.starts_with('*') {
            ("*", base.add_modifier(Modifier::ITALIC), 1)
        } else if remainder.starts_with('_') {
            ("_", base.add_modifier(Modifier::ITALIC), 1)
        } else {
            index += remainder.chars().next().map(char::len_utf8).unwrap_or(1);
            continue;
        };

        let Some(close_offset) = remainder[marker_len..].find(marker) else {
            index += marker_len;
            continue;
        };
        let close = index + marker_len + close_offset;
        if close == index + marker_len {
            index += marker_len;
            continue;
        }

        push_plain_span(&mut spans, &input[plain_start..index], base);
        spans.push(Span::styled(
            input[index + marker_len..close].to_string(),
            style,
        ));
        index = close + marker_len;
        plain_start = index;
    }

    push_plain_span(&mut spans, &input[plain_start..], base);
    spans
}

fn markdown_link(input: &str) -> Option<(usize, &str)> {
    if !input.starts_with('[') || input.starts_with("![") {
        return None;
    }
    let close_label = input.find("](")?;
    if close_label == 1 {
        return None;
    }
    let close_url = input[close_label + 2..].find(')')? + close_label + 2;
    Some((close_url + 1, &input[1..close_label]))
}

fn push_plain_span(spans: &mut Vec<Span<'static>>, text: &str, style: Style) {
    if !text.is_empty() {
        spans.push(Span::styled(text.to_string(), style));
    }
}

fn html_to_text(source: &str) -> String {
    let mut output = String::with_capacity(source.len());
    let mut in_tag = false;
    let mut tag = String::new();

    for character in source.chars() {
        if in_tag {
            if character == '>' {
                let normalized = tag.trim().to_ascii_lowercase();
                let tag_name = normalized
                    .trim_start_matches('/')
                    .split_whitespace()
                    .next()
                    .unwrap_or_default();
                let is_block_tag =
                    matches!(tag_name, "br" | "p" | "div" | "li" | "h1" | "h2" | "h3");
                if is_block_tag
                    && (tag_name == "br" || !output.is_empty())
                    && !output.ends_with('\n')
                {
                    output.push('\n');
                }
                in_tag = false;
                tag.clear();
            } else {
                tag.push(character);
            }
        } else if character == '<' {
            in_tag = true;
            tag.clear();
        } else {
            output.push(character);
        }
    }

    decode_html_entities(&output)
}

fn decode_html_entities(source: &str) -> String {
    source
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

fn sanitize_terminal_text(input: &str) -> String {
    #[derive(Clone, Copy)]
    enum State {
        Normal,
        Escape,
        Csi,
        Osc,
        OscEscape,
    }

    let mut state = State::Normal;
    let mut output = String::with_capacity(input.len());
    for character in input.chars() {
        match state {
            State::Normal => match character {
                '\x1b' => state = State::Escape,
                '\r' => {}
                '\t' => output.push_str("    "),
                '\n' => output.push('\n'),
                character if character.is_control() => output.push('�'),
                character => output.push(character),
            },
            State::Escape => {
                state = match character {
                    '[' => State::Csi,
                    ']' => State::Osc,
                    _ => State::Normal,
                };
            }
            State::Csi => {
                if ('@'..='~').contains(&character) {
                    state = State::Normal;
                }
            }
            State::Osc => {
                state = if character == '\x07' {
                    State::Normal
                } else if character == '\x1b' {
                    State::OscEscape
                } else {
                    State::Osc
                };
            }
            State::OscEscape => {
                state = if character == '\\' {
                    State::Normal
                } else {
                    State::Osc
                };
            }
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_headings_and_inline_styles_are_rendered() {
        let lines = render_document("# Title\n\nA **bold** and `code` line.");
        assert_eq!(lines[0].width(), "◆ Title".chars().count());
        assert!(lines.iter().any(|line| {
            line.spans
                .iter()
                .any(|span| span.content.as_ref() == "bold")
        }));
    }

    #[test]
    fn markdown_links_show_their_label_without_the_url() {
        let lines = render_document("Read [the guide](https://example.com) here.");
        assert!(lines.iter().any(|line| {
            line.spans
                .iter()
                .any(|span| span.content.as_ref() == "the guide")
        }));
        assert!(!lines.iter().any(|line| {
            line.spans
                .iter()
                .any(|span| span.content.as_ref().contains("https://example.com"))
        }));
    }

    #[test]
    fn terminal_escape_sequences_are_removed() {
        assert_eq!(
            sanitize_terminal_text("hello\x1b[31m red\x1b[0m"),
            "hello red"
        );
        assert_eq!(sanitize_terminal_text("a\x1b]0;title\x07b"), "ab");
    }

    #[test]
    fn html_tags_become_readable_text() {
        assert_eq!(
            html_to_text("<h1>Hello</h1><p>World &amp; all</p>"),
            "Hello\nWorld & all\n"
        );
    }

    #[test]
    fn supported_extensions_include_the_common_document_formats() {
        for path in ["a.md", "a.pdf", "a.docx", "a.pptx", "a.xlsx", "a.epub"] {
            assert!(is_supported_path(Path::new(path)), "{path}");
        }
    }
}
