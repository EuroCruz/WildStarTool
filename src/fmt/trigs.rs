use super::Data;
use crate::io::{err, Io, Len, Mode, Show, Str};
use ac_core::{Endian, Res};

const RAIN: u32 = 0x976f_2d35;
const SOUND: u32 = 0x9ee2_6816;
const BOX: u32 = 0xa078_7fc8;
const ZONE: u32 = 0x0a0a_ff97;
const DISTRICT: u32 = 0xa13e_6f5f;
const POLYGON: u32 = 0xaf36_a899;
const SPAWN: u32 = 0x9c8e_92d2;

#[derive(Default)]
pub struct Trigs {
    list: Vec<Trig>,
}

#[derive(Default)]
struct Trig {
    kind: u32,
    id: u32,
    hash: u32,
    hash2: u32,
    byte: u8,
    flags: [u16; 2],
    values: [f32; 3],
    box_: [f32; 14],
    text: String,
    texts: Vec<String>,
    hashes: Vec<u32>,
    volumes: Vec<Vec<[f32; 3]>>,
    blob: Vec<u8>,
    pairs: [Vec<(Vec<u8>, u32)>; 2],
    groups: Vec<Vec<Sub>>,
}

#[derive(Default)]
struct Sub {
    text: Vec<u8>,
    hash: u32,
    values: [f32; 2],
    byte: u8,
}

fn volumes<S: Io>(s: &mut S, v: &mut Vec<Vec<[f32; 3]>>) -> Res<()> {
    s.list("Volumes", v, Len::U16, |s, p| s.list("", p, Len::U16, |s, x| s.vec("", x)))
}

fn text<S: Io>(s: &mut S, v: &mut Vec<u8>) -> Res<()> {
    if s.skip("Text", v.is_empty()) {
        return Ok(());
    }
    s.blob("Text", v, Len::U16)
}

fn pairs<S: Io>(s: &mut S, k: &str, v: &mut Vec<(Vec<u8>, u32)>, n: u32) -> Res<()> {
    let n = s.n(n);
    s.list(k, v, n, |s, p| {
        text(s, &mut p.0)?;
        s.or("Hash", &mut p.1, Show::Hash, 0)
    })
}

fn trig<S: Io>(s: &mut S, t: &mut Trig) -> Res<()> {
    s.hash("Type", &mut t.kind)?;
    if t.kind == SPAWN {
        return Ok(());
    }
    s.hash("Id", &mut t.id)?;
    match t.kind {
        RAIN => {
            s.f32("Value", &mut t.values[0])?;
            volumes(s, &mut t.volumes)?;
            s.hex("Hash", &mut t.hash)
        }
        SOUND => {
            s.or("Hash", &mut t.hash, Show::Hash, 0)?;
            s.or("Flag", &mut t.flags[0], Show::Dec, 0)?;
            s.f32("Value", &mut t.values[0])?;
            volumes(s, &mut t.volumes)?;
            text(s, &mut t.blob)?;
            s.or("Hash2", &mut t.hash2, Show::Hash, 0)?;
            let [a, b] = &mut t.pairs;
            pairs(s, "Sounds1", a, 5)?;
            s.f32("Value2", &mut t.values[1])?;
            s.f32("Value3", &mut t.values[2])?;
            let n = s.n(3);
            s.list("Groups", &mut t.groups, n, |s, g| {
                s.list("", g, Len::U16, |s, x| {
                    text(s, &mut x.text)?;
                    s.hash("Hash", &mut x.hash)?;
                    s.f32("Value", &mut x.values[0])?;
                    s.u8("Byte", &mut x.byte)?;
                    s.f32("Value2", &mut x.values[1])
                })
            })?;
            pairs(s, "Sounds2", b, 4)
        }
        BOX => {
            s.vec("Values", &mut t.box_)?;
            s.text("Text", &mut t.text, Str::L16Z)?;
            s.u16("Flag1", &mut t.flags[0])?;
            s.u16("Flag2", &mut t.flags[1])
        }
        ZONE => {
            s.f32("Value", &mut t.values[0])?;
            volumes(s, &mut t.volumes)?;
            s.hashes("Hashes", &mut t.hashes, Len::U32)?;
            let n = s.n(4);
            s.list("Texts", &mut t.texts, n, |s, x| s.text("", x, Str::L16Z))?;
            s.hash("Hash", &mut t.hash)?;
            s.u16("Flag", &mut t.flags[0])
        }
        DISTRICT => {
            s.opt("Byte", &mut t.byte, Show::Dec)?;
            s.f32("Value", &mut t.values[0])?;
            volumes(s, &mut t.volumes)?;
            s.hash("Hash", &mut t.hash)?;
            s.or("Flag", &mut t.flags[0], Show::Dec, 0)
        }
        POLYGON => {
            s.hash("Hash", &mut t.hash)?;
            s.or("Flag", &mut t.flags[0], Show::Dec, 0)?;
            s.f32("Value", &mut t.values[0])?;
            volumes(s, &mut t.volumes)?;
            s.text("Text", &mut t.text, Str::L16Z)?;
            s.u16("Flags", &mut t.flags[1])
        }
        k => err(format!("unknown trigger type 0x{k:08x}")),
    }
}

impl Trigs {
    fn walk<S: Io>(&mut self, s: &mut S) -> Res<()> {
        s.magic(b"JGRT")?;
        s.note("Triggers: Type = Box, Zone, District, Polygon, SoundTrigger, RainTrigger (SpawnRegion has no data).\nVolumes: shapes as lists of points { x, y, z } (the game uses up to 10 points per shape)");
        s.list("Triggers", &mut self.list, Len::U32, trig)
    }

    fn check(&self) -> Res<()> {
        for (i, t) in self.list.iter().enumerate() {
            let bad = match t.kind {
                SOUND => t.pairs[0].len() != 5 || t.pairs[1].len() != 4 || t.groups.len() != 3,
                ZONE => t.texts.len() != 4,
                _ => false,
            };
            if bad {
                return err(format!("Triggers[{}]: SoundTrigger needs 5 Sounds1, 3 Groups, 4 Sounds2; Zone needs 4 Texts", i + 1));
            }
        }
        Ok(())
    }
}

impl Data for Trigs {
    const ID: &'static str = "trigs";
    const ABOUT: &'static str = "world triggers (zones, sounds, rain, boxes)";
    const EXT: &'static [&'static str] = &["trigs"];
    fn io<S: Io>(&mut self, s: &mut S) -> Res<()> {
        if s.mode() == Mode::Write {
            self.check()?;
        }
        self.walk(s)
    }
    fn size(&self) -> String {
        format!("{} triggers", self.list.len())
    }
    fn endian(d: &[u8]) -> Option<Endian> {
        match d.get(..4)? {
            b"JGRT" => Some(Endian::Le),
            b"TRGJ" => Some(Endian::Be),
            _ => None,
        }
    }
}
