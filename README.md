# herdr-docs

`herdr-docs` is a Herdr plugin for reading documents in one consistent terminal
view. It is designed for the moment when an agent produces a Markdown file,
design note, PDF, or office document and you want to read it without leaving
Herdr.

The reader keeps the visual model intentionally simple: a quiet header, generous
text margins, a focused document viewport, a prompt-like command field, and a
small shortcut footer. All supported inputs are normalized before rendering, so
the reader does not switch to a different UI for each file format.

## Install

Install the published plugin directly from GitHub:

```sh
herdr plugin install natori-hrj/herdr-docs
```

For local development, use the source checkout instead:

### Local development

From this directory:

```sh
cargo build --release
herdr plugin link "$PWD"
herdr plugin pane open --plugin herdr-docs --entrypoint reader --focus
```

The plugin opens in the current Herdr workspace. It prefers `README.md` when
one exists, then the first readable document in the workspace directory.

To bind it to a key, add this to Herdr's config:

```toml
[[keys.command]]
key = "prefix+d"
type = "plugin_action"
command = "herdr-docs.open"
description = "open document reader"
```

With Herdr's default prefix, press `Ctrl+B`, release it, then press `D`.
If you changed Herdr's prefix, use that prefix instead. Reload the running
Herdr server after editing the config:

```sh
herdr server reload-config
```

To open one specific document:

```sh
herdr plugin pane open \
  --plugin herdr-docs \
  --entrypoint reader \
  --env HERDR_DOC_PATH="$PWD/docs/design.pdf" \
  --focus
```

## Supported input

Markdown, text, source code, JSON/YAML/TOML/CSV, HTML, PDF, DOCX, PPTX, XLSX,
EPUB, ODT, ODP, ODS, and RTF are recognized by the file browser.

Text, Markdown, code, and HTML are handled directly. PDF uses `pdftotext -layout`
with `mutool` as a fallback. Office and EPUB files use `pandoc`, then the
optional `markitdown` command; on macOS, `textutil` is also used for DOCX/RTF/ODT.
Check the local setup with:

```sh
herdr-docs doctor
```

The external converters are deliberately argv-based subprocesses. No document
content is passed through a shell.

## Controls

| Key | Action |
| --- | --- |
| `j` / `k`, arrows | Scroll the document or move in the file list |
| `space`, `PageDown` / `PageUp` | Page scroll |
| `g` / `G` | Top / bottom |
| `Tab` / `b` | Show or hide the file browser |
| `o` | Open a path |
| `/`, `n` | Search / next match |
| `c` | Copy normalized document context for an LLM |
| `r` | Reload the current file |
| `?` | Help |
| `q`, `Esc` | Close the pane |

## LLM context

The same normalized content shown in the reader can be printed for an agent or
piped into another tool:

```sh
herdr-docs context docs/design.pdf
```

Inside the reader, `c` copies a small context envelope containing the document
title, source path, detected format, and normalized content.
