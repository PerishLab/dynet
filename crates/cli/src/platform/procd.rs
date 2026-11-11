use crate::watch::Stage;
use dynet_core::{Error, Instance};

pub(crate) fn render(instance: &Instance, stage: &Stage) -> Result<String, Error> {
    if stage.config.local {
        return Err(Error::new("procd worker requires forwarded-only capture"));
    }
    let root = stage
        .state
        .configured()
        .ok_or_else(|| Error::new("procd worker requires an explicit state root"))?;
    let executable = std::env::current_exe()
        .map_err(|error| Error::new(format!("cannot locate the procd executable: {error}")))?;
    let executable = executable
        .to_str()
        .ok_or_else(|| Error::new("procd executable path is not UTF-8"))?;
    let root = root
        .to_str()
        .ok_or_else(|| Error::new("procd state root is not UTF-8"))?;
    let bootstrap = stage
        .errand
        .bootstrap
        .to_str()
        .filter(|_| stage.errand.bootstrap.is_absolute())
        .ok_or_else(|| Error::new("procd bootstrap requires an absolute UTF-8 path"))?;
    let argv = [
        executable.to_string(),
        "--instance".into(),
        instance.get().to_string(),
        "--port".into(),
        stage.port.to_string(),
        "--span".into(),
        stage.span.get(),
        "--state".into(),
        root.to_string(),
        "divert".into(),
        "--bootstrap".into(),
        bootstrap.to_string(),
        "--claim".into(),
        stage.errand.claim.clone(),
        "--seconds".into(),
        "0".into(),
    ];
    let command = argv
        .iter()
        .map(|value| quoted(value))
        .collect::<Vec<_>>()
        .join(" ");
    let sources = quoted(&format!("DYNET_SOURCES={}", stage.config.sources));
    let transit = quoted(&format!("DYNET_TRANSIT={}", stage.transit));
    let name = quoted(instance.get());
    let record = match &stage.config.record {
        Some(path) => {
            let path = path
                .to_str()
                .ok_or_else(|| Error::new("diagnostic control path is not UTF-8"))?;
            format!(" {}", quoted(&format!("DYNET_RECORD={path}")))
        }
        None => String::new(),
    };
    let stdout = u8::from(stage.config.record.is_some());
    Ok(format!(
        "#!/bin/sh /etc/rc.common\nUSE_PROCD=1\n\nstart_service() {{\n\
         \tprocd_open_instance {name}\n\
         \tprocd_set_param command {command}\n\
         \tprocd_set_param env 'DYNET_LOCAL=false' {sources} {transit}{record}\n\
         \tprocd_set_param limits 'core=0 0' 'nofile=512 512' 'as=536870912 536870912'\n\
         \tprocd_set_param respawn 3600 5 5\n\
         \tprocd_set_param term_timeout 10\n\
         \tprocd_set_param stdout {stdout}\n\
         \tprocd_set_param stderr 1\n\
         \tprocd_close_instance\n\
         }}\n\nreload_service() {{\n\
         \tprocd_send_signal \"${{initscript##*/}}\" {name} HUP\n\
         }}\n"
    ))
}

fn quoted(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
