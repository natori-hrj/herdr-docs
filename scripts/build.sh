#!/bin/sh
# GitHub installs run this before Herdr registers the plugin. Build the binary and use it to
# bootstrap the host keybinding immediately; the startup hook remains as a retry for installs
# performed before the current server was available.
set -eu

cargo build --release
./target/release/herdr-docs startup
