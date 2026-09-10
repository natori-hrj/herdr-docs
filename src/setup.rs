use std::env;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

const KEYBINDING: &str = "prefix+d";
const ACTION: &str = "herdr-docs.open";
const MARKER_NAME: &str = "keybinding-v1";
const MARKER_CONTENT: &str = "herdr-docs configured its keybinding once\n";

/// Configure the reader's launch key when Herdr runs the plugin startup hook.
///
/// Herdr owns keybindings, so they cannot live in the plugin manifest. This hook provides the
/// same first-run convenience as herdr-lazy while refusing to overwrite an existing binding.
pub fn run_startup() -> Result<(), String> {
    if env::var_os("HERDR_DOCS_NO_BOOTSTRAP").is_some() {
        return Ok(());
    }

    let marker = setup_marker();
    if marker.as_ref().is_some_and(|path| path.exists()) {
        return Ok(());
    }

    let config_path = config_path()
        .ok_or_else(|| "cannot locate Herdr config.toml for keybinding setup".to_string())?;
    let existing = match fs::read_to_string(&config_path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(format!("could not read {}: {error}", config_path.display()));
        }
    };

    if has_action_binding(&existing) {
        println!("herdr-docs: {ACTION} is already configured; leaving config unchanged");
        record_marker(marker.as_deref());
        return Ok(());
    }

    if bound_keys(&existing).iter().any(|key| key == KEYBINDING) {
        println!("herdr-docs: {KEYBINDING} is already used; leaving config unchanged");
        println!(
            "  add a different key for {ACTION} in {}",
            config_path.display()
        );
        record_marker(marker.as_deref());
        return Ok(());
    }

    if !existing.is_empty() {
        let backup = config_path.with_extension("toml.herdr-docs-backup");
        fs::copy(&config_path, &backup).map_err(|error| {
            format!(
                "could not back up {} to {}: {error}",
                config_path.display(),
                backup.display()
            )
        })?;
        println!("herdr-docs: backed up config to {}", backup.display());
    }

    write_config_atomically(&config_path, &append_binding(&existing))
        .map_err(|error| format!("could not write {}: {error}", config_path.display()))?;

    match reload_config() {
        Ok(true) => println!("herdr-docs: bound {KEYBINDING} to {ACTION}"),
        Ok(false) => println!(
            "herdr-docs: wrote {KEYBINDING} to {}; reload Herdr config to activate it",
            config_path.display()
        ),
        Err(error) => println!(
            "herdr-docs: wrote {KEYBINDING} to {}; could not reload Herdr: {error}",
            config_path.display()
        ),
    }
    record_marker(marker.as_deref());
    Ok(())
}

fn config_path() -> Option<PathBuf> {
    if let Some(path) = non_empty_env_path("HERDR_CONFIG_PATH") {
        return Some(path);
    }

    if let Some(socket) = env::var_os("HERDR_SOCKET_PATH") {
        let socket = PathBuf::from(socket);
        if let Some(parent) = socket.parent() {
            return Some(parent.join("config.toml"));
        }
    }

    let config_home = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(config_home.join("herdr/config.toml"))
}

fn setup_marker() -> Option<PathBuf> {
    ["HERDR_PLUGIN_STATE_DIR", "HERDR_PLUGIN_CONFIG_DIR"]
        .iter()
        .find_map(|name| non_empty_env_path(name))
        .map(|directory| directory.join(MARKER_NAME))
}

fn non_empty_env_path(name: &str) -> Option<PathBuf> {
    env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn record_marker(marker: Option<&Path>) {
    let Some(marker) = marker else {
        return;
    };
    let result = (|| -> io::Result<()> {
        if let Some(parent) = marker.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(marker, MARKER_CONTENT)
    })();
    if let Err(error) = result {
        println!(
            "herdr-docs: could not record setup state at {}: {error}",
            marker.display()
        );
    }
}

fn assignment_value<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let (left, right) = line.split_once('=')?;
    if left.trim() != name {
        return None;
    }
    let right = right.trim().strip_prefix('"')?;
    right.split_once('"').map(|(value, _)| value)
}

fn has_action_binding(config: &str) -> bool {
    config
        .lines()
        .any(|line| assignment_value(line, "command") == Some(ACTION))
}

fn bound_keys(config: &str) -> Vec<String> {
    config
        .lines()
        .filter_map(|line| assignment_value(line, "key"))
        .map(str::to_string)
        .collect()
}

fn append_binding(existing: &str) -> String {
    let mut body = existing.to_string();
    if !body.is_empty() && !body.ends_with('\n') {
        body.push('\n');
    }
    body.push_str(&format!(
        "\n# added by herdr-docs\n[[keys.command]]\nkey = \"{KEYBINDING}\"\ntype = \"plugin_action\"\ncommand = \"{ACTION}\"\ndescription = \"open document reader\"\n"
    ));
    body
}

fn write_config_atomically(path: &Path, body: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("config.toml");
    let temporary = path.with_file_name(format!(
        ".{file_name}.herdr-docs-{}.tmp",
        std::process::id()
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(body.as_bytes())?;
        file.sync_all()?;
        if let Ok(metadata) = fs::metadata(path) {
            fs::set_permissions(&temporary, metadata.permissions())?;
        }
        fs::rename(&temporary, path)
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn reload_config() -> io::Result<bool> {
    let binary = env::var("HERDR_BIN_PATH").unwrap_or_else(|_| "herdr".to_string());
    let output = Command::new(binary)
        .args(["server", "reload-config"])
        .output()?;
    Ok(output.status.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bound_keys_ignore_comments_and_other_assignments() {
        let config = r#"
# key = "prefix+d"
[keys]
prefix = "ctrl+b"
[[keys.command]]
key = "prefix+shift+l"
"#;
        assert_eq!(bound_keys(config), vec!["prefix+shift+l"]);
    }

    #[test]
    fn action_binding_accepts_spacing_around_equals() {
        assert!(has_action_binding("command   =   \"herdr-docs.open\""));
        assert!(!has_action_binding("# command = \"herdr-docs.open\""));
    }

    #[test]
    fn append_binding_preserves_existing_config() {
        let result = append_binding("onboarding = false\n");
        assert!(result.starts_with("onboarding = false\n"));
        assert!(result.contains("key = \"prefix+d\""));
        assert!(result.contains("command = \"herdr-docs.open\""));
    }
}
