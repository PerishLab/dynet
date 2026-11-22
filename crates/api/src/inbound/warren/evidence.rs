use crate::resolver::Query;
use dynet_core::{Bearing, Course, Decision, Ground, Name, Subject, Told};
use serde_json::{Value, json};
use std::net::SocketAddrV4;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub enum Evidence<'a> {
    Line(&'a str),
    Lost,
}

pub type Reporter<'a> = Option<&'a (dyn Fn(Evidence<'_>) + Sync)>;

const LIMIT: usize = 32;
static NEXT: AtomicU64 = AtomicU64::new(1);

fn time() -> Option<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_millis()
        .try_into()
        .ok()
}

fn label(value: &str) -> Option<&str> {
    (value.len() <= 64
        && value
            .chars()
            .all(|c| c.is_alphanumeric() || " _./-".contains(c)))
    .then_some(value)
}

fn field(row: &mut Value, key: &str, value: &str) {
    if let Some(text) = label(value) {
        row[key] = json!(text);
    } else {
        row["labels_omitted"] = json!(true);
    }
}

pub(in crate::inbound) struct Resolution<'a> {
    pub generation: u64,
    pub query: &'a Query,
    pub rule: Option<&'a Subject>,
    pub cluster: Option<&'a Name>,
    pub source: &'a str,
}

fn outcome(body: Option<&[u8]>) -> &'static str {
    let Some(reply) = body else {
        return "unanswered";
    };
    match reply[3] & 15 {
        2 => "failed",
        3 => "negative",
        5 => "refused",
        0 if reply[2] & 2 != 0 => "truncated",
        0 if reply[6..8] == [0, 0] => "empty",
        0 => "answered",
        _ => "error",
    }
}

pub(in crate::inbound) fn dns(
    sink: &(dyn Fn(Evidence<'_>) + Sync),
    question: Resolution<'_>,
    body: Option<&[u8]>,
) {
    let Resolution {
        query,
        generation,
        rule,
        cluster,
        source,
    } = question;
    let Some(now) = time().filter(|_| generation > 0) else {
        sink(Evidence::Lost);
        return;
    };
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let mut row = json!({
        "v": 1, "event": "binding", "unix_ms": now, "generation":generation, "dns_id": id,
        "domain": query.name.trim_end_matches('.').to_ascii_lowercase(),
        "policy": if cluster.is_some() { "cluster" } else { "direct" },
    });
    if let Some(cluster) = cluster {
        field(&mut row, "cluster", cluster.get());
    }
    let mut header = row.clone();
    header["event"] = json!("dns");
    header["qtype"] = json!(query.kind);
    header["source"] = json!(source);
    match rule {
        Some(Subject::Exact(_)) => header["rule"] = json!("exact"),
        Some(Subject::Suffix(suffix)) => {
            header["rule"] = json!("suffix");
            header["suffix_labels"] = json!(suffix.get().split('.').count());
        }
        _ => header["rule"] = json!("default"),
    }
    let result = outcome(body);
    header["result"] = json!(result);
    let mut bindings = Vec::new();
    if let Some(reply) = body {
        header["rcode"] = json!(reply[3] & 15);
        if result == "answered" && matches!(query.kind, 1 | 28) {
            match crate::resolver::bindings(reply, query) {
                Ok(found) => bindings = found,
                Err(_) => header["bindings_unknown"] = json!(true),
            }
        }
    }
    header["binding_count"] = json!(bindings.len());
    header["omitted"] = json!(bindings.len().saturating_sub(LIMIT));
    sink(Evidence::Line(&header.to_string()));
    for (address, life) in bindings.into_iter().take(LIMIT) {
        row["address"] = json!(address);
        row["ttl_s"] = json!(life);
        sink(Evidence::Line(&row.to_string()));
    }
}

pub(in crate::inbound) struct Selection<'a> {
    pub generation: u64,
    pub target: SocketAddrV4,
    pub sight: &'a dynet_core::Sight,
    pub decision: &'a Decision,
    pub chosen: Option<&'a super::Chosen>,
    pub scope: &'static str,
}

pub(in crate::inbound) fn route(sink: Reporter<'_>, selection: Selection<'_>) {
    let Selection {
        target,
        generation,
        sight,
        decision,
        chosen,
        scope,
    } = selection;
    let Some(sink) = sink else { return };
    let Some(now) = time().filter(|_| generation > 0) else {
        sink(Evidence::Lost);
        return;
    };
    let ground = match decision.ground() {
        Ground::Named => "named",
        Ground::Held => "held",
        Ground::Unclaimed => "unclaimed",
        Ground::Issued => "issued",
        Ground::Fallback => "fallback",
    };
    let mut row = json!({
        "v":1, "event":"route", "unix_ms":now, "generation":generation,
        "transport": if sight.bearing==Bearing::Stream {"tcp"} else {"udp"},
        "address":target.ip(), "port":target.port(),
        "result":if chosen.is_some() {"selected"} else {"refused"},
        "scope":scope, "ground":ground,
    });
    match decision.course() {
        Course::Direct => row["policy"] = json!("direct"),
        Course::Through(cluster) | Course::Refused(cluster, _) => {
            row["policy"] = json!("cluster");
            field(&mut row, "cluster", cluster.get());
        }
    }
    if let Some(Told::Own(name)) = &sight.told {
        row["observed_name"] = json!(name.get());
    }
    if let Some(selection) = chosen {
        if let Some(charge) = &selection.charge {
            field(&mut row, "front", charge.label.get());
        }
        if let Some(exit) = &selection.exit {
            field(&mut row, "exit", exit.get());
        }
    }
    sink(Evidence::Line(&row.to_string()));
}

#[cfg(test)]
mod tests;
