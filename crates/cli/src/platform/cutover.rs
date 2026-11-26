use crate::platform::procd;
use dynet_api::control::Private;
use dynet_api::cutover::{Arm, Guard};
use dynet_api::host::State;
use dynet_core::{Error, Instance};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};

#[derive(clap::Subcommand)]
pub enum Cutover {
    Arm {
        #[arg(long)]
        bootstrap: PathBuf,
        #[arg(long)]
        candidate: PathBuf,
        #[arg(long)]
        rollback: PathBuf,
        #[arg(long, default_value = "90")]
        window: u64,
        #[arg(long, default_value = "15")]
        settle: u64,
        #[arg(long)]
        user: bool,
    },
    Apply {
        #[arg(long)]
        id: String,
        #[arg(long)]
        settle: Option<u64>,
        #[arg(long)]
        user: bool,
    },
    Confirm {
        #[arg(long)]
        id: String,
        #[arg(long)]
        user: bool,
    },
    Rollback {
        #[arg(long)]
        id: String,
        #[arg(long)]
        settle: Option<u64>,
        #[arg(long)]
        user: bool,
    },
    Watch {
        #[arg(long)]
        id: String,
        #[arg(long)]
        settle: Option<u64>,
        #[arg(long)]
        user: bool,
    },
    Reconcile {
        #[arg(long)]
        settle: Option<u64>,
        #[arg(long)]
        user: bool,
    },
    Status {
        #[arg(long)]
        user: bool,
    },
    Clear {
        #[arg(long)]
        id: String,
        #[arg(long)]
        user: bool,
    },
}

pub(crate) fn run(
    command: &Cutover,
    instance: &Instance,
    state: &State,
) -> Result<ExitCode, Error> {
    match command {
        Cutover::Arm {
            bootstrap,
            candidate,
            rollback,
            window,
            settle,
            user,
        } => {
            let guard = Guard::new(instance, state, custody(*user));
            let record = guard.arm(&Arm {
                bootstrap,
                candidate,
                rollback,
                window: *window,
                settle: *settle,
            })?;
            emit(&record)
        }
        Cutover::Apply { id, settle, user } => {
            let guard = Guard::new(instance, state, custody(*user));
            let _ = guard.begin(id)?;
            if let Err(error) = spawn(instance, state, id, *user) {
                let _ = guard.abort(id, &error.to_string());
                return Err(error);
            }
            let record = guard.activate(id, *settle)?;
            emit(&record)
        }
        Cutover::Confirm { id, user } => {
            let guard = Guard::new(instance, state, custody(*user));
            let record = guard.confirm(id)?;
            emit(&record)
        }
        Cutover::Rollback { id, settle, user } => {
            let guard = Guard::new(instance, state, custody(*user));
            let record = guard.rollback(id, *settle)?;
            emit(&record)
        }
        Cutover::Watch { id, settle, user } => {
            dynet_api::cutover::detach()?;
            let guard = Guard::new(instance, state, custody(*user));
            let _ = guard.watch(id, *settle)?;
            Ok(ExitCode::SUCCESS)
        }
        Cutover::Reconcile { settle, user } => {
            let guard = Guard::new(instance, state, custody(*user));
            match guard.reconcile(*settle)? {
                Some(record) => emit(&record),
                None => idle(),
            }
        }
        Cutover::Status { user } => {
            let guard = Guard::new(instance, state, custody(*user));
            match guard.status()? {
                Some(record) => emit(&record),
                None => idle(),
            }
        }
        Cutover::Clear { id, user } => {
            let guard = Guard::new(instance, state, custody(*user));
            let record = guard.clear(id)?;
            emit(&record)
        }
    }
}

fn custody(user: bool) -> Private {
    match user {
        true => Private::current(),
        false => Private::machine(),
    }
}

fn spawn(instance: &Instance, state: &State, id: &str, user: bool) -> Result<(), Error> {
    let executable = procd::executable()?;
    let mut command = Command::new(executable);
    command.args(["--instance", instance.get()]);
    if let Some(root) = state.configured() {
        command.arg("--state").arg(root);
    }
    command.args(["cutover", "watch", "--id", id]);
    if user {
        command.arg("--user");
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map(drop)
        .map_err(|error| Error::new(format!("cannot spawn deadline worker: {error}")))
}

fn emit(record: &impl serde::Serialize) -> Result<ExitCode, Error> {
    let rendered = serde_json::to_string(record)
        .map_err(|_| Error::new("cannot render cutover transaction receipt"))?;
    println!("{rendered}");
    Ok(ExitCode::SUCCESS)
}

fn idle() -> Result<ExitCode, Error> {
    println!(r#"{{"phase":"idle"}}"#);
    Ok(ExitCode::SUCCESS)
}
