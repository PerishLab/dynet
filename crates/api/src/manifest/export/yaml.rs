use crate::manifest::Entry;
use dynet_core::{Error, Manifest, Name, Subject};
use std::fmt::Write as _;

const FILTER: [&str; 13] = [
    "*.lan",
    "*.local",
    "localhost.ptlogin2.qq.com",
    "+.msftconnecttest.com",
    "+.msftncsi.com",
    "time.*.com",
    "time.*.gov",
    "time.*.edu.cn",
    "time.*.apple.com",
    "ntp.*.com",
    "+.ntp.org.cn",
    "dns.alidns.com",
    "doh.pub",
];

pub(super) struct Group {
    pub(super) name: String,
    pub(super) members: Vec<String>,
}

pub(super) struct Document(pub(super) String);

impl Document {
    pub(super) fn vless(
        &mut self,
        entry: &Entry,
        name: &str,
        addr: (&str, &str),
    ) -> Result<(), Error> {
        let uuid = required(entry, "uuid")?;
        let sni = required(entry, "servername")?;
        let key = required(entry, "reality-opts.public-key")?;
        let sid = entry.field("reality-opts.short-id").unwrap_or("");
        let _ = writeln!(
            self.0,
            "  - name: {}\n    type: vless\n    server: {}\n    port: {}\n    uuid: {}\n    network: tcp\n    tls: true\n    udp: false",
            quote(name),
            quote(addr.0),
            addr.1,
            quote(uuid)
        );
        if let Some(flow) = entry.field("flow").filter(|v| !v.is_empty()) {
            let _ = writeln!(self.0, "    flow: {}", quote(flow));
        }
        let _ = writeln!(
            self.0,
            "    servername: {}\n    client-fingerprint: chrome\n    reality-opts:\n      public-key: {}\n      short-id: {}",
            quote(sni),
            quote(key),
            quote(sid)
        );
        Ok(())
    }

    pub(super) fn vmess(
        &mut self,
        entry: &Entry,
        name: &str,
        addr: (&str, &str),
    ) -> Result<(), Error> {
        let uuid = required(entry, "uuid")?;
        let alter = entry.field("alterId").unwrap_or("0");
        let cipher = entry.field("cipher").unwrap_or("auto");
        let udp = entry.flag("udp");
        let _ = writeln!(
            self.0,
            "  - name: {}\n    type: vmess\n    server: {}\n    port: {}\n    uuid: {}\n    alterId: {alter}\n    cipher: {}\n    udp: {udp}",
            quote(name),
            quote(addr.0),
            addr.1,
            quote(uuid),
            quote(cipher)
        );
        Ok(())
    }

    pub(super) fn chained(&mut self, leg: &str, entry: &Entry, dialer: &str) -> Result<(), Error> {
        let server = required(entry, "server")?;
        let port = required(entry, "port")?;
        let cipher = required(entry, "cipher")?;
        let password = required(entry, "password")?;
        let _ = writeln!(
            self.0,
            "  - name: {}\n    type: ss\n    server: {}\n    port: {port}\n    cipher: {}\n    password: {}\n    udp: true",
            quote(leg),
            quote(server),
            quote(cipher),
            quote(password)
        );
        if let Some(uot) = entry.field("uot").filter(|v| !v.is_empty()) {
            let _ = writeln!(
                self.0,
                "    udp-over-tcp: true\n    udp-over-tcp-version: {uot}"
            );
        }
        let _ = writeln!(self.0, "    dialer-proxy: {}", quote(dialer));
        Ok(())
    }

    pub(super) fn header(&mut self) {
        self.0.push_str(concat!(
            "mixed-port: 7897\n",
            "allow-lan: false\n",
            "mode: rule\n",
            "log-level: warning\n",
            "ipv6: false\n",
            "unified-delay: true\n",
            "tcp-concurrent: true\n",
            "find-process-mode: strict\n",
            "dns:\n",
            "  enable: true\n",
            "  ipv6: false\n",
            "  enhanced-mode: fake-ip\n",
            "  fake-ip-range: 198.18.0.1/16\n",
            "  fake-ip-filter:\n",
        ));
        for item in FILTER {
            let _ = writeln!(self.0, "    - {}", quote(item));
        }
        self.0.push_str(concat!(
            "  default-nameserver:\n",
            "    - 223.5.5.5\n",
            "    - 119.29.29.29\n",
            "  nameserver:\n",
            "    - https://dns.alidns.com/dns-query\n",
            "    - https://doh.pub/dns-query\n",
            "  proxy-server-nameserver:\n",
            "    - 223.5.5.5\n",
            "    - 119.29.29.29\n",
            "  respect-rules: true\n",
        ));
    }

    pub(super) fn listing(&mut self, group: &Group, health: &str) {
        let _ = writeln!(
            self.0,
            "  - name: {}\n    type: url-test\n    url: {}\n    interval: 180\n    tolerance: 80\n    lazy: true\n    proxies:",
            quote(&group.name),
            quote(health)
        );
        for member in &group.members {
            let _ = writeln!(self.0, "      - {}", quote(member));
        }
    }

    pub(super) fn rules(&mut self, manifest: &Manifest) {
        self.0.push_str("rules:\n");
        for rule in manifest.table().rules() {
            let target = rule.cluster().get();
            let line = match rule.subject() {
                Subject::Exact(domain) => format!("DOMAIN,{},{target}", domain.get()),
                Subject::Suffix(domain) => format!("DOMAIN-SUFFIX,{},{target}", domain.get()),
                Subject::Holds(range) => {
                    format!(
                        "IP-CIDR,{}/{},{target},no-resolve",
                        range.base(),
                        range.prefix()
                    )
                }
            };
            let _ = writeln!(self.0, "  - {}", quote(&line));
        }
        let fallback = manifest.table().fallback().map_or("DIRECT", Name::get);
        let _ = writeln!(self.0, "  - {}", quote(&format!("MATCH,{fallback}")));
    }
}

pub(super) fn required<'a>(entry: &'a Entry, field: &str) -> Result<&'a str, Error> {
    entry
        .field(field)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::new(format!("node is missing required field {field}")))
}

fn quote(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len() + 2);
    escaped.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            other => escaped.push(other),
        }
    }
    escaped.push('"');
    escaped
}
