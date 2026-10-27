mod condition;
mod engine;

use super::route::Prefix;
use super::widen;
use condition::{interface, port, program, remote};
use engine::{Blob, Engine, display};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::io::Result;
use std::net::Ipv4Addr;
use windows_sys::Win32::NetworkManagement::WindowsFilteringPlatform::{
    FWP_ACTION_BLOCK, FWP_ACTION_PERMIT, FWP_ACTION_TYPE, FWP_UINT8, FWP_V4_ADDR_AND_MASK,
    FWP_VALUE0, FWP_VALUE0_0, FWPM_ACTION0, FWPM_FILTER_CONDITION0,
    FWPM_FILTER_FLAG_CLEAR_ACTION_RIGHT, FWPM_FILTER_FLAG_PERSISTENT, FWPM_FILTER0,
    FWPM_LAYER_ALE_AUTH_CONNECT_V4, FWPM_LAYER_ALE_AUTH_CONNECT_V6,
    FWPM_LAYER_ALE_AUTH_RECV_ACCEPT_V4,
};
use windows_sys::core::GUID;

const DNS: u16 = 53;
const HOLD: &str = "hold ";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Settled {
    pub added: usize,
    pub dropped: usize,
}

pub struct Wall {
    instance: String,
}

struct Rule<'a> {
    label: String,
    layer: GUID,
    weight: u8,
    action: FWP_ACTION_TYPE,
    conditions: &'a mut [FWPM_FILTER_CONDITION0],
}

impl Wall {
    pub fn new(instance: &str) -> Self {
        Self {
            instance: instance.to_string(),
        }
    }

    pub fn raised(&self) -> bool {
        Engine::open().is_ok_and(|engine| engine.holds(self.key("sublayer")))
    }

    pub fn raise(&self, seat: Ipv4Addr, program: &str) -> Result<()> {
        let engine = Engine::open()?;
        let blob = Blob::program(program)?;
        engine.begin()?;
        let raised = self
            .found(&engine)
            .and_then(|()| self.base(&engine, seat, &blob));
        close(&engine, raised)
    }

    pub fn settle(&self, tunnel: u64, wanted: &BTreeSet<Prefix>) -> Result<Settled> {
        let engine = Engine::open()?;
        let held = self.held(&engine)?;
        engine.begin()?;
        let settled = self.tunnel(&engine, tunnel).and_then(|()| {
            let mut settled = Settled::default();
            for prefix in wanted.difference(&held) {
                self.block(&engine, *prefix)?;
                settled.added += 1;
            }
            for prefix in held.difference(wanted) {
                engine.delete(self.key(&spoken(*prefix)))?;
                settled.dropped += 1;
            }
            Ok(settled)
        });
        close(&engine, settled)
    }

    pub fn hold(&self, prefix: Prefix) -> Result<()> {
        let engine = Engine::open()?;
        engine.begin()?;
        let held = self.block(&engine, prefix);
        close(&engine, held)
    }

    pub fn holds(&self) -> Result<BTreeSet<Prefix>> {
        self.held(&Engine::open()?)
    }

    pub fn lower(&self) -> Result<bool> {
        let engine = Engine::open()?;
        if !engine.holds(self.key("sublayer")) {
            return Ok(false);
        }
        let listed = engine.listed(self.key("provider"))?;
        engine.begin()?;
        let lowered = listed
            .iter()
            .try_for_each(|(key, _)| engine.delete(*key).map(|_| ()))
            .and_then(|()| engine.forget(self.key("sublayer"), self.key("provider")));
        close(&engine, lowered).map(|()| true)
    }

    fn found(&self, engine: &Engine) -> Result<()> {
        let name = format!("dynet {}", self.instance);
        engine.provider(self.key("provider"), &name)?;
        engine.sublayer(self.key("sublayer"), self.key("provider"), &name)
    }

    fn base(&self, engine: &Engine, seat: Ipv4Addr, blob: &Blob) -> Result<()> {
        for (label, layer) in [
            ("own v4", FWPM_LAYER_ALE_AUTH_CONNECT_V4),
            ("own v6", FWPM_LAYER_ALE_AUTH_CONNECT_V6),
        ] {
            let mut conditions = [program(blob.get())];
            self.place(
                engine,
                rule(label, layer, (14, FWP_ACTION_PERMIT), &mut conditions),
            )?;
        }
        let mut mask = FWP_V4_ADDR_AND_MASK {
            addr: u32::from(seat),
            mask: u32::MAX,
        };
        let mut conditions = [remote(&mut mask), port(DNS)];
        let layer = FWPM_LAYER_ALE_AUTH_CONNECT_V4;
        self.place(
            engine,
            rule("seat", layer, (12, FWP_ACTION_PERMIT), &mut conditions),
        )?;
        for (label, layer) in [
            ("dns v4", FWPM_LAYER_ALE_AUTH_CONNECT_V4),
            ("dns v6", FWPM_LAYER_ALE_AUTH_CONNECT_V6),
        ] {
            let mut conditions = [port(DNS)];
            self.place(
                engine,
                rule(label, layer, (10, FWP_ACTION_BLOCK), &mut conditions),
            )?;
        }
        Ok(())
    }

    fn tunnel(&self, engine: &Engine, luid: u64) -> Result<()> {
        let mut leaving = luid;
        let mut conditions = [interface(&mut leaving)];
        let layer = FWPM_LAYER_ALE_AUTH_CONNECT_V4;
        self.place(
            engine,
            rule("tunnel", layer, (13, FWP_ACTION_PERMIT), &mut conditions),
        )?;
        let mut arriving = luid;
        let mut conditions = [interface(&mut arriving)];
        let layer = FWPM_LAYER_ALE_AUTH_RECV_ACCEPT_V4;
        self.place(
            engine,
            rule("accept", layer, (15, FWP_ACTION_PERMIT), &mut conditions),
        )
    }

    fn block(&self, engine: &Engine, prefix: Prefix) -> Result<()> {
        let mut mask = FWP_V4_ADDR_AND_MASK {
            addr: u32::from(prefix.0),
            mask: u32::MAX.checked_shl(32 - u32::from(prefix.1)).unwrap_or(0),
        };
        let mut conditions = [remote(&mut mask)];
        let layer = FWPM_LAYER_ALE_AUTH_CONNECT_V4;
        let label = spoken(prefix);
        self.place(
            engine,
            rule(&label, layer, (8, FWP_ACTION_BLOCK), &mut conditions),
        )
    }

    fn place(&self, engine: &Engine, rule: Rule) -> Result<()> {
        let key = self.key(&rule.label);
        engine.delete(key)?;
        let mut name = widen(&format!("dynet {} {}", self.instance, rule.label));
        let mut provider = self.key("provider");
        let hard = match rule.action == FWP_ACTION_PERMIT && rule.weight == 15 {
            true => FWPM_FILTER_FLAG_CLEAR_ACTION_RIGHT,
            false => 0,
        };
        let filter = FWPM_FILTER0 {
            filterKey: key,
            displayData: display(&mut name),
            flags: FWPM_FILTER_FLAG_PERSISTENT | hard,
            providerKey: &mut provider,
            layerKey: rule.layer,
            subLayerKey: self.key("sublayer"),
            weight: FWP_VALUE0 {
                r#type: FWP_UINT8,
                Anonymous: FWP_VALUE0_0 { uint8: rule.weight },
            },
            numFilterConditions: rule.conditions.len() as u32,
            filterCondition: rule.conditions.as_mut_ptr(),
            action: FWPM_ACTION0 {
                r#type: rule.action,
                ..Default::default()
            },
            ..Default::default()
        };
        engine.add(&filter)
    }

    fn held(&self, engine: &Engine) -> Result<BTreeSet<Prefix>> {
        let head = format!("dynet {} {HOLD}", self.instance);
        Ok(engine
            .listed(self.key("provider"))?
            .into_iter()
            .filter_map(|(_, name)| name.strip_prefix(&head).and_then(read))
            .collect())
    }

    fn key(&self, label: &str) -> GUID {
        let digest = Sha256::digest(format!("dynet/{}/{label}", self.instance).as_bytes());
        let mut tail = [0; 8];
        tail.copy_from_slice(&digest[8..16]);
        GUID {
            data1: u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]]),
            data2: u16::from_be_bytes([digest[4], digest[5]]),
            data3: u16::from_be_bytes([digest[6], digest[7]]),
            data4: tail,
        }
    }
}

fn rule<'a>(
    label: &str,
    layer: GUID,
    told: (u8, FWP_ACTION_TYPE),
    conditions: &'a mut [FWPM_FILTER_CONDITION0],
) -> Rule<'a> {
    Rule {
        label: label.to_string(),
        layer,
        weight: told.0,
        action: told.1,
        conditions,
    }
}

fn close<T>(engine: &Engine, done: Result<T>) -> Result<T> {
    match done {
        Ok(value) => engine.commit().map(|()| value),
        Err(error) => {
            engine.abort();
            Err(error)
        }
    }
}

fn spoken(prefix: Prefix) -> String {
    format!("{HOLD}{}/{}", prefix.0, prefix.1)
}

fn read(text: &str) -> Option<Prefix> {
    let (base, length) = text.split_once('/')?;
    Some((base.parse().ok()?, length.parse().ok()?))
}
