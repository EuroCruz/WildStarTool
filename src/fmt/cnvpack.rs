use super::{endian_name, header, meta, meta_endian, Format, Input, Opts, Out, Sink};
use crate::io::{err, float, hexs, unfloat, unhexs};
use crate::names;
use ac_core::{Endian, Error, Reader, Res, Writer};
use ac_lua::Val;
use std::path::{Path, PathBuf};

const VERSION: u32 = 10;

fn tag(s: &str) -> u32 {
    u32::from_be_bytes(s.as_bytes().try_into().unwrap_or_default())
}

fn tag_name(t: u32) -> Option<String> {
    let b = t.to_be_bytes();
    b.iter().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()).then(|| String::from_utf8_lossy(&b).into_owned())
}

fn layout(t: u32, line: bool) -> Option<&'static str> {
    let shared = match &t.to_be_bytes() {
        b"NEXT" => Some("h"),
        b"SCR0" | b"SCR1" => Some("b"),
        b"LINE" => Some("L"),
        _ => None,
    };
    if shared.is_some() {
        return shared;
    }
    if line {
        Some(match &t.to_be_bytes() {
            b"SPKR" | b"NAME" | b"TGRT" | b"TXTC" => "u",
            b"TXT2" => "ux",
            b"CNDT" => "C",
            b"DELA" => "f",
            b"INF2" => "uuuuf",
            b"INFO" => "uuuf",
            b"INF3" => "uuuu",
            b"INF4" => "uuuuy",
            _ => "",
        })
    } else {
        Some(match &t.to_be_bytes() {
            b"HMA2" => "uuf",
            b"CAM1" | b"FILE" | b"SLOC" => "u",
            b"DIST" | b"SDST" => "f",
            b"HMA5" => "uuuy",
            b"HMA3" => "uufu",
            b"HMA4" => "uuu",
            b"HTXT" => "uu",
            b"SN3D" => "h",
            b"SBNK" => "b",
            _ => "",
        })
    }
}

#[derive(Clone)]
enum V {
    F(f32),
    U(u32),
    X(u32),
    H(u16),
    Y(u8),
    B(Vec<u8>),
    C(u16, Option<Vec<u8>>, u16),
    L(u16, Block),
}

#[derive(Clone, Default)]
struct Block {
    head: u16,
    items: Vec<(u32, Vec<V>)>,
}

struct Conv {
    id: u32,
    first: u16,
    body: Block,
}

fn read_block(r: &mut Reader, line: bool) -> Res<Block> {
    let mut b = Block { head: r.u16()?, items: Vec::new() };
    loop {
        let t = r.u32()?;
        if t == tag("CEND") {
            return Ok(b);
        }
        let lay = layout(t, line).unwrap_or("");
        let mut v = Vec::new();
        for c in lay.chars() {
            v.push(match c {
                'f' => V::F(r.f32()?),
                'u' => V::U(r.u32()?),
                'x' => V::X(r.u32()?),
                'h' => V::H(r.u16()?),
                'y' => V::Y(r.u8()?),
                'b' => {
                    let n = r.u16()? as usize;
                    V::B(r.take(n)?.to_vec())
                }
                'C' => {
                    let k = r.u16()?;
                    if k == 6 {
                        let n = r.u16()?;
                        if n > 1 {
                            V::C(k, Some(r.take(n as usize)?.to_vec()), n)
                        } else {
                            V::C(k, None, n)
                        }
                    } else {
                        V::C(k, None, 0)
                    }
                }
                _ => {
                    let i = r.u16()?;
                    V::L(i, read_block(r, true)?)
                }
            });
        }
        b.items.push((t, v));
    }
}

fn write_block(w: &mut Writer, b: &Block) {
    w.u16(b.head);
    for (t, v) in &b.items {
        w.u32(*t);
        for x in v {
            match x {
                V::F(f) => {
                    w.f32(*f);
                }
                V::U(u) | V::X(u) => {
                    w.u32(*u);
                }
                V::H(h) => {
                    w.u16(*h);
                }
                V::Y(y) => {
                    w.u8(*y);
                }
                V::B(b) => {
                    w.u16(b.len() as u16).bytes(b);
                }
                V::C(k, s, n) => {
                    w.u16(*k);
                    if *k == 6 {
                        match s {
                            Some(s) => w.u16(s.len() as u16).bytes(s),
                            None => w.u16(*n),
                        };
                    }
                }
                V::L(i, b) => {
                    w.u16(*i);
                    write_block(w, b);
                }
            }
        }
    }
    w.u32(tag("CEND"));
}

fn blob(b: &[u8]) -> Val {
    match b.split_last() {
        Some((0, s)) if s.iter().all(|c| (0x20..0x7f).contains(c)) => Val::Str(String::from_utf8_lossy(s).into_owned()),
        _ => hexs(b),
    }
}

fn unblob(v: &Val) -> Option<Vec<u8>> {
    match v {
        Val::Str(s) => Some([s.as_bytes(), &[0]].concat()),
        v => unhexs(v),
    }
}

fn hash_val(h: u32) -> Val {
    match names::show(h) {
        Val::Str(s) => Val::Str(s),
        _ => Val::Raw(format!("0x{h:08x}")),
    }
}

fn one(x: &V) -> Val {
    match x {
        V::F(f) => float(*f),
        V::U(u) => hash_val(*u),
        V::X(u) => Val::Raw(format!("0x{u:08x}")),
        V::H(h) => Val::Int(*h as i64),
        V::Y(y) => Val::Int(*y as i64),
        V::B(b) => blob(b),
        V::C(k, s, n) => match (k, s) {
            (6, Some(s)) => Val::Tbl(vec![(None, Val::Int(6)), (None, blob(s))]),
            (6, None) => Val::Tbl(vec![(None, Val::Int(6)), (None, Val::Int(*n as i64))]),
            _ => Val::Int(*k as i64),
        },
        V::L(i, b) => dump_block(b, Some(*i)),
    }
}

fn dump_block(b: &Block, index: Option<u16>) -> Val {
    let mut t = Vec::new();
    if let Some(i) = index {
        t.push((None, Val::Int(i as i64)));
    }
    t.push((None, Val::Int(b.head as i64)));
    for (k, v) in &b.items {
        let key = tag_name(*k).unwrap_or_else(|| format!("0x{k:08x}"));
        let val = match v.len() {
            0 => Val::Bool(true),
            1 => one(&v[0]),
            _ => Val::Tbl(v.iter().map(|x| (None, one(x))).collect()),
        };
        t.push((Some(key), val));
    }
    Val::Tbl(t)
}

fn num(v: &Val) -> Option<u32> {
    match v {
        Val::Str(s) => Some(names::key(s)),
        Val::Raw(r) => u32::from_str_radix(r.strip_prefix("0x")?, 16).ok(),
        v => v.int().and_then(|i| u32::try_from(i).ok()),
    }
}

fn int<T: TryFrom<i64>>(v: &Val) -> Option<T> {
    v.int().and_then(|i| T::try_from(i).ok())
}

fn load_block(v: &Val, line: bool) -> Result<(Option<u16>, Block), String> {
    let pos: Vec<&Val> = v.items().iter().filter(|(k, _)| k.is_none()).map(|(_, x)| x).collect();
    let (index, head) = match (line, pos.as_slice()) {
        (true, [i, h, ..]) => (Some(int(i).ok_or("line index must be a number")?), *h),
        (false, [h, ..]) => (None, *h),
        _ => return Err("needs the leading numbers (line index and/or count)".into()),
    };
    let mut b = Block { head: int(head).ok_or("count must be a number")?, items: Vec::new() };
    for (k, x) in v.items() {
        let Some(k) = k else { continue };
        let t = match k.strip_prefix("0x") {
            Some(h) => u32::from_str_radix(h, 16).map_err(|_| format!("{k}: bad tag"))?,
            None if k.len() == 4 => tag(k),
            None => return Err(format!("{k}: tags are 4 letters like SPKR")),
        };
        let lay = layout(t, line).unwrap_or("");
        let parts: Vec<&Val> = match (lay.len(), x) {
            (0, _) => Vec::new(),
            (1, x) => vec![x],
            (_, x) => x.items().iter().map(|(_, y)| y).collect(),
        };
        if parts.len() != lay.len() {
            return Err(format!("{k}: needs {} values", lay.len()));
        }
        let bad = || format!("{k}: wrong value");
        let mut vals = Vec::new();
        for (c, p) in lay.chars().zip(parts) {
            vals.push(match c {
                'f' => V::F(unfloat(p).ok_or_else(bad)?),
                'u' => V::U(num(p).ok_or_else(bad)?),
                'x' => V::X(num(p).ok_or_else(bad)?),
                'h' => V::H(int(p).ok_or_else(bad)?),
                'y' => V::Y(int(p).ok_or_else(bad)?),
                'b' => V::B(unblob(p).ok_or_else(bad)?),
                'C' => match p {
                    Val::Tbl(t) => match t.as_slice() {
                        [(None, k), (None, Val::Int(n))] if int::<u16>(k) == Some(6) => V::C(6, None, u16::try_from(*n).map_err(|_| bad())?),
                        [(None, k), (None, s)] if int::<u16>(k) == Some(6) => V::C(6, Some(unblob(s).ok_or_else(bad)?), 0),
                        _ => return Err(bad()),
                    },
                    p => V::C(int(p).ok_or_else(bad)?, None, 0),
                },
                _ => {
                    let (i, nb) = load_block(p, true).map_err(|m| format!("{k}: {m}"))?;
                    V::L(i.unwrap_or(0), nb)
                }
            });
        }
        b.items.push((t, vals));
    }
    Ok((index, b))
}

fn parse(d: &[u8]) -> Res<(Vec<Conv>, Endian)> {
    let e = match d.get(..4) {
        Some([10, 0, 0, 0]) => Endian::Le,
        Some([0, 0, 0, 10]) => Endian::Be,
        _ => return err("not a conversations pack (version is not 10)"),
    };
    let mut r = Reader::new(d, e);
    r.skip(4)?;
    let n = r.u32()? as usize;
    let mut v = Vec::with_capacity(n);
    for i in 0..n {
        let size = r.u32()? as usize;
        let start = r.pos();
        let at = |m: Error| Error::Msg(format!("conversation {}: {m}", i + 1));
        if r.u16().map_err(at)? != VERSION as u16 {
            return Err(at(Error::Bad("version is not 10")));
        }
        let id = r.u32().map_err(at)?;
        let first = r.u16().map_err(at)?;
        let body = read_block(&mut r, false).map_err(at)?;
        if r.pos() - start != size {
            return Err(at(Error::Bad("size does not match the contents")));
        }
        v.push(Conv { id, first, body });
    }
    match r.left() {
        0 => Ok((v, e)),
        n => err(format!("{n} unexpected bytes at the end")),
    }
}

fn write(v: &[Conv], e: Endian) -> Vec<u8> {
    let mut w = Writer::new(e);
    w.u32(VERSION).u32(v.len() as u32);
    for c in v {
        let mut b = Writer::new(e);
        b.u16(VERSION as u16).u32(c.id).u16(c.first);
        write_block(&mut b, &c.body);
        let b = b.finish();
        w.u32(b.len() as u32).bytes(&b);
    }
    w.finish()
}

pub struct CnvPack;

impl Format for CnvPack {
    fn id(&self) -> &'static str {
        "cnvpack"
    }
    fn about(&self) -> &'static str {
        "conversations (dialog trees: speakers, lines, conditions)"
    }
    fn exts(&self) -> &'static [&'static str] {
        &["cnvpack"]
    }
    fn probe(&self, i: &mut Input) -> bool {
        i.len < 64 << 20 && i.all().is_ok_and(|d| parse(d).is_ok())
    }
    fn info(&self, i: &mut Input) -> Res<Vec<(&'static str, String)>> {
        let (v, e) = parse(i.all()?)?;
        Ok(vec![("contents", format!("{} conversations", v.len())), ("byte order", endian_name(e).into())])
    }
    fn unpack(&self, i: &mut Input, o: &mut Out) -> Res<PathBuf> {
        let (v, e) = parse(i.all()?)?;
        let name = i.name.clone();
        let mut t = header(&name, self.about());
        t.extend(meta(self.id(), &name, e));
        t.push((None, Val::Note("Conversations: { id, number, number, then tags in game order }. Tags are 4-letter names;
LINE = { line index, number, tags... } (lines nest). TXT2 = { voice, text id from gametext.dlg }. A tag with no value is true".into())));
        let list = v
            .iter()
            .map(|c| {
                let Val::Tbl(mut b) = dump_block(&c.body, None) else { unreachable!() };
                b.insert(0, (None, Val::Int(c.first as i64)));
                b.insert(0, (None, hash_val(c.id)));
                (None, Val::Tbl(b))
            })
            .collect();
        t.push((Some("Conversations".into()), Val::Tbl(list)));
        o.text(&format!("{name}.lua"), &Val::Tbl(t))
    }
    fn pack(&self, src: &Path, meta: &Val, opts: Opts, out: &mut Sink) -> Res<()> {
        let e = meta_endian(meta, opts);
        let mut v = Vec::new();
        for (i, (_, x)) in meta.key("Conversations").ok_or(Error::Bad("no Conversations = { ... }"))?.items().iter().filter(|(_, x)| !matches!(x, Val::Note(_))).enumerate() {
            let at = |m: String| Error::Msg(format!("{}: Conversations[{}]: {m}", src.display(), i + 1));
            let items = x.items();
            let pos: Vec<&Val> = items.iter().filter(|(k, _)| k.is_none()).map(|(_, y)| y).collect();
            let id = pos.first().and_then(|p| num(p)).ok_or_else(|| at("first value must be the conversation id".into()))?;
            let first = pos.get(1).and_then(|p| int(p)).ok_or_else(|| at("second value must be a number".into()))?;
            let third = pos.get(2).ok_or_else(|| at("third value must be a number".into()))?;
            let mut rest = vec![(None, (*third).clone())];
            rest.extend(items.iter().filter(|(k, _)| k.is_some()).cloned());
            let (_, body) = load_block(&Val::Tbl(rest), false).map_err(at)?;
            v.push(Conv { id, first, body });
        }
        out.put(&write(&v, e))
    }
}
