use dynet_api::host::Journal;
use dynet_core::{Error, Instance};
use std::process::ExitCode;
use std::time::{Duration, Instant};

const PATIENCE: Duration = Duration::from_secs(10);
const REST: Duration = Duration::from_millis(100);

#[derive(clap::Args)]
pub struct Errand {
    pub told: String,
    #[arg(long)]
    pub dry: bool,
}

pub fn flush(
    instance: &Instance,
    errand: &Errand,
    state: &dynet_api::host::State,
) -> Result<ExitCode, Error> {
    let journal = Journal::new(instance, state, None);
    let order = match errand.dry {
        true => format!("{} dry", errand.told),
        false => errand.told.clone(),
    };
    journal.ask(&order)?;
    let started = Instant::now();
    while started.elapsed() < PATIENCE {
        if let Ok(reply) = std::fs::read_to_string(journal.answer()) {
            let _ = std::fs::remove_file(journal.answer());
            println!("{reply}");
            return Ok(match reply.starts_with("refused") {
                true => ExitCode::from(1),
                false => ExitCode::SUCCESS,
            });
        }
        std::thread::sleep(REST);
    }
    let _ = std::fs::remove_file(journal.order());
    Err(Error::new(format!(
        "no dynet {} answered the flush order within {}s; is it diverting?",
        instance.get(),
        PATIENCE.as_secs()
    )))
}
