use super::Data;
use crate::io::{Io, Len, Show, Str};
use ac_core::{Endian, Res};

#[derive(Default)]
pub struct WwiseIds {
    counts: [u32; 10],
    groups: Vec<Group>,
    links: Vec<Link>,
    maps: [Vec<[u32; 2]>; 8],
    names: Vec<String>,
}

#[derive(Default)]
struct Group {
    name: String,
    unused: [u32; 2],
    banks: Vec<String>,
}

#[derive(Default)]
struct Link {
    id: u32,
    a: u32,
    b: u32,
}

const NONE: u32 = u32::MAX;

impl WwiseIds {
    fn walk<S: Io>(&mut self, s: &mut S) -> Res<()> {
        s.konst(5u32)?;
        s.hide(&mut self.counts)?;
        s.note("Groups: bank name patterns; Links: Id -> A and B (missing = none); Map1..Map8: { key, value }");
        let n = s.n(self.counts[0]);
        s.list("Groups", &mut self.groups, n, |s, g| {
            s.text("Name", &mut g.name, Str::L32)?;
            s.opt("Unused", &mut g.unused, Show::Hex)?;
            s.list("Banks", &mut g.banks, Len::U32, |s, b| s.text("", b, Str::L32))
        })?;
        let n = s.n(self.counts[1]);
        s.list("Links", &mut self.links, n, |s, l| {
            s.hex("Id", &mut l.id)?;
            s.or("A", &mut l.a, Show::Hex, NONE)?;
            s.or("B", &mut l.b, Show::Hex, NONE)
        })?;
        for (i, m) in self.maps.iter_mut().enumerate() {
            let k = format!("Map{}", i + 1);
            if !s.skip(&k, m.is_empty()) {
                let n = s.n(self.counts[2 + i]);
                s.list(&k, m, n, |s, x| s.val("", x, Show::Hex))?;
            }
        }
        s.list("Names", &mut self.names, Len::U32, |s, x| s.text("", x, Str::L32))
    }

    fn fixup(&mut self) {
        self.counts[0] = self.groups.len() as u32;
        self.counts[1] = self.links.len() as u32;
        for (c, m) in self.counts[2..].iter_mut().zip(&self.maps) {
            *c = m.len() as u32;
        }
    }
}

impl Data for WwiseIds {
    const ID: &'static str = "wwiseidtable";
    const ABOUT: &'static str = "Wwise id tables (bank groups, id links, maps, parameter names)";
    const EXT: &'static [&'static str] = &["bin"];
    fn io<S: Io>(&mut self, s: &mut S) -> Res<()> {
        self.walk(s)
    }
    fn fix(&mut self) -> Res<()> {
        self.fixup();
        Ok(())
    }
    fn size(&self) -> String {
        format!("{} groups, {} links, {} names", self.groups.len(), self.links.len(), self.names.len())
    }
    fn endian(d: &[u8]) -> Option<Endian> {
        match d.get(..4)? {
            [5, 0, 0, 0] => Some(Endian::Le),
            [0, 0, 0, 5] => Some(Endian::Be),
            _ => None,
        }
    }
}
