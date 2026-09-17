//! Launch an explicit recorded session without shell interpolation or parent-env mutation.
use anyhow::{Context, Result, bail};
use std::{
    env,
    path::{Path, PathBuf},
    process::{Command, ExitStatus},
};

#[derive(Clone)]
pub struct Settings {
    pub program: PathBuf,
    pub cwd: Option<PathBuf>,
}
pub struct Plan {
    pub id: String,
    pub home: PathBuf,
    pub profile: String,
}

pub struct Ready {
    pub plan: Plan,
    pub program: PathBuf,
    pub cwd: PathBuf,
}

impl Plan {
    pub fn new(id: &str, history: &Path) -> Result<Self> {
        // history.jsonl records UUIDs, not shell expressions or CLI flags.
        let valid = id.len() == 36
            && id.bytes().enumerate().all(|(index, byte)| {
                if [8, 13, 18, 23].contains(&index) {
                    byte == b'-'
                } else {
                    byte.is_ascii_hexdigit()
                }
            });
        if !valid {
            bail!("Cannot resume: the recorded session ID is not a UUID");
        }
        let parent = history
            .parent()
            .context("History source has no parent directory")?;
        let home = parent
            .canonicalize()
            .map(native_path)
            .with_context(|| format!("Cannot access CODEX_HOME {}", parent.display()))?;
        if !home.is_dir() {
            bail!("CODEX_HOME is not a directory: {}", home.display());
        }
        Ok(Self {
            id: id.to_owned(),
            profile: crate::history::source_name(history),
            home,
        })
    }

    pub fn command(&self, program: &Path, cwd: &Path) -> Command {
        let mut command = Command::new(program);
        command.arg("resume");
        // Keep filesystem paths out of command arguments for Windows npm
        // .cmd shims. The working directory is passed through the OS API.
        command.current_dir(cwd).args(["--cd", "."]);
        command
            .arg("--")
            .arg(&self.id)
            .env("CODEX_HOME", &self.home);
        command
    }
}

impl Settings {
    pub fn prepare(
        &self,
        plan: Plan,
        directory: &Path,
        session_path: Option<&Path>,
        progress: &mut crate::loader::Progress<'_>,
    ) -> Result<Ready> {
        progress("Locating project directory", 0)?;
        let recorded = if self.cwd.is_none() {
            let path = match session_path {
                Some(path) => path.to_path_buf(),
                None => crate::session::find_with(directory, &plan.id, progress)?,
            };
            Some(project_directory(&path, &plan.id)?)
        } else {
            None
        };
        progress("Checking resume directory and executable", 0)?;
        let (program, cwd) = self.resolve(recorded.as_deref())?;
        progress("Ready to resume in project directory", 1)?;
        Ok(Ready { plan, program, cwd })
    }

    fn resolve(&self, recorded: Option<&Path>) -> Result<(PathBuf, PathBuf)> {
        let program = resolve_program(&self.program)?;
        let directory = self
            .cwd
            .as_deref()
            .or(recorded)
            .context("Session has no project directory; use --resume-cwd PATH")?;
        if self.cwd.is_none() && !directory.is_absolute() {
            bail!("Recorded project path is not absolute on this platform; use --resume-cwd PATH");
        }
        let cwd = directory.canonicalize().map(native_path).with_context(|| {
            format!(
                "Cannot access project directory {}; use --resume-cwd PATH if it moved",
                directory.display()
            )
        })?;
        if !cwd.is_dir() {
            bail!(
                "Project path is not a directory: {}; use --resume-cwd PATH",
                cwd.display()
            );
        }
        Ok((program, cwd))
    }
}

fn project_directory(path: &Path, id: &str) -> Result<PathBuf> {
    use std::io::BufRead;
    let mut line = String::new();
    std::io::BufReader::new(
        std::fs::File::open(path)
            .with_context(|| format!("Cannot open session metadata {}", path.display()))?,
    )
    .read_line(&mut line)?;
    let metadata: serde_json::Value =
        serde_json::from_str(&line).context("Invalid session metadata; use --resume-cwd PATH")?;
    let payload = &metadata["payload"];
    if metadata["type"] != "session_meta" || !(payload["id"] == id || payload["session_id"] == id) {
        bail!("Session metadata does not match the selected session");
    }
    let cwd = payload["cwd"]
        .as_str()
        .filter(|path| !path.is_empty())
        .context("Session has no recorded project directory; use --resume-cwd PATH")?;
    Ok(PathBuf::from(cwd))
}

fn resolve_program(program: &Path) -> Result<PathBuf> {
    #[cfg(windows)]
    if program
        .extension()
        .is_some_and(|ext| ext.to_string_lossy().eq_ignore_ascii_case("ps1"))
    {
        bail!(
            "Use codex.exe or codex.cmd with --codex-bin; PowerShell functions and .ps1 scripts are not executable launchers"
        );
    }
    let candidates = |base: PathBuf| -> Vec<PathBuf> {
        #[cfg(windows)]
        if base.extension().is_none() {
            return vec![
                base.with_extension("exe"),
                base.with_extension("cmd"),
                base.with_extension("bat"),
            ];
        }
        vec![base]
    };
    let locations = if program.is_absolute() || program.components().count() > 1 {
        candidates(program.to_path_buf())
    } else {
        env::split_paths(&env::var_os("PATH").unwrap_or_default())
            .flat_map(|directory| candidates(directory.join(program)))
            .collect()
    };
    for path in locations {
        if path.is_file() {
            return path
                .canonicalize()
                .map(native_path)
                .with_context(|| format!("Cannot resolve {}", path.display()));
        }
    }
    bail!(
        "Codex executable not found: {}. Install Codex or provide --codex-bin PATH",
        program.display()
    )
}

// Windows canonicalize uses a verbatim prefix, which cmd.exe npm shims do not
// consistently understand. Preserve the actual path while removing that prefix.
fn native_path(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        let mut components = path.components();
        let prefix = match components.next() {
            Some(Component::Prefix(prefix)) => prefix.kind(),
            _ => return path,
        };
        let mut native = match prefix {
            Prefix::VerbatimDisk(drive) => PathBuf::from(format!("{}:\\", drive as char)),
            Prefix::VerbatimUNC(server, share) => {
                let mut root = std::ffi::OsString::from("\\\\");
                root.push(server);
                root.push("\\");
                root.push(share);
                PathBuf::from(root)
            }
            _ => return path,
        };
        for component in components {
            if let Component::Normal(part) = component {
                native.push(part);
            }
        }
        native
    }
    #[cfg(not(windows))]
    path
}

pub fn execute(plan: &Plan, program: &Path, cwd: &Path) -> Result<ExitStatus> {
    plan.command(program, cwd)
        .status()
        .with_context(|| format!("Could not start {}", program.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    const ID: &str = "019feeaf-b9a2-7773-8776-269af6ca57c4";
    #[test]
    fn preparation_uses_recorded_project_and_override_wins() {
        let base = env::temp_dir().join(format!(
            "codex-project-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let sessions = base.join("sessions");
        let project = base.join("project & spaces");
        std::fs::create_dir_all(&sessions).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        let path = sessions.join("session.jsonl");
        let write = |cwd: Option<&Path>| {
            std::fs::write(
                &path,
                serde_json::json!({"type":"session_meta", "payload":{"id":ID, "cwd":cwd}})
                    .to_string(),
            )
            .unwrap()
        };
        write(Some(&project));
        let settings = Settings {
            program: env::current_exe().unwrap(),
            cwd: None,
        };
        let plan = || Plan::new(ID, &base.join("history.jsonl")).unwrap();
        let ready = settings
            .prepare(plan(), &sessions, None, &mut |_, _| Ok(()))
            .unwrap();
        assert_eq!(ready.cwd, native_path(project.canonicalize().unwrap()));
        write(None);
        assert!(
            settings
                .prepare(plan(), &sessions, Some(&path), &mut |_, _| Ok(()))
                .is_err()
        );
        let override_settings = Settings {
            program: env::current_exe().unwrap(),
            cwd: Some(project.clone()),
        };
        assert!(
            override_settings
                .prepare(plan(), &sessions, Some(&path), &mut |_, _| Ok(()))
                .is_ok()
        );
        write(Some(&base.join("missing")));
        assert!(
            settings
                .prepare(plan(), &sessions, Some(&path), &mut |_, _| Ok(()))
                .is_err()
        );
        std::fs::remove_dir_all(base).unwrap();
    }
    #[cfg(windows)]
    #[test]
    fn windows_shim_paths_do_not_keep_verbatim_prefixes() {
        assert_eq!(
            native_path(PathBuf::from(r"\\?\C:\tools\codex.cmd")),
            PathBuf::from(r"C:\tools\codex.cmd")
        );
        assert_eq!(
            native_path(PathBuf::from(r"\\?\UNC\host\share\codex.cmd")),
            PathBuf::from(r"\\host\share\codex.cmd")
        );
    }
    #[test]
    fn sources_set_only_child_home_and_keep_uuid_and_cwd_as_separate_arguments() {
        let base = env::temp_dir().join(format!(
            "codex-resume-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let before = env::var_os("CODEX_HOME");
        for (directory, profile) in [
            (".codex", ".codex"),
            (".codex-beta", ".codex-beta"),
            (".codex-gamma", ".codex-gamma"),
        ] {
            let home = base.join(directory);
            std::fs::create_dir_all(&home).unwrap();
            let plan = Plan::new(ID, &home.join("history.jsonl")).unwrap();
            assert_eq!(plan.profile, profile);
            let command = plan.command(Path::new("codex"), &base);
            assert_eq!(
                command.get_args().collect::<Vec<_>>(),
                vec!["resume", "--cd", ".", "--", ID]
            );
            assert_eq!(command.get_current_dir(), Some(base.as_path()));
            assert!(command.get_envs().any(|(key, value)| key == "CODEX_HOME" && value == Some(plan.home.as_os_str())));
        }
        assert_eq!(env::var_os("CODEX_HOME"), before);
        assert!(Plan::new("--last", &base.join("history.jsonl")).is_err());
        assert!(Plan::new("id & echo injected", &base.join("history.jsonl")).is_err());
        assert!(
            Settings {
                program: base.join("missing-codex"),
                cwd: None
            }
            .resolve(None)
            .is_err()
        );
        std::fs::remove_dir_all(base).unwrap();
    }
}
