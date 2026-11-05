use dynet_api::control::{Private, Seed};
use dynet_api::host::State;
use dynet_core::{Error, Instance};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(clap::Args)]
pub struct Errand {
    #[arg(long)]
    bootstrap: PathBuf,
    #[arg(long)]
    issued: PathBuf,
}

pub fn seed(errand: &Errand, instance: &Instance, state: &State) -> Result<ExitCode, Error> {
    let receipt = Seed {
        bootstrap: &errand.bootstrap,
        issued: &errand.issued,
        instance,
        state,
    }
    .run(Private::machine())?;
    let receipt = serde_json::to_string(&receipt)
        .map_err(|_| Error::new("cannot render checkpoint receipt"))?;
    println!("{receipt}");
    Ok(ExitCode::SUCCESS)
}
