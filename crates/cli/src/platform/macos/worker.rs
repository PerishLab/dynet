use crate::watch::{self, Stage};
use dynet_api::host;
use dynet_core::{Error, Instance};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

pub(crate) fn serve(instance: &Instance, stage: &Stage) -> Result<ExitCode, Error> {
    let halt = Arc::new(AtomicBool::new(false));
    for signal in [signal_hook::consts::SIGTERM, signal_hook::consts::SIGINT] {
        signal_hook::flag::register(signal, Arc::clone(&halt))
            .map_err(|error| Error::new(format!("cannot listen for a halt: {error}")))?;
    }
    let told = |line: &str| println!("{} {line}", host::clock());
    let ground = host::Ground {
        local: stage.config.local,
        port: stage.port,
        claim: &stage.errand.claim,
        span: stage.span,
        bare: stage.errand.bare,
        sources: &[],
        transit: stage.transit,
    };
    told(&format!(
        "starting {} {}",
        instance.get(),
        dynet_api::VERSION
    ));
    host::establish(instance, &ground)?;
    let carried = watch::carried(instance, stage, (&halt, &told));
    match carried {
        Ok(_) => {
            told("stopped; protection is retained until explicit service stop or down");
            Ok(ExitCode::SUCCESS)
        }
        Err(error) => {
            told(&format!(
                "diverting failed; claimed destinations stay closed: {error}"
            ));
            Err(error)
        }
    }
}
