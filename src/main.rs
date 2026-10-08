use anyhow::{Context, Result, ensure};
use clap::{CommandFactory, Parser, Subcommand};
use herdr_callstack::{
    host,
    model::{Flow, Scope, clean, plain},
    runtime,
    store::{INPUT_LIMIT, Store, read_limited},
};
use std::{
    env,
    fs::File,
    io::{self, IsTerminal},
    path::PathBuf,
};

#[derive(Parser)]
#[command(version, about = "Mouse-controlled call flows for Herdr")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
    #[arg(long, global = true, env = "CALLSTACK_WORKSPACE")]
    workspace: Option<String>,
    #[arg(
        long,
        global = true,
        env = "CALLSTACK_SESSION",
        default_value = "default"
    )]
    session: String,
    #[arg(long, global = true, env = "CALLSTACK_NAMESPACE")]
    namespace: Option<String>,
    #[arg(long, global = true, env = "CALLSTACK_STATE_DIR")]
    state_dir: Option<PathBuf>,
    #[arg(long, global = true, env = "CALLSTACK_PROJECT")]
    project: Option<PathBuf>,
}
#[derive(Subcommand)]
enum Commands {
    /// Save a flow from a JSON file. Use - for standard input.
    Publish { file: String },
    /// List saved flows in this workspace and session.
    List {
        #[arg(long)]
        json: bool,
    },
    /// Print a flow.
    Show {
        name: String,
        #[arg(long)]
        json: bool,
    },
    /// Remove one flow. No undo is available.
    Delete { name: String },
    /// Run the terminal view. Mouse and keyboard controls are available.
    View {
        #[arg(long)]
        once: bool,
    },
    /// Open the terminal view beside the caller pane.
    Open,
    #[command(hide = true)]
    Edit,
}

fn run(cli: Cli) -> Result<()> {
    let Some(command) = cli.command else {
        Cli::command().print_help()?;
        println!();
        return Ok(());
    };
    if matches!(command, Commands::Edit) {
        return host::edit_source();
    }
    let scope = Scope {
        namespace: cli
            .namespace
            .or_else(|| env::var("HERDR_SOCKET_PATH").ok())
            .unwrap_or_else(|| "local".into()),
        workspace: cli
            .workspace
            .or_else(|| env::var("HERDR_WORKSPACE_ID").ok())
            .context("Set --workspace NAME outside a Herdr pane.")?,
        session: cli.session,
    };
    let root = cli.state_dir.unwrap_or_else(|| {
        env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(env::var_os("HOME").unwrap_or_default()).join(".local/state")
            })
            .join("herdr-callstack")
    });
    ensure!(root.is_absolute(), "Use an absolute state directory.");
    let store = Store::new(&root, scope.clone())?;
    match command {
        Commands::Publish { file } => {
            let data = if file == "-" {
                read_limited(io::stdin().lock(), INPUT_LIMIT)?
            } else {
                read_limited(File::open(file)?, INPUT_LIMIT)?
            };
            let flow: Flow = serde_json::from_slice(&data).context("Invalid flow JSON.")?;
            let project = cli
                .project
                .unwrap_or(env::current_dir()?)
                .canonicalize()
                .context("Cannot read the project folder.")?;
            let record = store.publish(flow, Some(project.to_string_lossy().into_owned()))?;
            println!("Published: {}", clean(&record.flow.name));
        }
        Commands::List { json } => {
            let records = store.list()?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &records.iter().map(AsRef::as_ref).collect::<Vec<_>>()
                    )?
                );
            } else if records.is_empty() {
                println!("No flows.");
            } else {
                for r in records {
                    println!("{} [{}]", clean(&r.flow.name), r.flow.status());
                }
            }
        }
        Commands::Show { name, json } => {
            let record = store
                .list()?
                .into_iter()
                .find(|r| r.flow.name == name)
                .context("Flow not found.")?;
            println!(
                "{}",
                if json {
                    serde_json::to_string_pretty(record.as_ref())?
                } else {
                    plain(&record.flow)
                }
            );
        }
        Commands::Delete { name } => {
            store.delete(&name)?;
            println!("Deleted: {}", clean(&name));
        }
        Commands::View { once } => {
            if once || !io::stdin().is_terminal() || !io::stdout().is_terminal() {
                let records = store.list()?;
                if records.is_empty() {
                    println!("No flows. Publish a flow to start.");
                } else {
                    println!(
                        "{}",
                        records
                            .iter()
                            .map(|r| plain(&r.flow))
                            .collect::<Vec<_>>()
                            .join("\n\n")
                    );
                }
            } else {
                runtime::view(store, cli.project)?;
            }
        }
        Commands::Open => {
            let pane = host::herdr(&["pane".into(), "current".into(), "--current".into()])?;
            let pane = &pane["pane"];
            ensure!(
                pane["workspace_id"].as_str() == Some(&scope.workspace),
                "Run open from the requested workspace."
            );
            let target = pane["pane_id"]
                .as_str()
                .context("The caller pane is missing.")?;
            let project = cli
                .project
                .or_else(|| {
                    pane["foreground_cwd"]
                        .as_str()
                        .or(pane["cwd"].as_str())
                        .map(PathBuf::from)
                })
                .context("Use --project PATH to set the source folder.")?
                .canonicalize()?;
            let mut args: Vec<String> = [
                "plugin",
                "pane",
                "open",
                "--plugin",
                "callstack",
                "--entrypoint",
                "tree",
                "--target-pane",
                target,
                "--direction",
                "right",
                "--no-focus",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect();
            for (key, value) in [
                ("CALLSTACK_NAMESPACE", scope.namespace),
                ("CALLSTACK_WORKSPACE", scope.workspace),
                ("CALLSTACK_SESSION", scope.session),
                ("CALLSTACK_STATE_DIR", root.to_string_lossy().into_owned()),
                ("CALLSTACK_PROJECT", project.to_string_lossy().into_owned()),
            ] {
                args.extend(["--env".into(), format!("{key}={value}")]);
            }
            println!("{}", host::herdr(&args)?);
        }
        Commands::Edit => unreachable!(),
    }
    Ok(())
}

fn main() {
    if let Err(error) = run(Cli::parse()) {
        eprintln!("Error: {}", clean(&format!("{error:#}")));
        std::process::exit(1);
    }
}
