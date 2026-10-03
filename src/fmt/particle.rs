use super::Data;
use crate::io::{Io, Len, Show, Str};
use ac_core::{Endian, Res};

const FX00: u32 = 0x4658_3030;
const FX01: u32 = 0x4658_3031;
const TRACKS: [(&str, usize); 5] = [("Track1", 3), ("Track2", 1), ("Track3", 1), ("Track4", 1), ("Track5", 4)];

#[derive(Default)]
pub struct Particles {
    version: u32,
    effects: Vec<Effect>,
}

#[derive(Default)]
struct Effect {
    id: u32,
    emitters: Vec<Emitter>,
}

#[derive(Default)]
struct Emitter {
    name: String,
    values: [f32; 16],
    hash: u32,
    flags: u16,
    data: Vec<u8>,
    shape: [u16; 2],
    shape_values: [f32; 15],
    shape_extra: f32,
    shape_flags: u16,
    pairs: Vec<(u32, f32)>,
    children: Vec<Emitter>,
    kind: u16,
    range: Vec<f32>,
    curve: [f32; 28],
    curve_flag: u16,
    value: f32,
    tracks: [Vec<Key>; 5],
}

#[derive(Default)]
struct Key {
    time: f32,
    mode: u8,
    vals: Vec<f32>,
}

fn range_len(kind: u16) -> u32 {
    match kind {
        1 | 3 => 6,
        2 | 4 => 18,
        _ => 0,
    }
}

fn emitter<S: Io>(s: &mut S, x: &mut Emitter, fx01: bool) -> Res<()> {
    s.text("Name", &mut x.name, Str::L16Z)?;
    s.vec("Values", &mut x.values)?;
    s.hash("Hash", &mut x.hash)?;
    s.val("Flags", &mut x.flags, Show::Hex)?;
    if !s.skip("Effect", x.data.is_empty()) {
        s.blob("Effect", &mut x.data, Len::U16)?;
    }
    s.node("Shape", |s| {
        s.val("Kind", &mut x.shape, Show::Dec)?;
        s.vec("Values", &mut x.shape_values)?;
        if fx01 {
            s.f32("Extra", &mut x.shape_extra)?;
        }
        s.u16("Flags", &mut x.shape_flags)?;
        s.list("Pairs", &mut x.pairs, Len::U16, |s, p| {
            s.hash("Hash", &mut p.0)?;
            s.f32("Value", &mut p.1)
        })?;
        s.list("Children", &mut x.children, Len::U8, |s, c| emitter(s, c, fx01))
    })?;
    s.node("Curve", |s| {
        s.u16("Kind", &mut x.kind)?;
        let n = s.n(range_len(x.kind));
        s.floats("Range", &mut x.range, n)?;
        s.vec("Values", &mut x.curve)?;
        s.u16("Flag", &mut x.curve_flag)
    })?;
    s.node("Tracks", |s| {
        s.f32("Value", &mut x.value)?;
        for ((k, n), t) in TRACKS.iter().zip(&mut x.tracks) {
            s.list(k, t, Len::U8, |s, key| {
                s.f32("Time", &mut key.time)?;
                s.u8("Mode", &mut key.mode)?;
                let m = s.n(*n as u32);
                s.floats("Values", &mut key.vals, m)
            })?;
        }
        Ok(())
    })
}

impl Particles {
    fn walk<S: Io>(&mut self, s: &mut S) -> Res<()> {
        s.or("Version", &mut self.version, Show::Hex, FX01)?;
        if self.version != FX00 && self.version != FX01 {
            return crate::io::err("not a particle pack (no FX00/FX01 magic)");
        }
        let fx01 = self.version == FX01;
        s.note("Effects: Id, then its Emitters. Values, Hash, Flags, Shape, Curve and Tracks are read by the game in this order;\nfield meanings are not confirmed. Tracks: keys { Time, Mode (1, 2 or 3), Values }. Always big-endian");
        s.list("Effects", &mut self.effects, Len::U32, |s, x| {
            s.hash("Id", &mut x.id)?;
            s.list("Emitters", &mut x.emitters, Len::U8, |s, e| emitter(s, e, fx01))
        })
    }
}

impl Data for Particles {
    const ID: &'static str = "particles";
    const ABOUT: &'static str = "particle effects";
    const EXT: &'static [&'static str] = &["pack"];
    fn io<S: Io>(&mut self, s: &mut S) -> Res<()> {
        self.walk(s)
    }
    fn size(&self) -> String {
        format!("{} effects", self.effects.len())
    }
    fn endian(d: &[u8]) -> Option<Endian> {
        matches!(d.get(..4)?, b"FX00" | b"FX01").then_some(Endian::Be)
    }
}
