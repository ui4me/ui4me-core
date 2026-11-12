//! Application manifests are data, not shell commands. Paths relative to the
//! manifest resolve against its directory. The sole explicit substitution is
//! `{app_dir}`, usable in program, arguments, cwd, environment values and icon.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

const MAX_MANIFEST_BYTES: u64 = 256 * 1024;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    pub app: AppInfo,
    pub backend: BackendConfig,
    #[serde(default)]
    pub host: HostConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AppInfo {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub icon: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BackendConfig {
    pub program: PathBuf,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub cwd: PathBuf,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InstancePolicy {
    #[default]
    Single,
    Multiple,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct HostConfig {
    pub startup_timeout_ms: u64,
    pub shutdown_timeout_ms: u64,
    pub max_windows: usize,
    pub instance: InstancePolicy,
}

impl Default for HostConfig {
    fn default() -> Self {
        Self {
            startup_timeout_ms: 15_000,
            shutdown_timeout_ms: 5_000,
            max_windows: 16,
            instance: InstancePolicy::Single,
        }
    }
}

impl AppConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let manifest = path
            .canonicalize()
            .with_context(|| format!("Cannot locate application manifest {}", path.display()))?;
        let metadata =
            fs::metadata(&manifest).context("Cannot read application manifest metadata")?;
        if !metadata.is_file() || metadata.len() > MAX_MANIFEST_BYTES {
            bail!(
                "Application manifest must be a regular file of at most {MAX_MANIFEST_BYTES} bytes"
            );
        }
        let text = fs::read_to_string(&manifest)
            .context("Application manifest must contain UTF-8 TOML")?;
        let base = manifest
            .parent()
            .context("Application manifest has no parent directory")?;
        Self::from_toml(&text, base)
    }

    /// Parse against an explicit directory. Useful to tooling that constructs a
    /// manifest in memory; production launch normally uses `load`.
    pub fn from_toml(text: &str, app_dir: &Path) -> Result<Self> {
        if text.len() > MAX_MANIFEST_BYTES as usize {
            bail!("Application manifest exceeds its size limit");
        }
        let mut config: Self = toml::from_str(text).context("Invalid application manifest")?;
        config.resolve_and_validate(app_dir)?;
        Ok(config)
    }

    fn resolve_and_validate(&mut self, app_dir: &Path) -> Result<()> {
        validate_app_id(&self.app.id)?;
        validate_text("app.name", &self.app.name, 128)?;
        if let Some(version) = &self.app.version {
            validate_text("app.version", version, 128)?;
        }
        if !(1_000..=300_000).contains(&self.host.startup_timeout_ms) {
            bail!("host.startup_timeout_ms must be between 1000 and 300000");
        }
        if !(100..=120_000).contains(&self.host.shutdown_timeout_ms) {
            bail!("host.shutdown_timeout_ms must be between 100 and 120000");
        }
        if !(1..=64).contains(&self.host.max_windows) {
            bail!("host.max_windows must be between 1 and 64");
        }
        let base = app_dir.canonicalize().with_context(|| {
            format!(
                "Application directory does not exist: {}",
                app_dir.display()
            )
        })?;
        if !base.is_dir() {
            bail!("Application directory is not a directory");
        }
        let base_text = base
            .to_str()
            .context("Application directory must be representable as Unicode")?;
        // Windows canonicalization yields a verbatim \\?\ path, in which
        // `/` is not a separator: "{app_dir}/icon.png" was an invalid path.
        let base_text = &without_verbatim_prefix(base_text);

        let program = self
            .backend
            .program
            .to_str()
            .context("backend.program must be Unicode")?;
        if program.is_empty() || program.contains('\0') {
            bail!("backend.program must be a nonempty executable path or PATH name");
        }
        let program = expand(program, base_text);
        let has_separator = program.contains('/') || program.contains('\\');
        let program_path = PathBuf::from(&program);
        self.backend.program = if program_path.is_absolute() || has_separator {
            let candidate = absolutize(&base, &program_path);
            let resolved = candidate.canonicalize().with_context(|| {
                format!("Backend executable does not exist: {}", candidate.display())
            })?;
            if !resolved.is_file() {
                bail!("backend.program is not a file");
            }
            resolved
        } else {
            // Command resolves a single component through PATH. Do not join it
            // with the manifest directory, or a normal `python3` would break.
            program_path
        };

        let cwd = self
            .backend
            .cwd
            .to_str()
            .context("backend.cwd must be Unicode")?;
        if cwd.contains('\0') {
            bail!("backend.cwd contains a NUL character");
        }
        let cwd = expand(cwd, base_text);
        self.backend.cwd = absolutize(&base, Path::new(&cwd))
            .canonicalize()
            .context("backend.cwd does not exist")?;
        if !self.backend.cwd.is_dir() {
            bail!("backend.cwd is not a directory");
        }
        if self.backend.args.len() > 256 {
            bail!("backend.args has more than 256 arguments");
        }
        for argument in &mut self.backend.args {
            if argument.contains('\0') {
                bail!("Backend arguments must not contain NUL characters");
            }
            *argument = expand(argument, base_text);
            if argument.len() > 32_768 {
                bail!("A backend argument exceeds 32768 UTF-8 bytes");
            }
        }
        if self.backend.env.len() > 128 {
            bail!("backend.env has more than 128 entries");
        }
        for (key, value) in &mut self.backend.env {
            if key.is_empty() || key.contains('=') || key.contains('\0') {
                bail!("Invalid backend environment variable name");
            }
            if reserved_env(key) {
                bail!("backend.env cannot override host transport variables");
            }
            if value.contains('\0') {
                bail!("Environment values must not contain NUL characters");
            }
            *value = expand(value, base_text);
            if key.len() > 256 || value.len() > 32_768 {
                bail!("Backend environment entry exceeds size limits");
            }
        }
        // Keep the aggregate launch data bounded as well as each individual field.
        let launch_bytes = self.backend.args.iter().map(String::len).sum::<usize>()
            + self
                .backend
                .env
                .iter()
                .map(|(key, value)| key.len() + value.len())
                .sum::<usize>();
        if launch_bytes > 128 * 1024 {
            bail!("Backend arguments and environment exceed 128 KiB");
        }

        if let Some(icon) = &self.app.icon {
            let text = icon.to_str().context("app.icon must be Unicode")?;
            if text.contains('\0') || text.is_empty() {
                bail!("app.icon must be a nonempty path");
            }
            let resolved = absolutize(&base, Path::new(&expand(text, base_text)))
                .canonicalize()
                .context("app.icon does not exist")?;
            if !resolved.is_file() {
                bail!("app.icon is not a file");
            }
            self.app.icon = Some(resolved);
        }
        Ok(())
    }
}

/// The portable identity subset is suitable for desktop-file identities and
/// Windows AppUserModelIDs. Session-specific arbitration uses a separate key.
pub fn validate_app_id(id: &str) -> Result<()> {
    if id.is_empty() || id.len() > 128 || !id.is_ascii() || !id.contains('.') {
        bail!("app.id must be a dot-separated ASCII identity of at most 128 bytes");
    }
    for part in id.split('.') {
        let mut bytes = part.bytes();
        if !bytes.next().is_some_and(|byte| byte.is_ascii_alphabetic())
            || !bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        {
            bail!(
                "Each app.id component must begin with a letter and contain letters, digits, '_' or '-'"
            );
        }
    }
    Ok(())
}

fn validate_text(field: &str, text: &str, max_chars: usize) -> Result<()> {
    if text.trim().is_empty()
        || text.chars().count() > max_chars
        || text.chars().any(char::is_control)
    {
        bail!(
            "{field} must be nonempty, at most {max_chars} characters, and contain no control characters"
        );
    }
    Ok(())
}

fn reserved_env(key: &str) -> bool {
    // Case-insensitive because Windows environment names are case-insensitive.
    let key = key.to_ascii_uppercase();
    key.starts_with("UI4ME_HOST_")
        || matches!(
            key.as_str(),
            "UI4ME_ENDPOINT" | "UI4ME_TOKEN" | "UI4ME_APP_ID" | "UI4ME_PROTOCOL_VERSION"
        )
}

/// `\\?\C:\dir` -> `C:\dir`; `\\?\UNC\server\share` -> `\\server\share`.
fn without_verbatim_prefix(path: &str) -> String {
    if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = path
        .strip_prefix(r"\\?\")
        .filter(|rest| rest.as_bytes().get(1) == Some(&b':'))
    {
        rest.to_owned()
    } else {
        path.to_owned()
    }
}

fn expand(text: &str, base: &str) -> String {
    text.replace("{app_dir}", base)
}
fn absolutize(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbatim_windows_prefix_is_removed_for_substitution() {
        assert_eq!(
            without_verbatim_prefix(r"\\?\C:\Apps\Demo"),
            r"C:\Apps\Demo"
        );
        assert_eq!(
            without_verbatim_prefix(r"\\?\UNC\server\share\app"),
            r"\\server\share\app"
        );
        assert_eq!(without_verbatim_prefix("/home/user/app"), "/home/user/app");
        assert_eq!(
            without_verbatim_prefix(r"\\?\GLOBALROOT\x"),
            r"\\?\GLOBALROOT\x"
        );
    }

    #[test]
    fn app_dir_paths_with_forward_slashes_resolve_on_every_platform() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("icon.png"), b"png").unwrap();
        let config = AppConfig::from_toml(
            "[app]\nid='org.ui4me.Probe'\nname='Probe'\nicon='{app_dir}/icon.png'\n[backend]\nprogram='python3'\nargs=['{app_dir}/backend.py']\n",
            directory.path(),
        )
        .unwrap();
        assert!(config.app.icon.unwrap().is_file());
        let argument = &config.backend.args[0];
        assert!(!argument.starts_with(r"\\?\"), "{argument}");
        assert!(argument.ends_with("backend.py"));
    }

    fn manifest() -> &'static str {
        "[app]\nid='org.ui4me.Probe'\nname='Host probe'\n[backend]\nprogram='python3'\nargs=['{app_dir}/backend.py', 'literal $HOME $(ignored)']\n"
    }

    #[test]
    fn paths_resolve_without_shell_expansion() {
        let base = std::env::temp_dir().canonicalize().unwrap();
        let config = AppConfig::from_toml(manifest(), &base).unwrap();
        assert_eq!(config.backend.program, PathBuf::from("python3"));
        assert_eq!(config.backend.cwd, base);
        assert!(
            config.backend.args[0].starts_with(&without_verbatim_prefix(base.to_str().unwrap()))
        );
        assert_eq!(config.backend.args[1], "literal $HOME $(ignored)");
        assert_eq!(config.host.max_windows, 16);
        assert_eq!(config.host.instance, InstancePolicy::Single);
    }

    #[test]
    fn invalid_identity_and_unknown_fields_fail_early() {
        let base = std::env::temp_dir();
        for id in [
            "",
            "single",
            "org..app",
            "org.1app",
            "../app",
            "org.app/name",
            "org.český",
        ] {
            assert!(validate_app_id(id).is_err(), "accepted {id}");
        }
        assert!(AppConfig::from_toml(&format!("{}\nunknown=true", manifest()), &base).is_err());
        assert!(
            AppConfig::from_toml(&format!("{}\n[host]\nmax_windows=0", manifest()), &base).is_err()
        );
    }

    #[test]
    fn missing_working_directory_is_not_silently_replaced() {
        let text = format!("{}\ncwd='ui4me-nonexistent-cwd-925c1330'", manifest());
        assert!(AppConfig::from_toml(&text, &std::env::temp_dir()).is_err());
    }

    #[test]
    fn transport_environment_cannot_be_overridden() {
        let text = format!("{}\n[backend.env]\nui4me_host_token='bad'", manifest());
        assert!(AppConfig::from_toml(&text, &std::env::temp_dir()).is_err());
    }

    #[test]
    fn relative_executable_is_validated_and_absolutized() {
        // Use this test executable as a known-existing program without relying
        // on Python, a shell, or PATH to exist on the test runner.
        let executable = std::env::current_exe().unwrap().canonicalize().unwrap();
        let parent = executable.parent().unwrap();
        let relative = format!("./{}", executable.file_name().unwrap().to_str().unwrap());
        let text = manifest().replace("program='python3'", &format!("program='{relative}'"));
        let config = AppConfig::from_toml(&text, parent).unwrap();
        assert_eq!(config.backend.program, executable);
    }
}
