use super::Boundary;
use crate::Config;
use crate::watch::{Errand, Stage};
use dynet_api::host;
use dynet_core::{Error, Instance, Span};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

#[derive(clap::Subcommand)]
pub(crate) enum Service {
    Install(Errand),
    Start,
    Stop,
    Restart,
    Uninstall,
}

impl Boundary {
    pub(crate) fn service(
        &self,
        config: &Config,
        command: &Service,
        state: &host::State,
    ) -> Result<ExitCode, Error> {
        let registration = Registration(&self.0);
        match command {
            Service::Install(errand) => registration.install(config, state, errand)?,
            Service::Start => registration.start()?,
            Service::Stop => {
                registration.stop()?;
                host::reclaim(&self.0)?;
            }
            Service::Restart => {
                registration.stop()?;
                registration.start()?;
            }
            Service::Uninstall => {
                let path = registration.path();
                if host::held(&path)? == host::Held::Foreign {
                    return Err(Error::new(
                        "the launchd property list belongs to another owner",
                    ));
                }
                registration.stop()?;
                host::reclaim(&self.0)?;
                host::Fragment::remove(&path)?;
            }
        }
        Ok(ExitCode::SUCCESS)
    }
}

pub(super) fn stop(instance: &Instance) -> Result<(), Error> {
    Registration(instance).stop()
}

pub(crate) fn unit(instance: &Instance, stage: &Stage) -> Result<String, Error> {
    Registration(instance).unit(stage)
}

struct Registration<'a>(&'a Instance);

impl Registration<'_> {
    fn label(&self) -> String {
        format!("uk.perish.dynet.{}", self.0.get())
    }

    fn path(&self) -> PathBuf {
        PathBuf::from(format!("/Library/LaunchDaemons/{}.plist", self.label()))
    }

    fn stop(&self) -> Result<(), Error> {
        let target = format!("system/{}", self.label());
        if !host::attempt("/bin/launchctl", &["print", &target]) {
            return Ok(());
        }
        if host::held(&self.path())? != host::Held::Owned {
            return Err(Error::new(
                "the loaded launchd service has no owned property list",
            ));
        }
        host::run("/bin/launchctl", &["bootout", &target])?;
        let began = Instant::now();
        while host::attempt("/bin/launchctl", &["print", &target]) {
            if began.elapsed() >= Duration::from_secs(30) {
                return Err(Error::new("launchd has not finished unloading the service"));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Ok(())
    }

    fn start(&self) -> Result<(), Error> {
        let path = self.path();
        if host::held(&path)? != host::Held::Owned {
            return Err(Error::new(
                "install the owned launchd service before starting it",
            ));
        }
        let target = format!("system/{}", self.label());
        if host::attempt("/bin/launchctl", &["print", &target]) {
            host::run("/bin/launchctl", &["kickstart", &target])?;
        } else {
            host::run(
                "/bin/launchctl",
                &["bootstrap", "system", &path.to_string_lossy()],
            )?;
        }
        Ok(())
    }

    fn install(&self, config: &Config, state: &host::State, errand: &Errand) -> Result<(), Error> {
        let program = program()?;
        trusted(&program)?;
        trusted(&program.canonicalize().map_err(failed)?)?;
        if !config.sources.is_empty() || config.transit != 0 {
            return Err(Error::new("macOS service captures this host alone"));
        }
        if errand.bare || errand.unit || config.port != 53 {
            return Err(Error::new(
                "macOS service requires native PF capture and resolver port 53",
            ));
        }
        if errand.claim != "0.0.0.0/0" {
            return Err(Error::new(
                "macOS service captures issued and held addresses",
            ));
        }
        let source = std::path::absolute(&errand.bootstrap).map_err(failed)?;
        let private = dynet_api::control::Private::machine();
        dynet_api::control::Bootstrap::read(&source, private)?.inspect(private, self.0.mark())?;
        std::fs::create_dir_all(state.root()).map_err(failed)?;
        trusted(state.root())?;
        trusted(&state.root().canonicalize().map_err(failed)?)?;
        std::fs::set_permissions(state.root(), std::fs::Permissions::from_mode(0o700))
            .map_err(failed)?;
        let stage = Stage {
            errand,
            state,
            port: config.port,
            span: Span::new(&config.span)?,
            transit: config.transit,
        };
        let text = self.unit(&stage)?;
        let path = self.path();
        if host::held(&path)? == host::Held::Foreign {
            return Err(Error::new(
                "the launchd property list belongs to another owner",
            ));
        }
        if host::attempt(
            "/bin/launchctl",
            &["print", &format!("system/{}", self.label())],
        ) {
            return Err(Error::new(
                "stop the installed service before replacing its registration",
            ));
        }
        std::fs::write(&path, text).map_err(failed)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).map_err(failed)?;
        host::run("/usr/bin/plutil", &["-lint", &path.to_string_lossy()])?;
        self.start()
    }

    fn unit(&self, stage: &Stage) -> Result<String, Error> {
        let program = program()?;
        let bootstrap = std::path::absolute(&stage.errand.bootstrap).map_err(failed)?;
        let arguments = vec![
            program.to_string_lossy().into_owned(),
            "--instance".into(),
            self.0.get().into(),
            "--port".into(),
            stage.port.to_string(),
            "--span".into(),
            stage.span.get(),
            "--state".into(),
            stage.state.root().to_string_lossy().into_owned(),
            "divert".into(),
            "--service".into(),
            "--bootstrap".into(),
            bootstrap.to_string_lossy().into_owned(),
            "--claim".into(),
            stage.errand.claim.clone(),
            "--seconds".into(),
            "0".into(),
        ];
        let arguments: String = arguments
            .iter()
            .map(|value| format!("<string>{}</string>", escape(value)))
            .collect();
        let log = stage.state.root().join(format!("{}.log", self.0.get()));
        Ok(format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!-- {} -->\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict><key>Label</key><string>{}</string><key>ProgramArguments</key><array>{arguments}</array><key>RunAtLoad</key><true/><key>KeepAlive</key><true/><key>ThrottleInterval</key><integer>5</integer><key>ExitTimeOut</key><integer>30</integer><key>Umask</key><integer>63</integer><key>StandardOutPath</key><string>{}</string><key>StandardErrorPath</key><string>{}</string></dict></plist>\n",
            host::MARKER,
            self.label(),
            escape(&log.to_string_lossy()),
            escape(&log.to_string_lossy())
        ))
    }
}

fn program() -> Result<PathBuf, Error> {
    let actual = std::env::current_exe()
        .map_err(failed)?
        .canonicalize()
        .map_err(failed)?;
    if let Some(invoked) = std::env::args_os().next().map(PathBuf::from)
        && invoked.is_absolute()
        && invoked
            .canonicalize()
            .is_ok_and(|resolved| resolved == actual)
    {
        return Ok(invoked);
    }
    Ok(actual)
}

fn trusted(path: &Path) -> Result<(), Error> {
    for parent in path.ancestors() {
        let metadata = parent.metadata().map_err(failed)?;
        if metadata.uid() != 0 || metadata.permissions().mode() & 0o022 != 0 {
            return Err(Error::new(format!(
                "{} must be root-owned and not writable by other users",
                parent.display()
            )));
        }
    }
    Ok(())
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\"', "&quot;")
        .replace('\'', "&apos;")
}

fn failed(error: std::io::Error) -> Error {
    Error::new(format!("the service could not be registered: {error}"))
}
