use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT_FIXTURE_ID: AtomicUsize = AtomicUsize::new(0);

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_ludusavi-cli")
}

fn workspace_path(parts: &[&str]) -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for part in parts {
        path.push(part);
    }
    path
}

fn contract_root() -> PathBuf {
    workspace_path(&[
        "target",
        "cli-only-contract",
        &format!("{}", std::process::id()),
        &format!("{}", NEXT_FIXTURE_ID.fetch_add(1, Ordering::Relaxed)),
    ])
}

fn path_arg(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn run(args: &[&str]) -> Output {
    Command::new(binary())
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap_or_else(|error| panic!("failed to run ludusavi-cli with args {args:?}: {error}"))
}

fn assert_success(output: Output, args: &[&str]) -> String {
    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be UTF-8");

    assert!(
        output.status.success(),
        "command failed: {args:?}\nstatus: {}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status,
    );

    stdout
}

fn assert_json_stdout(output: Output, args: &[&str]) -> serde_json::Value {
    let stdout = assert_success(output, args);
    serde_json::from_str(&stdout).unwrap_or_else(|error| {
        panic!("stdout was not valid JSON for {args:?}: {error}\nstdout:\n{stdout}")
    })
}

struct IsolatedCli {
    root: PathBuf,
    config_dir: PathBuf,
    backup_dir: PathBuf,
    live_dir: PathBuf,
}

impl IsolatedCli {
    fn new() -> Self {
        let root = contract_root();
        let _ = fs::remove_dir_all(&root);

        let config_dir = root.join("config");
        let backup_dir = root.join("backup");
        let live_dir = root.join("live").join("game1");

        fs::create_dir_all(&config_dir).unwrap();
        fs::create_dir_all(&backup_dir).unwrap();
        fs::create_dir_all(live_dir.join("subdir")).unwrap();
        fs::write(live_dir.join("file1.txt"), "1").unwrap();
        fs::write(live_dir.join("subdir").join("file2.txt"), "22").unwrap();

        let config = format!(
            r#"
manifest:
  enable: false
roots: []
backup:
  path: "{backup}"
restore:
  path: "{backup}"
customGames:
  - name: game1
    files:
      - "{file1}"
      - "{subdir}"
"#,
            backup = path_arg(&backup_dir),
            file1 = path_arg(&live_dir.join("file1.txt")),
            subdir = path_arg(&live_dir.join("subdir")),
        );
        fs::write(config_dir.join("config.yaml"), config).unwrap();

        Self {
            root,
            config_dir,
            backup_dir,
            live_dir,
        }
    }

    fn config_arg(&self) -> String {
        path_arg(&self.config_dir)
    }

    fn backup_arg(&self) -> String {
        path_arg(&self.backup_dir)
    }

    fn reset_live_data_for_restore(&self) {
        fs::remove_file(self.live_dir.join("file1.txt")).unwrap();
        fs::remove_dir_all(self.live_dir.join("subdir")).unwrap();
    }
}

impl Drop for IsolatedCli {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn core_help_commands_execute() {
    for args in [
        vec!["--help"],
        vec!["backup", "--help"],
        vec!["restore", "--help"],
        vec!["manifest", "--help"],
        vec!["schema", "--help"],
    ] {
        assert_success(run(&args), &args);
    }
}

#[test]
fn schema_commands_emit_json() {
    for args in [
        vec!["schema", "api-input"],
        vec!["schema", "api-output"],
        vec!["schema", "general-output"],
        vec!["schema", "config"],
    ] {
        assert_json_stdout(run(&args), &args);
    }
}

#[test]
fn manifest_commands_use_isolated_config() {
    let isolated = IsolatedCli::new();
    let config = isolated.config_arg();

    let show_args = ["--config", config.as_str(), "manifest", "show", "--api"];
    assert_json_stdout(run(&show_args), &show_args);

    let update_args = ["--config", config.as_str(), "manifest", "update"];
    assert_success(run(&update_args), &update_args);

    assert!(isolated.config_dir.join("config.yaml").exists());
}

#[test]
fn backup_and_restore_api_stdout_is_json() {
    let isolated = IsolatedCli::new();
    let config = isolated.config_arg();
    let backup = isolated.backup_arg();

    let backup_preview = [
        "--config",
        config.as_str(),
        "backup",
        "--api",
        "--preview",
        "--path",
        backup.as_str(),
        "game1",
    ];
    assert_json_stdout(run(&backup_preview), &backup_preview);

    let backup_final = [
        "--config",
        config.as_str(),
        "backup",
        "--api",
        "--force",
        "--path",
        backup.as_str(),
        "game1",
    ];
    assert_json_stdout(run(&backup_final), &backup_final);

    isolated.reset_live_data_for_restore();

    let restore_preview = [
        "--config",
        config.as_str(),
        "restore",
        "--api",
        "--preview",
        "--path",
        backup.as_str(),
        "game1",
    ];
    assert_json_stdout(run(&restore_preview), &restore_preview);

    let restore_final = [
        "--config",
        config.as_str(),
        "restore",
        "--api",
        "--force",
        "--path",
        backup.as_str(),
        "game1",
    ];
    assert_json_stdout(run(&restore_final), &restore_final);

    assert!(isolated.live_dir.join("file1.txt").exists());
    assert!(isolated.live_dir.join("subdir").join("file2.txt").exists());
}
