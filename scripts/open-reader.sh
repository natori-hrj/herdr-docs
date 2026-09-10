#!/bin/sh
# Herdr actions do not have a PTY, so the reader must be opened as a pane.
set -eu

herdr_bin="${HERDR_BIN_PATH:-herdr}"

exec "$herdr_bin" plugin pane open \
    --plugin herdr-docs \
    --entrypoint reader \
    --focus
