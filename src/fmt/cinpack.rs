use super::arch::{clean, entry, entry_num, entry_path, uniq};
use super::{endian_name, header, meta, meta_endian, Format, Input, Item, Opts, Out, Sink, INDEX};
use crate::io::{err, float, unfloat};
use crate::names;
use ac_core::{Endian, Error, Reader, Res, Writer};
use ac_lua::Val;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

const VERSION: u32 = 12;
const CAMERA: u32 = 0x6090_527c;
const TRACK: u32 = 0x3049_baab;
const TRACK4: &[u8; 4] = b"4NRT";
const TRACK4_LAYOUT: &str = "uffufffuhh";
const KEY_TAGS: [&[u8; 4]; 4] = [b"TAMK", b"VOFK", b"4FDK", b"COCK"];
const FLAGS: [&[u8; 4]; 5] = [b"PIKS", b"LOTN", b"YLPA", b"SUAP", b"DMCL"];

const LAYOUTS: &[(u32, &str)] = &[
    (0x6a8f_7467, "ffuuh"),
    (0x427c_c1f8, "ffsh"),
    (0x2b94_1a68, "fu"),
    (0x1693_0afe, "fuu"),
    (0x0558_6466, "ffffh"),
    (0x1290_2319, "ffu"),
    (0x1afc_f1ce, ""),
    (0x1bd8_8db7, "fffusu"),
    (0x3c7a_73eb, "ffuuuhhh"),
    (0x3b74_3523, "fh"),
    (0x3f0b_90fc, "fuu"),
    (0x412d_1576, "fufff"),
    (0x5537_7182, "fu"),
    (0x4a09_e56c, "ffuuuu"),
    (0x4867_25bb, "ffu"),
    (0x4b39_ed5c, "fff"),
    (0x4d76_49ec, "fsh"),
    (0x60d1_bb07, "fu"),
    (0x57ba_4f5a, "u"),
    (0x65c5_2b0f, "fuu"),
    (0x6636_4edb, "ffff"),
    (0xf494_70ba, "ffff"),
    (0x7447_097d, ""),
    (0x6dc9_d9b2, ""),
    (0x6d17_d447, "u"),
    (0x6fee_f89f, "fhuh"),
    (0x7213_e378, "ffuuf"),
    (0xb4d9_b99b, "ffuuuu"),
    (0x9990_edd9, "fuu"),
    (0x8d1b_88c3, "fffusu"),
    (0x86bf_6c5b, "fuuu"),
    (0x84b6_25dc, "fs"),
    (0x7bbe_9a2b, "fh"),
    (0x9cc8_6fe3, "fs"),
    (0x9f8b_ca10, "ffs"),
    (0xa9de_d8d5, "fs"),
    (0xebb1_1a67, "fuu"),
    (0xe6ce_9b69, "ffuu"),
    (0xda5c_4b83, ""),
    (0xcba9_18ea, "f"),
    (0xd0b1_07bb, "ffshu"),
    (0xd98b_7848, "fff"),
    (CAMERA, "ffu"),
    (TRACK, TRACK4_LAYOUT),
];

#[derive(Clone)]
enum V {
    F(f32),
    U(u32),
    H(u16),
    S(String),
}

#[derive(Default, Clone)]
struct Key {
    m: [f32; 12],
    fov: f32,
    dof: [f32; 4],
    coc: f32,
}

#[derive(Default, Clone)]
struct Elem {
    kind: u32,
    vals: Vec<V>,
    keys: Vec<Key>,
    tail: u32,
    flags: [bool; 5],
    capo: Option<f32>,
    frts: Option<u32>,
}

struct Cin {
    key: u32,
    flag: u8,
    elems: Vec<Elem>,
}

fn layout(kind: u32) -> Option<&'static str> {
    LAYOUTS.iter().find(|x| x.0 == kind).map(|x| x.1)
}

fn tag(t: &[u8; 4]) -> u32 {
    u32::from_le_bytes(*t)
}

fn names_of(lay: &str) -> Vec<String> {
    let count = |c| lay.chars().filter(|&x| x == c).count();
    let mut seen = [0usize; 4];
    lay.chars()
        .map(|c| {
            let (i, base) = match c {
                'f' => (0, "Value"),
                'u' => (1, "Hash"),
                'h' => (2, "Flag"),
                _ => (3, "Text"),
            };
            seen[i] += 1;
            let n = seen[i];
            match (c, count(c)) {
                ('f', 1) => "Time".into(),
                ('f', _) if n == 1 => "Start".into(),
                ('f', _) if n == 2 => "End".into(),
                ('f', _) => format!("{base}{}", n - 2),
                (_, 1) => base.into(),
                _ => format!("{base}{n}"),
            }
        })
        .collect()
}

fn read_vals(r: &mut Reader, lay: &str) -> Res<Vec<V>> {
    lay.chars()
        .map(|c| {
            Ok(match c {
                'f' => V::F(r.f32()?),
                'u' => V::U(r.u32()?),
                'h' => V::H(r.u16()?),
                _ => {
                    let n = r.u16()? as usize;
                    let b = r.take(n)?;
                    V::S(String::from_utf8_lossy(b.strip_suffix(&[0]).ok_or(Error::Bad("text is not zero-terminated"))?).into_owned())
                }
            })
        })
        .collect()
}

fn read_elem(r: &mut Reader, kind: u32) -> Res<Elem> {
    let mut x = Elem { kind, ..Default::default() };
    if kind == TRACK {
        if r.u32()? != tag(TRACK4) {
            return err("only the 4NRT track layout is known");
        }
    }
    let lay = layout(kind).ok_or_else(|| Error::Msg(format!("unknown element type 0x{kind:08x}")))?;
    x.vals = read_vals(r, lay)?;
    if kind == CAMERA {
        let n = match x.vals.pop() {
            Some(V::U(n)) => n as usize,
            _ => 0,
        };
        for _ in 0..n {
            let mut k = Key::default();
            for (i, t) in KEY_TAGS.iter().enumerate() {
                if r.u32()? != tag(t) {
                    return err("camera key: unexpected tag");
                }
                match i {
                    0 => k.m.iter_mut().try_for_each(|v| r.f32().map(|f| *v = f))?,
                    1 => k.fov = r.f32()?,
                    2 => k.dof.iter_mut().try_for_each(|v| r.f32().map(|f| *v = f))?,
                    _ => k.coc = r.f32()?,
                }
            }
            if r.u32()? != tag(b"DNEK") {
                return err("camera key: missing KEND");
            }
            x.keys.push(k);
        }
        x.tail = r.u32()?;
    }
    loop {
        let t = r.u32()?;
        if t == tag(b"DNEC") {
            return Ok(x);
        }
        if let Some(i) = FLAGS.iter().position(|f| tag(f) == t) {
            x.flags[i] = true;
        } else if t == tag(b"CAPO") {
            x.capo = Some(r.f32()?);
        } else if t == tag(b"FRTS") {
            x.frts = Some(r.u32()?);
        } else {
            return err(format!("unexpected tag 0x{t:08x} inside an element"));
        }
    }
}

fn read_cin(d: &[u8], at: usize, e: Endian) -> Res<(u32, Vec<Elem>, usize)> {
    let mut r = Reader::at(d, e, at)?;
    if r.u16()? != VERSION as u16 {
        return err("cinematic does not start with version 12");
    }
    let id = r.u32()?;
    let mut v = Vec::new();
    loop {
        let t = r.u32()?;
        if t == tag(b"DNEC") {
            return Ok((id, v, r.pos()));
        }
        v.push(read_elem(&mut r, t).map_err(|m| Error::Msg(format!("element {}: {m}", v.len() + 1)))?);
    }
}

fn write_vals(w: &mut Writer, v: &[V]) {
    for x in v {
        match x {
            V::F(f) => w.f32(*f),
            V::U(u) => w.u32(*u),
            V::H(h) => w.u16(*h),
            V::S(s) => w.u16(s.len() as u16 + 1).bytes(s.as_bytes()).u8(0),
        };
    }
}

fn write_cin(w: &mut Writer, c: &Cin) {
    let t = |w: &mut Writer, x: &[u8; 4]| {
        w.u32(tag(x));
    };
    w.u16(VERSION as u16).u32(c.key);
    for x in &c.elems {
        w.u32(x.kind);
        if x.kind == TRACK {
            t(w, TRACK4);
        }
        write_vals(w, &x.vals);
        if x.kind == CAMERA {
            w.u32(x.keys.len() as u32);
            for k in &x.keys {
                t(w, KEY_TAGS[0]);
                k.m.iter().for_each(|f| {
                    w.f32(*f);
                });
                t(w, KEY_TAGS[1]);
                w.f32(k.fov);
                t(w, KEY_TAGS[2]);
                k.dof.iter().for_each(|f| {
                    w.f32(*f);
                });
                t(w, KEY_TAGS[3]);
                w.f32(k.coc);
                t(w, b"DNEK");
            }
            w.u32(x.tail);
        }
        for (i, f) in FLAGS.iter().enumerate() {
            if x.flags[i] {
                t(w, f);
            }
        }
        if let Some(c) = x.capo {
            t(w, b"CAPO");
            w.f32(c);
        }
        if let Some(f) = x.frts {
            t(w, b"FRTS");
            w.u32(f);
        }
        t(w, b"DNEC");
    }
    t(w, b"DNEC");
}

fn hash_val(h: u32) -> Val {
    match names::show(h) {
        Val::Str(s) => Val::Str(s),
        _ => Val::Raw(format!("0x{h:08x}")),
    }
}

fn floats(v: &[f32]) -> Val {
    Val::Tbl(v.iter().map(|f| (None, float(*f))).collect())
}

fn dump_elem(x: &Elem) -> Val {
    let mut t = vec![(Some("Type".to_string()), hash_val(x.kind))];
    let lay = layout(x.kind).unwrap_or("");
    let lay = if x.kind == CAMERA { &lay[..2] } else { lay };
    for (n, v) in names_of(lay).into_iter().zip(&x.vals) {
        let v = match v {
            V::F(f) => float(*f),
            V::U(u) => hash_val(*u),
            V::H(h) => Val::Int(*h as i64),
            V::S(s) => Val::Str(s.clone()),
        };
        t.push((Some(n), v));
    }
    for (i, f) in FLAGS.iter().enumerate() {
        if x.flags[i] {
            t.push((Some(flag_name(f)), Val::Bool(true)));
        }
    }
    if let Some(c) = x.capo {
        t.push((Some("CAPO".into()), float(c)));
    }
    if let Some(f) = x.frts {
        t.push((Some("FRTS".into()), Val::Raw(format!("0x{f:08x}"))));
    }
    if x.kind == CAMERA {
        t.push((Some("Tail".into()), Val::Raw(format!("0x{:08x}", x.tail))));
        let keys = x.keys.iter().map(|k| (None, Val::Tbl(vec![(None, floats(&k.m)), (None, float(k.fov)), (None, floats(&k.dof)), (None, float(k.coc))]))).collect();
        t.push((Some("Keys".into()), Val::Tbl(keys)));
    }
    Val::Tbl(t)
}

fn flag_name(f: &[u8; 4]) -> String {
    f.iter().rev().map(|&c| c as char).collect()
}

fn num(v: &Val) -> Option<u32> {
    match v {
        Val::Str(s) => Some(names::key(s)),
        Val::Raw(r) => u32::from_str_radix(r.strip_prefix("0x")?, 16).ok(),
        v => v.int().and_then(|i| u32::try_from(i).ok()),
    }
}

fn fl(v: &Val, n: usize) -> Option<Vec<f32>> {
    let l: Option<Vec<f32>> = v.items().iter().map(|(_, x)| unfloat(x)).collect();
    l.filter(|l| l.len() == n)
}

fn load_elem(v: &Val) -> Result<Elem, String> {
    let kind = v.key("Type").and_then(num).ok_or("needs Type = \"element name\" or 0x...")?;
    let lay = layout(kind).ok_or(format!("unknown element type 0x{kind:08x}"))?;
    let lay = if kind == CAMERA { &lay[..2] } else { lay };
    let mut x = Elem { kind, ..Default::default() };
    for (n, c) in names_of(lay).into_iter().zip(lay.chars()) {
        let f = v.key(&n).ok_or(format!("missing {n}"))?;
        let bad = || format!("{n}: wrong value");
        x.vals.push(match c {
            'f' => V::F(unfloat(f).ok_or_else(bad)?),
            'u' => V::U(num(f).ok_or_else(bad)?),
            'h' => V::H(f.int().and_then(|i| u16::try_from(i).ok()).ok_or_else(bad)?),
            _ => V::S(f.str().ok_or_else(bad)?.to_string()),
        });
    }
    for (i, f) in FLAGS.iter().enumerate() {
        x.flags[i] = matches!(v.key(&flag_name(f)), Some(Val::Bool(true)));
    }
    x.capo = v.key("CAPO").map(|c| unfloat(c).ok_or("CAPO must be a number")).transpose()?;
    x.frts = v.key("FRTS").map(|c| num(c).ok_or("FRTS must be a number")).transpose()?;
    if kind == CAMERA {
        x.tail = v.key("Tail").and_then(num).unwrap_or(0);
        for (i, (_, k)) in v.key("Keys").map(Val::items).unwrap_or(&[]).iter().enumerate() {
            let it = k.items();
            let get = |j: usize| it.get(j).map(|x| &x.1);
            let bad = || format!("Keys[{}] must be {{ {{12 numbers}}, fov, {{4 numbers}}, coc }}", i + 1);
            let m = get(0).and_then(|x| fl(x, 12)).ok_or_else(bad)?;
            let dof = get(2).and_then(|x| fl(x, 4)).ok_or_else(bad)?;
            x.keys.push(Key {
                m: m.try_into().map_err(|_| bad())?,
                fov: get(1).and_then(unfloat).ok_or_else(bad)?,
                dof: dof.try_into().map_err(|_| bad())?,
                coc: get(3).and_then(unfloat).ok_or_else(bad)?,
            });
        }
    }
    Ok(x)
}

fn head(i: &mut Input) -> Res<(Vec<(u32, u32, u8)>, Endian)> {
    let h = i.head(8);
    let e = match h.get(..4) {
        Some([12, 0, 0, 0]) => Endian::Le,
        Some([0, 0, 0, 12]) => Endian::Be,
        _ => return err("not a cinematics pack (version is not 12)"),
    };
    let n = e.get::<u32>(&h[4..]).unwrap_or(0) as usize;
    if 8 + n as u64 * 9 > i.len {
        return err("table is larger than the file");
    }
    let t = i.at(8, n * 9)?;
    let mut r = Reader::new(&t, e);
    let v = r.list(n, |r| Ok((r.u32()?, r.u32()?, r.u8()?)))?;
    if v.iter().any(|x| x.1 as u64 >= i.len) {
        return err("a cinematic starts past the end of the file");
    }
    Ok((v, e))
}

pub struct CinPack;

impl Format for CinPack {
    fn id(&self) -> &'static str {
        "cinpack"
    }
    fn about(&self) -> &'static str {
        "in-game cinematics (camera, animations, sounds, events)"
    }
    fn exts(&self) -> &'static [&'static str] {
        &["cinpack"]
    }
    fn folder(&self) -> bool {
        true
    }
    fn probe(&self, i: &mut Input) -> bool {
        head(i).is_ok()
    }
    fn info(&self, i: &mut Input) -> Res<Vec<(&'static str, String)>> {
        let (v, e) = head(i)?;
        Ok(vec![("contents", format!("{} cinematics", v.len())), ("byte order", endian_name(e).into())])
    }
    fn list(&self, i: &mut Input) -> Res<Vec<Item>> {
        Ok(head(i)?.0.iter().map(|x| Item { name: names::label(x.0), size: 0, note: String::new() }).collect())
    }
    fn unpack(&self, i: &mut Input, o: &mut Out) -> Res<PathBuf> {
        let (mut v, e) = head(i)?;
        v.sort_by_key(|x| x.1);
        let d = i.all()?.to_vec();
        let mut used = HashSet::new();
        let mut list = Vec::new();
        let mut at = 8 + v.len() * 9;
        for &(key, start, flag) in &v {
            if start as usize != at {
                return err(format!("cinematic 0x{key:08x} is not where the table says"));
            }
            let (id, elems, end) = read_cin(&d, at, e).map_err(|m| Error::Msg(format!("{}: {m}", names::label(key))))?;
            if id != key {
                return err(format!("{}: id inside does not match the table", names::label(key)));
            }
            at = end;
            let name = match names::show(key) {
                Val::Str(s) => s,
                _ => format!("0x{key:08x}"),
            };
            let rel = uniq(&mut used, &format!("{}.lua", clean(&name)));
            let mut extra = Vec::new();
            if names::hash(rel.trim_end_matches(".lua")) != key {
                extra.push((Some("Key".into()), Val::Raw(format!("0x{key:08x}"))));
            }
            if flag != 1 {
                extra.push((Some("Flag".into()), Val::Int(flag as i64)));
            }
            let mut t = header(&rel, "one cinematic");
            t.push((None, Val::Note("Elements in order: Type, then its values. Start/End or Time are seconds; Hash, Flag, Text, Value are\nread by the game (meaning not confirmed). CameraXSI Keys: { {3x4 matrix}, fov, {4 depth-of-field values}, coc }".into())));
            t.push((Some("Elements".into()), Val::Tbl(elems.iter().map(|x| (None, dump_elem(x))).collect())));
            o.text(&rel, &Val::Tbl(t))?;
            list.push((None, entry(&rel, extra)));
        }
        if at != d.len() {
            return err(format!("{} unexpected bytes at the end", d.len() - at));
        }
        let name = i.name.clone();
        let mut t = header(&name, self.about());
        t.extend(meta(self.id(), &name, e));
        t.push((None, Val::Note("Cinematics in pack order, one .lua file each (the file name is the cinematic name).\nNew .lua files are added at the end. Flag: table flag (default 1, meaning not confirmed)".into())));
        t.push((Some("Files".into()), Val::Tbl(list)));
        o.text(INDEX, &Val::Tbl(t))
    }
    fn pack(&self, src: &Path, meta: &Val, opts: Opts, out: &mut Sink) -> Res<()> {
        let e = meta_endian(meta, opts);
        let mut items = Vec::new();
        for (_, v) in meta.key("Files").ok_or(Error::Bad("index.lua has no Files = { ... }"))?.items().iter().filter(|(_, v)| !matches!(v, Val::Note(_))) {
            let rel = entry_path(v)?;
            let key = entry_num(v, "Key")?.unwrap_or_else(|| names::key(rel.trim_end_matches(".lua")));
            let flag = v.key("Flag").and_then(Val::int).unwrap_or(1) as u8;
            items.push((rel, key, flag));
        }
        let have: Vec<String> = items.iter().map(|x| x.0.to_lowercase()).collect();
        let mut fresh: Vec<String> = ac_core::fs::walk(src)?
            .iter()
            .map(|p| p.strip_prefix(src).unwrap_or(p).to_string_lossy().replace('\\', "/"))
            .filter(|r| r.to_lowercase().ends_with(".lua") && r != INDEX && !have.contains(&r.to_lowercase()))
            .collect();
        fresh.sort();
        for r in fresh {
            let key = names::key(r.trim_end_matches(".lua"));
            items.push((r, key, 1));
        }
        let mut cins = Vec::with_capacity(items.len());
        for (rel, key, flag) in &items {
            let at = |m: String| Error::Msg(format!("{rel}: {m}"));
            let v = super::read_text(&ac_core::fs::join_safe(src, rel)?)?;
            let mut elems = Vec::new();
            for (i, (_, x)) in v.key("Elements").ok_or_else(|| at("no Elements = { ... }".into()))?.items().iter().filter(|(_, x)| !matches!(x, Val::Note(_))).enumerate() {
                elems.push(load_elem(x).map_err(|m| at(format!("Elements[{}]: {m}", i + 1)))?);
            }
            cins.push(Cin { key: *key, flag: *flag, elems });
        }
        let mut body = Writer::new(e);
        let mut table = Vec::with_capacity(cins.len());
        let base = 8 + cins.len() * 9;
        for c in &cins {
            table.push((c.key, (base + body.pos()) as u32, c.flag));
            write_cin(&mut body, c);
        }
        table.sort_by_key(|x| x.0);
        if let Some(w) = table.windows(2).find(|w| w[0].0 == w[1].0) {
            return err(format!("two cinematics have the key 0x{:08x}", w[0].0));
        }
        let mut w = Writer::new(e);
        w.u32(VERSION).u32(table.len() as u32);
        for (k, at, f) in &table {
            w.u32(*k).u32(*at).u8(*f);
        }
        out.put(&w.finish())?;
        out.put(&body.finish())
    }
}
