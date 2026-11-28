use super::{active, render, same};
use crate::control::{Candidate, Payload, Session};
use dynet_core::Error;
use secrets::{Code, Phase};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const LIMIT: usize = 4096;
const TIMEOUT: Duration = Duration::from_secs(2);
const POLL: Duration = Duration::from_millis(100);

pub fn serve(listener: TcpListener, session: Session, stop: Arc<AtomicBool>) -> Result<(), Error> {
    let local = listener
        .local_addr()
        .map_err(|error| Error::new(format!("cannot read listener address: {error}")))?;
    if !local.ip().is_loopback() {
        return Err(Error::new(
            "projection server must bind to a loopback address",
        ));
    }
    listener
        .set_nonblocking(true)
        .map_err(|error| Error::new(format!("cannot configure nonblocking listener: {error}")))?;

    let mut held: Option<Payload> = initial(&session)?;
    let phase = if held.is_some() {
        Phase::Applied
    } else {
        Phase::Revoked
    };
    session.control.phase(phase, None);

    while !stop.load(Ordering::Acquire) {
        if session.control.failed() {
            return Err(Error::new("encrypted control durability failed"));
        }
        let _ = session
            .control
            .reconcile(|candidate| apply(&mut held, candidate));
        match listener.accept() {
            Ok((stream, _)) => answer(stream, held.as_ref(), clock()),
            Err(_) => std::thread::sleep(POLL),
        }
    }
    Ok(())
}

fn apply(held: &mut Option<Payload>, candidate: &Candidate) -> Result<(), Error> {
    let Some(payload) = &candidate.payload else {
        *held = None;
        return Ok(());
    };
    let spec = payload
        .projection
        .as_ref()
        .ok_or_else(|| Error::new("projection payload missing projection section"))?;
    let _ = render(&payload.loaded, Some(spec), clock())?;
    *held = Some(payload.clone());
    Ok(())
}

fn initial(session: &Session) -> Result<Option<Payload>, Error> {
    let Some(payload) = &session.initial.payload else {
        return Ok(None);
    };
    let Some(spec) = &payload.projection else {
        session.control.phase(Phase::Failed, Some(Code::Input));
        return Err(Error::new(
            "projection service requires a payload carrying a projection section",
        ));
    };
    let _ = render(&payload.loaded, Some(spec), clock())?;
    Ok(Some(payload.clone()))
}

fn answer(mut stream: TcpStream, held: Option<&Payload>, now: u64) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(TIMEOUT));
    let _ = stream.set_write_timeout(Some(TIMEOUT));

    let Some((method, token)) = parse(&mut stream) else {
        let _ = missing(&mut stream);
        return;
    };
    let Some(body) = resolve(held, &token, now) else {
        let _ = missing(&mut stream);
        return;
    };
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/yaml; charset=utf-8\r\n\
Cache-Control: no-store, no-cache, must-revalidate\r\n\
Content-Disposition: attachment; filename=\"dynet.yaml\"\r\n\
Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    if method == "GET" {
        let _ = stream.write_all(body.as_bytes());
    }
    let _ = stream.flush();
}

fn resolve(held: Option<&Payload>, token: &str, now: u64) -> Option<String> {
    let payload = held?;
    let spec = payload.projection.as_ref()?;
    let matched = active(spec, now)
        .iter()
        .any(|(_, expected)| same(token, expected));
    if !matched {
        return None;
    }
    render(&payload.loaded, Some(spec), now).ok()
}

fn parse(stream: &mut TcpStream) -> Option<(String, String)> {
    let mut buf = [0u8; LIMIT];
    let mut filled = 0usize;
    while filled < buf.len() {
        let read = stream.read(&mut buf[filled..]).ok()?;
        if read == 0 {
            break;
        }
        filled += read;
        if buf[..filled].windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }
    let text = std::str::from_utf8(&buf[..filled]).ok()?;
    let mut parts = text.lines().next()?.split_whitespace();
    let method = parts.next()?.to_owned();
    let target = parts.next()?;
    let version = parts.next()?;
    if !version.starts_with("HTTP/1.") || (method != "GET" && method != "HEAD") {
        return None;
    }
    let token = target.strip_prefix('/')?;
    if token.is_empty() || token.contains('/') || token.contains('?') {
        return None;
    }
    Some((method, token.to_owned()))
}

fn missing(stream: &mut TcpStream) -> std::io::Result<()> {
    stream.write_all(
        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
    )?;
    stream.flush()
}

fn clock() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}
