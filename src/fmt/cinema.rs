use super::Data;
use crate::io::{Io, Show};
use ac_core::{Endian, Res};

#[derive(Default)]
pub struct Cxa {
    anims: Vec<Anim>,
}

#[derive(Default)]
struct Anim {
    id: u32,
    elems: Vec<Elem>,
}

#[derive(Default)]
struct Elem {
    kind: u32,
    hashes: [u32; 3],
    values: [f32; 2],
    flags: [u16; 2],
    extra: [u32; 2],
}

enum Class {
    Anim,
    AnimTarget,
    Sound,
    Target,
    Expression,
    Other,
}

fn class(kind: u32) -> Class {
    match kind {
        0x1816_6555 => Class::Anim,
        0x7b79_4547 => Class::AnimTarget,
        0xdd62_ba1a => Class::Sound,
        0x180a_0045 => Class::Target,
        0x4fd7_b0dd | 0xc023_acd3 => Class::Expression,
        _ => Class::Other,
    }
}

fn flag<S: Io>(s: &mut S, k: &str, v: &mut u16) -> Res<()> {
    s.or(k, v, Show::Bool, 0)
}

fn elem<S: Io>(s: &mut S, x: &mut Elem) -> Res<()> {
    s.hash("Type", &mut x.kind)?;
    s.hash("Hash1", &mut x.hashes[0])?;
    s.hash("Hash2", &mut x.hashes[1])?;
    s.or("Hash3", &mut x.hashes[2], Show::Hash, 0)?;
    s.f32("Value1", &mut x.values[0])?;
    s.or("Value2", &mut x.values[1], Show::Dec, -1.0)?;
    match class(x.kind) {
        Class::Anim => {
            flag(s, "Flag1", &mut x.flags[0])?;
            flag(s, "Flag2", &mut x.flags[1])?;
            let mut f = f32::from_bits(x.extra[0]);
            s.f32("Value3", &mut f)?;
            x.extra[0] = f.to_bits();
        }
        Class::AnimTarget => {
            flag(s, "Flag1", &mut x.flags[0])?;
            s.hash("Hash4", &mut x.extra[0])?;
            flag(s, "Flag2", &mut x.flags[1])?;
        }
        Class::Sound | Class::Target => flag(s, "Flag1", &mut x.flags[0])?,
        Class::Expression => {
            s.hash("Hash4", &mut x.extra[0])?;
            s.hex("Value3", &mut x.extra[1])?;
        }
        Class::Other => {}
    }
    Ok(())
}

impl Cxa {
    fn walk<S: Io>(&mut self, s: &mut S) -> Res<()> {
        s.konst(9u16)?;
        s.note("Complex animations: Id, then Elements in order.\nType picks the element class (Animation, Animation_Target, Say, Sound, Lookat_Target, Listener, Expression...)");
        s.tagged("Anims", &mut self.anims, b"2AXC", b"DNEC", |s, a| {
            s.hash("Id", &mut a.id)?;
            s.tagged("Elements", &mut a.elems, b"MELE", b"DNEC", elem)
        })
    }
}

impl Data for Cxa {
    const ID: &'static str = "cxa";
    const ABOUT: &'static str = "complex animations for cinematics and conversations";
    const EXT: &'static [&'static str] = &["cxa"];
    fn io<S: Io>(&mut self, s: &mut S) -> Res<()> {
        self.walk(s)
    }
    fn size(&self) -> String {
        format!("{} animations", self.anims.len())
    }
    fn endian(d: &[u8]) -> Option<Endian> {
        match d.get(..2)? {
            [9, 0] => Some(Endian::Le),
            [0, 9] => Some(Endian::Be),
            _ => None,
        }
    }
}
