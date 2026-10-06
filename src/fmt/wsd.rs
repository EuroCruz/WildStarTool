use super::{endian_name, header, meta, meta_endian, Format, Input, Item, Opts, Out, Sink};
use crate::io::{err, float, hexs, unfloat, unhexs};
use crate::names;
use ac_core::{Endian, Error, Reader, Res, Writer};
use ac_lua::Val;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Vec3,
    Text,
    Ref,
    Float,
    Int,
    Bool,
    Lua,
}

const KINDS: &[(&str, Kind)] = &[
    ("Position", Kind::Vec3),
    ("XAxis", Kind::Vec3),
    ("YAxis", Kind::Vec3),
    ("ZAxis", Kind::Vec3),
    ("ClassName", Kind::Text),
    ("Type", Kind::Text),
    ("Script", Kind::Text),
    ("Parent", Kind::Text),
    ("LuaTable", Kind::Ref),
    ("AttractionPt", Kind::Ref),
    ("ParentObject", Kind::Ref),
    ("AttachedID0", Kind::Ref),
    ("AttachedID1", Kind::Ref),
    ("AttachedID2", Kind::Ref),
    ("AttachedID3", Kind::Ref),
    ("ID0", Kind::Ref),
    ("ID1", Kind::Ref),
    ("ID2", Kind::Ref),
    ("ID3", Kind::Ref),
    ("Width", Kind::Float),
    ("AttachedCount", Kind::Int),
    ("StartDisabled", Kind::Bool),
    ("NeedsEnabled", Kind::Bool),
    ("LuaParam", Kind::Lua),
];

const LUA: [&str; 5] = ["LuaString", "LuaFloat", "LuaBool", "LuaCrc", "LuaCrcList"];

const VEC3: [&str; 5] = ["Position", "Tangent", "Node", "FenceNode", "Axis"];

fn kind(h: u32) -> Option<Kind> {
    if let Some(k) = KINDS.iter().find(|k| names::hash(k.0) == h) {
        return Some(k.1);
    }
    let n = names::name(h).filter(|n| names::hash(n) == h)?;
    let base = n.trim_end_matches(|c: char| c.is_ascii_digit());
    if VEC3.contains(&base) {
        Some(Kind::Vec3)
    } else if base.ends_with("Duration") || base.ends_with("Time") {
        Some(Kind::Float)
    } else {
        None
    }
}

fn key_name(h: u32) -> String {
    match names::name(h) {
        Some(n) if names::hash(&n) == h && ac_lua::data::pretty(&Val::Str(n.clone()), &Default::default()).len() == n.len() + 2 => n,
        _ => format!("0x{h:08x}"),
    }
}

fn key_hash(k: &str) -> u32 {
    k.strip_prefix("0x").and_then(|h| u32::from_str_radix(h, 16).ok()).filter(|_| k.len() == 10).unwrap_or_else(|| names::key(k))
}

fn text(b: &[u8]) -> Option<String> {
    let (z, s) = b.split_last()?;
    (*z == 0 && s.iter().all(|c| (0x20..0x7f).contains(c))).then(|| String::from_utf8_lossy(s).into_owned())
}

fn cstr(s: &str) -> Vec<u8> {
    let mut v = s.as_bytes().to_vec();
    v.push(0);
    v
}

fn lua_show(b: &[u8], e: Endian) -> Option<Val> {
    let z = b.iter().position(|&c| c == 0)?;
    let name = text(&b[..=z])?;
    let mut r = Reader::new(&b[z + 1..], e);
    let t = r.u32().ok()?;
    let p = r.rest();
    let v = match LUA.iter().position(|l| names::hash(l) == t)? {
        0 => Val::Str(text(p)?),
        1 if p.len() == 4 => float(e.get::<f32>(p)?),
        2 if p.len() == 1 && p[0] < 2 => Val::Bool(p[0] == 1),
        3 if p.len() == 4 => Val::Call("crc".into(), vec![names::show(e.get::<u32>(p)?)]),
        4 => {
            let mut r = Reader::new(p, e);
            let n = r.u32().ok()?;
            let mut l = Vec::new();
            for _ in 0..n {
                l.push((None, Val::Str(String::from_utf8_lossy(r.cstr().ok()?).into_owned())));
            }
            if r.left() != 0 {
                return None;
            }
            Val::Call("list".into(), vec![Val::Tbl(l)])
        }
        _ => return None,
    };
    Some(Val::Tbl(vec![(None, Val::Str(name)), (None, v)]))
}

fn lua_load(v: &Val, e: Endian) -> Option<Vec<u8>> {
    let [(None, Val::Str(name)), (None, x)] = v.items() else { return None };
    let mut w = Writer::new(e);
    w.bytes(&cstr(name));
    match x {
        Val::Str(s) => w.u32(names::hash(LUA[0])).bytes(&cstr(s)),
        Val::Bool(b) => w.u32(names::hash(LUA[2])).u8(*b as u8),
        Val::Call(n, _) if n == "crc" => w.u32(names::hash(LUA[3])).u32(crc(x)?),
        Val::Call(n, a) if n == "list" => {
            let l = a.first()?.items();
            w.u32(names::hash(LUA[4])).u32(l.len() as u32);
            for (_, s) in l {
                w.bytes(&cstr(s.str()?));
            }
            &mut w
        }
        v => w.u32(names::hash(LUA[1])).f32(unfloat(v)?),
    };
    Some(w.finish())
}

fn crc(v: &Val) -> Option<u32> {
    match v {
        Val::Call(n, a) if n == "crc" => match a.first()? {
            Val::Str(s) => Some(names::key(s)),
            v => num(v),
        },
        _ => None,
    }
}

fn nice(f: f32) -> bool {
    f.is_normal() && (1e-4..1e7).contains(&f.abs()) && format!("{f:?}").len() <= 10
}

fn num(v: &Val) -> Option<u32> {
    match v {
        Val::Raw(r) if r.starts_with("0x") => u32::from_str_radix(&r[2..], 16).ok(),
        v => v.int().and_then(|i| u32::try_from(i).ok()),
    }
}

fn vec3(b: &[u8], e: Endian) -> Val {
    Val::Tbl(b.chunks_exact(4).map(|c| (None, float(e.get::<f32>(c).unwrap_or(0.0)))).collect())
}

fn guess(b: &[u8], e: Endian) -> Option<Val> {
    match b.len() {
        1 if b[0] < 2 => Some(Val::Bool(b[0] == 1)),
        4 => {
            let x = e.get::<u32>(b)?;
            let f = f32::from_bits(x);
            if let Some(n) = names::name(x).filter(|n| x > 0xffff && names::hash(n) == x) {
                return Some(Val::Call("crc".into(), vec![Val::Str(n)]));
            }
            Some(if nice(f) { float(f) } else { Val::Int(x as i64) })
        }
        8 | 12 | 16 if b.chunks_exact(4).all(|c| e.get::<f32>(c).is_some_and(|f| f == 0.0 || nice(f))) => Some(vec3(b, e)),
        _ => text(b).map(Val::Str),
    }
}

fn show(h: u32, b: &[u8], e: Endian) -> Val {
    let v = match kind(h) {
        Some(Kind::Vec3) if b.len() == 12 => Some(vec3(b, e)),
        Some(Kind::Text) => text(b).map(Val::Str),
        Some(Kind::Ref) if b.len() == 4 => e.get::<u32>(b).map(names::show),
        Some(Kind::Float) if b.len() == 4 => e.get::<f32>(b).map(float),
        Some(Kind::Float) if b.len() % 4 == 0 => Some(vec3(b, e)),
        Some(Kind::Int) if b.len() == 4 => e.get::<u32>(b).map(|x| Val::Int(x as i64)),
        Some(Kind::Bool) if b.len() == 1 && b[0] < 2 => Some(Val::Bool(b[0] == 1)),
        Some(Kind::Lua) => lua_show(b, e),
        _ => guess(b, e),
    };
    match v {
        Some(v) if load(h, &v, e).as_deref() == Some(b) => v,
        _ => hexs(b),
    }
}

fn load(h: u32, v: &Val, e: Endian) -> Option<Vec<u8>> {
    let k = kind(h);
    let mut w = Writer::new(e);
    match v {
        Val::Call(n, _) if n == "hex" => return unhexs(v),
        _ if k == Some(Kind::Lua) => return lua_load(v, e),
        Val::Tbl(t) if !t.is_empty() => {
            for (_, x) in t {
                w.f32(unfloat(x)?);
            }
        }
        Val::Str(s) if k == Some(Kind::Ref) => {
            w.u32(names::key(s));
        }
        Val::Str(s) => return Some(cstr(s)),
        Val::Bool(b) => {
            w.u8(*b as u8);
        }
        Val::Raw(r) if k == Some(Kind::Float) || (!r.starts_with("0x") && r.contains(['.', 'e'])) => {
            w.f32(unfloat(v)?);
        }
        Val::Call(n, _) if n == "crc" => {
            w.u32(crc(v)?);
        }
        Val::Call(..) if k == Some(Kind::Float) || unfloat(v).is_some() => {
            w.f32(unfloat(v)?);
        }
        v => {
            w.u32(num(v)?);
        }
    }
    Some(w.finish())
}

fn read_params(r: &mut Reader) -> Res<Vec<(u32, Vec<u8>)>> {
    let k = r.u32()? as usize;
    if k * 8 > r.left() {
        return err("parameter count is larger than the data");
    }
    r.list(k, |r| {
        let (h, s) = (r.u32()?, r.u32()? as usize);
        Ok((h, r.take(s)?.to_vec()))
    })
}

fn write_params(w: &mut Writer, p: &[(u32, Vec<u8>)]) {
    w.u32(p.len() as u32);
    for (h, b) in p {
        w.u32(*h).u32(b.len() as u32).bytes(b);
    }
}

fn dump_params(p: &[(u32, Vec<u8>)], e: Endian) -> impl Iterator<Item = (Option<String>, Val)> + '_ {
    p.iter().map(move |(h, b)| (Some(key_name(*h)), show(*h, b, e)))
}

fn load_params<'a>(v: impl Iterator<Item = &'a (Option<String>, Val)>, e: Endian) -> Result<Vec<(u32, Vec<u8>)>, String> {
    v.map(|(k, x)| {
        let k = k.as_deref().ok_or("every value needs a name")?;
        let h = key_hash(k);
        load(h, x, e).map(|b| (h, b)).ok_or(format!("{k}: value does not fit this field"))
    })
    .collect()
}

struct Obj {
    id: u32,
    params: Vec<(u32, Vec<u8>)>,
}

fn endian(d: &[u8]) -> Option<Endian> {
    let n = |e: Endian| e.get::<u32>(d.get(4..8)?);
    if d.get(..4)? != [0; 4] {
        return None;
    }
    let small = |x: Option<u32>| x.is_some_and(|x| (x as usize) <= d.len() / 8);
    match (small(n(Endian::Le)), small(n(Endian::Be))) {
        (true, false) => Some(Endian::Le),
        (false, true) => Some(Endian::Be),
        (true, true) => Some(if n(Endian::Le) <= n(Endian::Be) { Endian::Le } else { Endian::Be }),
        _ => None,
    }
}

fn parse(d: &[u8]) -> Res<(Vec<Obj>, Endian)> {
    let e = endian(d).ok_or(Error::Bad("not an edit nodes file"))?;
    let other = if e == Endian::Le { Endian::Be } else { Endian::Le };
    parse_as(d, e).map(|v| (v, e)).or_else(|x| parse_as(d, other).map(|v| (v, other)).map_err(|_| x))
}

fn parse_as(d: &[u8], e: Endian) -> Res<Vec<Obj>> {
    if d.get(..4) != Some(&[0; 4]) {
        return err("not an edit nodes file");
    }
    let mut r = Reader::new(d, e);
    r.skip(4)?;
    let n = r.u32()? as usize;
    let mut v = Vec::with_capacity(n);
    for _ in 0..n {
        let id = r.u32()?;
        v.push(Obj { id, params: read_params(&mut r)? });
    }
    if r.left() != 0 {
        return err(format!("{} unexpected bytes at the end", r.left()));
    }
    Ok(v)
}

#[cfg(test)]
pub(super) fn is_nodes(d: &[u8], e: Endian) -> bool {
    parse_as(d, e).is_ok()
}

fn write(v: &[Obj], e: Endian) -> Vec<u8> {
    let mut w = Writer::new(e);
    w.u32(0).u32(v.len() as u32);
    for o in v {
        w.u32(o.id);
        write_params(&mut w, &o.params);
    }
    w.finish()
}

fn dump(v: &[Obj], e: Endian) -> Val {
    Val::Tbl(
        v.iter()
            .map(|o| {
                let mut t = vec![(Some("Id".to_string()), names::show(o.id))];
                t.extend(dump_params(&o.params, e));
                (None, Val::Tbl(t))
            })
            .collect(),
    )
}

fn undump(v: &Val, e: Endian) -> Res<Vec<Obj>> {
    let mut out = Vec::new();
    for (i, (_, o)) in v.items().iter().filter(|(_, x)| !matches!(x, Val::Note(_))).enumerate() {
        let at = |m: String| Error::Msg(format!("Objects[{}].{m}", i + 1));
        let items = o.items();
        let id = match items.iter().find(|(_, x)| !matches!(x, Val::Note(_))) {
            Some((Some(k), Val::Str(s))) if k == "Id" => names::key(s),
            Some((Some(k), x)) if k == "Id" => x.int().and_then(|n| u32::try_from(n).ok()).ok_or_else(|| at("Id must be a name or a number".into()))?,
            _ => return Err(at("the first value must be Id = ...".into())),
        };
        let params = load_params(items.iter().filter(|(_, x)| !matches!(x, Val::Note(_))).skip(1), e).map_err(at)?;
        out.push(Obj { id, params });
    }
    Ok(out)
}

pub struct Wsd;

impl Format for Wsd {
    fn id(&self) -> &'static str {
        "wsd"
    }
    fn about(&self) -> &'static str {
        "edit nodes (objects placed in the world)"
    }
    fn exts(&self) -> &'static [&'static str] {
        &["wsd"]
    }
    fn probe(&self, i: &mut Input) -> bool {
        i.len < 64 << 20 && i.all().is_ok_and(|d| parse(d).is_ok())
    }
    fn info(&self, i: &mut Input) -> Res<Vec<(&'static str, String)>> {
        let (v, e) = parse(i.all()?)?;
        Ok(vec![("contents", format!("{} objects", v.len())), ("byte order", endian_name(e).into())])
    }
    fn unpack(&self, i: &mut Input, o: &mut Out) -> Res<PathBuf> {
        let (v, e) = parse(i.all()?)?;
        let name = i.name.clone();
        let mut t = header(&name, self.about());
        t.extend(meta(self.id(), &name, e));
        t.push((None, Val::Note("Objects in order: Id = object name, then its fields in game order (a name may repeat).\nUnknown fields are 0x... ; crc\"x\" = hash of a name; hex\"..\" = raw data; LuaParam = { name, value }".into())));
        t.push((Some("Objects".into()), dump(&v, e)));
        o.text(&format!("{name}.lua"), &Val::Tbl(t))
    }
    fn pack(&self, src: &Path, meta: &Val, opts: Opts, out: &mut Sink) -> Res<()> {
        let e = meta_endian(meta, opts);
        let v = undump(meta.key("Objects").ok_or(Error::Bad("no Objects = { ... }"))?, e).map_err(|x| Error::Msg(format!("{}: {x}", src.display())))?;
        out.put(&write(&v, e))
    }
}

struct Tpl {
    name: String,
    class: String,
    params: Vec<(u32, Vec<u8>)>,
}

fn tpl_endian(d: &[u8]) -> Res<Endian> {
    match d.get(..4) {
        Some(b"AULB") => Ok(Endian::Le),
        Some(b"BLUA") => Ok(Endian::Be),
        _ => err("not a game templates file (no AULB magic)"),
    }
}

fn tpl_text(r: &mut Reader) -> Res<String> {
    let n = r.u32()? as usize;
    text(r.take(n)?).ok_or(Error::Bad("template name is not a text"))
}

fn tpl_parse(d: &[u8]) -> Res<(Vec<Option<Tpl>>, Endian)> {
    let e = tpl_endian(d)?;
    let mut r = Reader::new(d, e);
    r.skip(4)?;
    let n = r.u32()? as usize;
    let mut v = Vec::new();
    for i in 0..n {
        let size = r.u32()? as usize;
        let mut b = Reader::new(r.take(size)?, e);
        let at = |m: Error| Error::Msg(format!("template {}: {m}", i + 1));
        let k = (|| Ok::<_, Error>((b.u32()?, b.u32()?)))().map_err(at)?.1;
        if k > 1 {
            return Err(at(Error::Msg(format!("holds {k} objects, only 1 is supported"))));
        }
        let t = match k {
            0 => None,
            _ => Some((|| Ok::<_, Error>(Tpl { name: tpl_text(&mut b)?, class: tpl_text(&mut b)?, params: read_params(&mut b)? }))().map_err(at)?),
        };
        if b.left() != 0 {
            return Err(at(Error::Msg(format!("{} unexpected bytes at the end", b.left()))));
        }
        v.push(t);
    }
    if r.left() != 0 {
        return err(format!("{} unexpected bytes at the end", r.left()));
    }
    Ok((v, e))
}

fn tpl_write(v: &[Option<Tpl>], e: Endian) -> Vec<u8> {
    let mut w = Writer::new(e);
    w.bytes(if e == Endian::Le { b"AULB" } else { b"BLUA" }).u32(v.len() as u32);
    for t in v {
        let mut b = Writer::new(e);
        b.u32(0).u32(t.is_some() as u32);
        if let Some(t) = t {
            for s in [&t.name, &t.class] {
                b.u32(s.len() as u32 + 1).bytes(&cstr(s));
            }
            write_params(&mut b, &t.params);
        }
        let b = b.finish();
        w.u32(b.len() as u32).bytes(&b);
    }
    w.finish()
}

pub struct GameTemplates;

impl Format for GameTemplates {
    fn id(&self) -> &'static str {
        "gametemplates"
    }
    fn about(&self) -> &'static str {
        "object templates (blueprints) the game can spawn by name"
    }
    fn exts(&self) -> &'static [&'static str] {
        &["wsd"]
    }
    fn probe(&self, i: &mut Input) -> bool {
        tpl_endian(&i.head(4)).is_ok()
    }
    fn info(&self, i: &mut Input) -> Res<Vec<(&'static str, String)>> {
        let (v, e) = tpl_parse(i.all()?)?;
        Ok(vec![("contents", format!("{} templates", v.len())), ("byte order", endian_name(e).into())])
    }
    fn unpack(&self, i: &mut Input, o: &mut Out) -> Res<PathBuf> {
        let (v, e) = tpl_parse(i.all()?)?;
        let name = i.name.clone();
        let mut t = header(&name, self.about());
        t.extend(meta(self.id(), &name, e));
        t.push((None, Val::Note("Templates in order: { \"template name\", \"object class\", then its fields in game order }; {} = empty slot.
Unknown fields are 0x... ; crc\"x\" = hash of a name; hex\"..\" = raw data; LuaParam = { name, value }".into())));
        let list = v.iter().map(|x| {
            let mut t = Vec::new();
            if let Some(x) = x {
                t = vec![(None, Val::Str(x.name.clone())), (None, Val::Str(x.class.clone()))];
                t.extend(dump_params(&x.params, e));
            }
            (None, Val::Tbl(t))
        });
        t.push((Some("Templates".into()), Val::Tbl(list.collect())));
        o.text(&format!("{name}.lua"), &Val::Tbl(t))
    }
    fn pack(&self, src: &Path, meta: &Val, opts: Opts, out: &mut Sink) -> Res<()> {
        let e = meta_endian(meta, opts);
        let mut v = Vec::new();
        for (n, (_, x)) in meta.key("Templates").ok_or(Error::Bad("no Templates = { ... }"))?.items().iter().filter(|(_, x)| !matches!(x, Val::Note(_))).enumerate() {
            let at = |m: String| Error::Msg(format!("{}: Templates[{}]: {m}", src.display(), n + 1));
            let mut items = x.items().iter().filter(|(_, x)| !matches!(x, Val::Note(_))).peekable();
            if items.peek().is_none() {
                v.push(None);
                continue;
            }
            let mut word = || match items.next() {
                Some((None, Val::Str(s))) => Ok(s.clone()),
                _ => Err(at("must start with the template name and the object class".into())),
            };
            let (name, class) = (word()?, word()?);
            v.push(Some(Tpl { name, class, params: load_params(items, e).map_err(at)? }));
        }
        out.put(&tpl_write(&v, e))
    }
}

struct Entry {
    hash: u32,
    size: u32,
    at: u32,
}

fn nodes_head(i: &mut Input) -> Res<(Vec<Entry>, Endian)> {
    let h = i.head(8);
    let e = match h.get(..4) {
        Some(b"00ED") => Endian::Le,
        Some(b"DE00") => Endian::Be,
        _ => return err("not an edit nodes pack (no 00ED magic)"),
    };
    let n = e.get::<u32>(&h[4..]).unwrap_or(0) as usize;
    if 8 + n as u64 * 12 > i.len {
        return err("entry table is larger than the file");
    }
    let t = i.at(8, n * 12)?;
    let mut r = Reader::new(&t, e);
    let v = r.list(n, |r| Ok(Entry { hash: r.u32()?, size: r.u32()?, at: r.u32()? }))?;
    if v.iter().any(|x| x.at as u64 + x.size as u64 > i.len) {
        return err("an entry runs past the end of the file");
    }
    Ok((v, e))
}

pub struct EditNodes;

impl Format for EditNodes {
    fn id(&self) -> &'static str {
        "editnodes"
    }
    fn about(&self) -> &'static str {
        "pack of edit node files (.wsd)"
    }
    fn exts(&self) -> &'static [&'static str] {
        &["pack"]
    }
    fn probe(&self, i: &mut Input) -> bool {
        nodes_head(i).is_ok()
    }
    fn info(&self, i: &mut Input) -> Res<Vec<(&'static str, String)>> {
        let (v, e) = nodes_head(i)?;
        Ok(vec![("contents", format!("{} files", v.len())), ("byte order", endian_name(e).into())])
    }
    fn list(&self, i: &mut Input) -> Res<Vec<Item>> {
        Ok(nodes_head(i)?.0.iter().map(|x| Item { name: names::label(x.hash), size: x.size as u64, note: String::new() }).collect())
    }
    fn unpack(&self, i: &mut Input, o: &mut Out) -> Res<PathBuf> {
        let (v, e) = nodes_head(i)?;
        let mut list = Vec::new();
        for x in &v {
            let d = i.at(x.at as u64, x.size as usize)?;
            let objs = parse_as(&d, e).map_err(|m| Error::Msg(format!("{}: {m}", names::label(x.hash))))?;
            let name = match names::name(x.hash).filter(|n| names::hash(n) == x.hash) {
                Some(n) => Val::Str(n),
                None => Val::Raw(format!("0x{:08x}", x.hash)),
            };
            list.push((None, Val::Tbl(vec![(None, name), (Some("Objects".into()), dump(&objs, e))])));
        }
        let name = i.name.clone();
        let mut t = header(&name, self.about());
        t.extend(meta(self.id(), &name, e));
        t.push((None, Val::Note("Files in pack order: { \"game path of the .wsd\" (or its hash 0x...), Objects = { ... } }.
Objects: Id = object name, then its fields in game order (a name may repeat);
unknown fields are 0x... ; crc\"x\" = hash of a name; hex\"..\" = raw data; LuaParam = { name, value }".into())));
        t.push((Some("Files".into()), Val::Tbl(list)));
        o.text(&format!("{name}.lua"), &Val::Tbl(t))
    }
    fn pack(&self, src: &Path, meta: &Val, opts: Opts, out: &mut Sink) -> Res<()> {
        let e = meta_endian(meta, opts);
        let mut list = Vec::new();
        for (n, (_, v)) in meta.key("Files").ok_or(Error::Bad("no Files = { ... }"))?.items().iter().filter(|(_, v)| !matches!(v, Val::Note(_))).enumerate() {
            let at = |m: String| Error::Msg(format!("{}: Files[{}]: {m}", src.display(), n + 1));
            let h = match v.items().first() {
                Some((None, Val::Str(s))) => names::key(s),
                Some((None, x)) => num(x).ok_or_else(|| at("first value must be the .wsd name or its hash".into()))?,
                _ => return Err(at("first value must be the .wsd name or its hash".into())),
            };
            let objs = undump(v.key("Objects").ok_or_else(|| at("missing Objects".into()))?, e).map_err(|m| at(m.to_string()))?;
            list.push((h, write(&objs, e)));
        }
        let mut w = Writer::new(e);
        w.bytes(if e == Endian::Le { b"00ED" } else { b"DE00" }).u32(list.len() as u32);
        let mut at = 8 + list.len() as u32 * 12;
        for (h, b) in &list {
            w.u32(*h).u32(b.len() as u32).u32(at);
            at += b.len() as u32;
        }
        out.put(&w.finish())?;
        for (_, b) in &list {
            out.put(b)?;
        }
        Ok(())
    }

}
