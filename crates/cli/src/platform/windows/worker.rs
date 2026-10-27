use crate::watch::{self, Stage};
use dynet_api::host::{self, clock};
use dynet_core::{Error, Instance};
use std::io::Write;
use std::process::ExitCode;
use std::sync::Mutex;

pub(crate) fn serve(instance: &Instance, stage: &Stage) -> Result<ExitCode, Error> {
    let instance = instance.clone();
    let errand = stage.errand.clone();
    let state = stage.state.clone();
    let (port, span) = (stage.port, stage.span);
    let name = instance.get().to_string();
    dynet_host::windows::serve(&name, move || {
        let stage = Stage {
            errand: &errand,
            state: &state,
            port,
            span,
            transit: 0,
        };
        work(&instance, &stage)
    })
    .map_err(|error| Error::new(format!("the service was not served: {error}")))?;
    Ok(ExitCode::SUCCESS)
}

fn work(instance: &Instance, stage: &Stage) -> u32 {
    if dynet_api::control::Private::machine()
        .folder(stage.state.root())
        .is_err()
    {
        return 1;
    }
    let log = stage.state.root().join(format!("{}.log", instance.get()));
    let Some(parent) = log.parent() else { return 1 };
    if std::fs::create_dir_all(parent).is_err() {
        return 1;
    }
    let file = match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
    {
        Ok(file) => file,
        Err(_) => return 1,
    };
    let file = Mutex::new(file);
    let told = |line: &str| {
        if let Ok(mut held) = file.lock() {
            let _ = writeln!(held, "{} {line}", clock());
        }
    };
    let ground = host::Ground {
        port: stage.port,
        claim: &stage.errand.claim,
        span: stage.span,
        bare: stage.errand.bare,
        sources: &[],
        transit: 0,
    };
    told(&format!(
        "starting {} {}",
        instance.get(),
        dynet_api::VERSION
    ));
    if let Err(error) = host::establish(instance, &ground) {
        told(&format!("refused to raise: {error}"));
        return 1;
    }
    let carried = watch::carried(instance, stage, (dynet_host::windows::halted(), &told));
    let code = match &carried {
        Ok(_) => 0,
        Err(error) => {
            told(&format!(
                "diverting failed, claimed destinations stay closed: {error}"
            ));
            1
        }
    };
    if carried.is_ok() && !dynet_host::windows::closing() {
        match host::reclaim(instance) {
            Ok(cleared) => told(&format!("stopped and lowered: veil={}", cleared.veil)),
            Err(error) => told(&format!("stopped but not lowered: {error}")),
        }
    }
    code
}
