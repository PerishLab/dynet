use super::super::run;
use dynet_core::{Error, Instance};
use dynet_windows::Wall;
use std::net::Ipv4Addr;

pub struct Veil;

impl Veil {
    pub fn raised(instance: &Instance) -> bool {
        Self::ruled(instance) || Self::walled(instance)
    }

    pub fn walled(instance: &Instance) -> bool {
        Wall::new(instance.get()).raised()
    }

    pub fn ruled(instance: &Instance) -> bool {
        let script = format!(
            "@(Get-DnsClientNrptRule | Where-Object Comment -eq '{}').Count",
            comment(instance)
        );
        shell(&script).is_ok_and(|count| count.trim() != "0")
    }

    pub fn attended(seat: Ipv4Addr, port: u16) -> bool {
        match std::net::UdpSocket::bind((seat, port)) {
            Ok(_) => false,
            Err(error) => error.kind() == std::io::ErrorKind::AddrInUse,
        }
    }

    pub fn raise(instance: &Instance, seat: Ipv4Addr) -> Result<(), Error> {
        let program = super::program()?;
        Wall::new(instance.get())
            .raise(seat, &program.to_string_lossy())
            .map_err(|error| Error::new(format!("the wall was not raised: {error}")))?;
        let script = format!(
            "$rules = @(Get-DnsClientNrptRule | Where-Object Comment -eq '{}'); if ($rules.Count -eq 0) {{ Add-DnsClientNrptRule -Namespace '.' -NameServers '{seat}' -Comment '{}' | Out-Null }} else {{ $rules | ForEach-Object {{ Set-DnsClientNrptRule -Name $_.Name -NameServers '{seat}' }} }}; Clear-DnsClientCache",
            comment(instance),
            comment(instance)
        );
        shell(&script).map(|_| ())
    }

    pub fn lower(instance: &Instance) -> Result<bool, Error> {
        let ruled = Self::ruled(instance);
        if ruled {
            let script = format!(
                "Get-DnsClientNrptRule | Where-Object Comment -eq '{}' | ForEach-Object {{ Remove-DnsClientNrptRule -Name $_.Name -Force }}; Clear-DnsClientCache",
                comment(instance)
            );
            shell(&script)?;
        }
        let walled = Wall::new(instance.get())
            .lower()
            .map_err(|error| Error::new(format!("the wall was not lowered: {error}")))?;
        Ok(ruled || walled)
    }
}

fn comment(instance: &Instance) -> String {
    format!("dynet:{}", instance.get())
}

fn shell(script: &str) -> Result<String, Error> {
    let script = format!("$ErrorActionPreference='Stop'; {script}");
    run(
        "powershell",
        &["-NoProfile", "-NonInteractive", "-Command", &script],
    )
}
