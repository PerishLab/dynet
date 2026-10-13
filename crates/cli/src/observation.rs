use locus::{Candidate, Config, Context, Engine, Policy, Role};
use serde::Deserialize;
use serde_json::json;
use std::path::PathBuf;

#[derive(Default, Deserialize)]
struct File {
    #[serde(default)]
    locus: Settings,
}

#[derive(Default, Deserialize)]
struct Settings {
    #[serde(default)]
    enabled: bool,
    endpoint: Option<String>,
}

pub(crate) struct Run {
    engine: Engine,
    context: Context,
    cycle: Context,
}

impl Run {
    pub(crate) fn start(command: &str) -> Option<Self> {
        let home = plumb::config::data("dynet")?;
        match load(home, command) {
            Ok(run) => run,
            Err(error) => {
                eprintln!("dynet observation: {error}");
                None
            }
        }
    }

    pub(crate) fn view(&self) -> (&Engine, &Context) {
        (&self.engine, &self.context)
    }

    pub(crate) fn finish(self, code: i32) {
        let candidate = Candidate::event(json!({"event": "cli.finish", "code": code}));
        let _ = self.engine.append(&self.cycle, candidate);
    }
}

fn load(home: PathBuf, command: &str) -> Result<Option<Run>, String> {
    let path = home.join("config.toml");
    let file: File = match plumb::config::load(&path) {
        Ok(file) => file,
        Err(plumb::config::Error::Read { source, .. })
            if source.kind() == std::io::ErrorKind::NotFound =>
        {
            return Ok(None);
        }
        Err(plumb::config::Error::Parse { path, .. }) => {
            return Err(format!(
                "invalid observation configuration in {}; check [locus] settings",
                path.display()
            ));
        }
        Err(error) => return Err(error.to_string()),
    };
    if !file.locus.enabled {
        return Ok(None);
    }
    let endpoint = file.locus.endpoint.unwrap_or_else(|| {
        plumb::config::value("LOCUS_API").unwrap_or_else(|| "http://127.0.0.1:43308".into())
    });
    build(command, endpoint, home.join("state/locus")).map(Some)
}

fn build(command: &str, endpoint: String, buffer: PathBuf) -> Result<Run, String> {
    let engine = Engine::bootstrap(Config::new(
        Policy::default()
            .producer("dynet")
            .reporter(locus::reporter::Spec::api(endpoint, buffer)),
    ))
    .map_err(|error| error.to_string())?;
    let candidate = Candidate::context().ensure(Role::trace()).explicit(
        Role::new("dynet.command").map_err(|error| error.to_string())?,
        locus::Key::new(command).map_err(|error| error.to_string())?,
    );
    let context = engine
        .append(&Context::empty(), candidate)
        .map_err(|error| error.to_string())?
        .context();
    let cycle = engine
        .append(
            &context,
            Candidate::event(json!({"event": "cli.start"})).ensure(Role::span()),
        )
        .map_err(|error| error.to_string())?
        .context();
    Ok(Run {
        engine,
        context,
        cycle,
    })
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
    }
}
