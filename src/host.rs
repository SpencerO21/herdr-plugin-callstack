use anyhow::{Context, Result, bail, ensure};
use serde_json::Value;
use std::{
    env,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
pub struct Source {
    pub project: PathBuf,
    pub file: PathBuf,
    pub line: u64,
}

pub fn source_location(project: &Path, location: &str) -> Result<Source> {
    ensure!(!location.is_empty(), "This call has no source location.");
    ensure!(
        !location.chars().any(char::is_control),
        "The source path contains a control character."
    );
    let mut path = location;
    let mut line = 1;
    if let Some((left, last)) = location.rsplit_once(':')
        && !last.is_empty()
        && last.bytes().all(|b| b.is_ascii_digit())
    {
        line = last.parse().context("Invalid source line.")?;
        path = left;
        if let Some((file, number)) = left.rsplit_once(':')
            && !number.is_empty()
            && number.bytes().all(|b| b.is_ascii_digit())
        {
            line = number.parse().context("Invalid source line.")?;
            path = file;
        }
    }
    ensure!(line > 0 && line <= i32::MAX as u64, "Invalid source line.");
    let project = project
        .canonicalize()
        .context("Cannot read the project folder. Use --project PATH.")?;
    let file = project
        .join(path)
        .canonicalize()
        .with_context(|| format!("Cannot read source file: {path}"))?;
    ensure!(
        file.starts_with(&project),
        "The source file is outside the project folder."
    );
    ensure!(file.is_file(), "The source path is not a file.");
    Ok(Source {
        project,
        file,
        line,
    })
}

// Drain both pipes while waiting, so a full output pipe cannot deadlock the child.
// Cap retained output without stopping the drain. The caller runs on a worker.
fn drain(mut pipe: impl Read) -> std::io::Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut buffer = [0; 8192];
    loop {
        let n = pipe.read(&mut buffer)?;
        if n == 0 {
            return Ok(out);
        }
        let keep = n.min(1_000_000usize.saturating_sub(out.len()));
        out.extend_from_slice(&buffer[..keep]);
    }
}

pub fn run_json(binary: &str, args: &[String], timeout: Duration) -> Result<Value> {
    let mut child = Command::new(binary)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("Cannot start {binary}"))?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let out = thread::spawn(move || drain(stdout));
    let err = thread::spawn(move || drain(stderr));
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            bail!("The Herdr command timed out. Check the pane before trying again.");
        }
        thread::sleep(Duration::from_millis(20));
    };
    let output = out
        .join()
        .map_err(|_| anyhow::anyhow!("Output reader failed."))??;
    let errors = err
        .join()
        .map_err(|_| anyhow::anyhow!("Error reader failed."))??;
    ensure!(
        status.success(),
        "Herdr command failed: {}",
        String::from_utf8_lossy(&errors)
    );
    let value: Value = serde_json::from_slice(&output).context("Invalid Herdr response.")?;
    ensure!(
        value.get("error").is_none(),
        "Herdr error: {}",
        value["error"]
    );
    value
        .get("result")
        .cloned()
        .context("The Herdr response has no result.")
}

pub fn herdr(args: &[String]) -> Result<Value> {
    run_json(
        &env::var("HERDR_BIN_PATH").unwrap_or_else(|_| "herdr".into()),
        args,
        Duration::from_secs(10),
    )
}

pub fn open_source(project: &Path, location: &str) -> Result<()> {
    let source = source_location(project, location)?;
    let target =
        env::var("HERDR_PANE_ID").context("Open the tree in a Herdr pane to use Neovim.")?;
    let mut args: Vec<String> = [
        "plugin",
        "pane",
        "open",
        "--plugin",
        "callstack",
        "--entrypoint",
        "editor",
        "--target-pane",
        &target,
        "--direction",
        "right",
        "--focus",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    for (key, value) in [
        (
            "CALLSTACK_PROJECT",
            source.project.to_string_lossy().into_owned(),
        ),
        ("CALLSTACK_FILE", source.file.to_string_lossy().into_owned()),
        ("CALLSTACK_LINE", source.line.to_string()),
    ] {
        args.extend(["--env".into(), format!("{key}={value}")]);
    }
    herdr(&args)?;
    Ok(())
}

pub fn edit_source() -> Result<()> {
    use std::os::unix::process::CommandExt;
    let project = env::var("CALLSTACK_PROJECT").context("Missing project folder.")?;
    let file = env::var("CALLSTACK_FILE").context("Missing source file.")?;
    let line = env::var("CALLSTACK_LINE").unwrap_or_else(|_| "1".into());
    let source = source_location(Path::new(&project), &format!("{file}:{line}"))?;
    // Replace this process. No idle wrapper remains beside Neovim.
    let error = Command::new("nvim")
        .arg(format!("+{}", source.line))
        .arg("--")
        .arg(source.file)
        .current_dir(source.project)
        .exec();
    Err(error).context("Cannot start Neovim. Check that nvim is on PATH.")
}
