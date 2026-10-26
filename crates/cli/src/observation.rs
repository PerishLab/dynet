use plumb::observation::{Declaration, Run};

pub(crate) fn start(command: &str) -> Option<Run> {
    let home = plumb::config::data("dynet")?;
    Run::start(
        Declaration {
            producer: "dynet",
            file: &home.join("config.toml"),
            buffer: home.join("state/locus"),
        },
        command,
    )
}

pub(crate) fn name(command: &crate::Command) -> &'static str {
    match command {
        crate::Command::Doctor => "doctor",
        crate::Command::Up { .. } => "up",
        crate::Command::Down => "down",
        crate::Command::Standing => "standing",
        crate::Command::Divert(_) => "divert.unit",
        crate::Command::Resolve { .. } => "resolve",
        crate::Command::Reach { .. } => "reach",
        crate::Command::Spread { .. } => "spread",
        crate::Command::Exit { .. } => "exit",
        crate::Command::Carry(_) => "carry",
        crate::Command::Flush(_) => "flush",
    }
}
