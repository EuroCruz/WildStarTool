use super::{build, endian_name, header, meta, meta_endian, Format, Input, Item, Opts, Out, Sink, INDEX};
use crate::io::err;
use crate::names;
use ac_core::{Endian, Error, Res, Writer};
use ac_lua::Val;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub fn clean(name: &str) -> String {
    let p: Vec<String> = name.split(['\\', '/']).filter(|s| !s.is_empty() && *s != "." && *s != "..").map(ac_core::fs::clean_name).collect();
    if p.is_empty() {
        "unnamed".into()
    } else {
        p.join("/")
    }
}

pub fn uniq(used: &mut HashSet<String>, name: &str) -> String {
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() && !e.contains('/') => (s, format!(".{e}")),
        _ => (name, String::new()),
    };
    let mut n = name.to_string();
    let mut i = 1;
    while !used.insert(n.to_lowercase()) {
        n = format!("{stem}_{i}{ext}");
        i += 1;
    }
    n
}

pub fn game_path(rel: &str) -> String {
    rel.replace('/', "\\")
}

fn kv(k: &str, v: Val) -> (Option<String>, Val) {
    (Some(k.into()), v)
}

pub fn entry(path: &str, extra: Vec<(Option<String>, Val)>) -> Val {
    if extra.is_empty() {
        Val::Str(path.into())
    } else {
        Val::Tbl([vec![(None, Val::Str(path.into()))], extra].concat())
    }
}

pub fn entry_path(v: &Val) -> Res<String> {
    match v {
        Val::Str(s) => Ok(s.clone()),
        Val::Tbl(t) => t.iter().find(|(k, _)| k.is_none()).and_then(|(_, v)| v.str()).map(str::to_string).ok_or(Error::Bad("entry needs a file name in quotes")),
        _ => Err(Error::Bad("entry must be \"file name\" or { \"file name\", ... }")),
    }
}

pub fn entry_num(v: &Val, k: &str) -> Res<Option<u32>> {
    match v.key(k) {
        None => Ok(None),
        Some(Val::Str(s)) => Ok(Some(names::key(s))),
        Some(Val::Int(i)) => u32::try_from(*i).map(Some).map_err(|_| Error::Msg(format!("{k} does not fit 32 bits"))),
        Some(_) => Err(Error::Msg(format!("{k} must be a number or a name in quotes"))),
    }
}

fn unlisted(dir: &Path, listed: &HashSet<String>, ext: &str, base: &str, out: &mut Vec<String>) -> Res<()> {
    let mut es: Vec<_> = std::fs::read_dir(dir)?.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    es.sort();
    for p in es {
        let n = p.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let rel = if base.is_empty() { n.clone() } else { format!("{base}/{n}") };
        let low = n.to_lowercase();
        if low == INDEX || low.starts_with('.') {
            continue;
        }
        let unit = low.strip_suffix(".lua").unwrap_or(&low).ends_with(ext);
        let key = rel.strip_suffix(".lua").unwrap_or(&rel).to_lowercase();
        if unit {
            if !listed.contains(&key) {
                out.push(rel.strip_suffix(".lua").unwrap_or(&rel).to_string());
            }
        } else if p.is_dir() {
            unlisted(&p, listed, ext, &rel, out)?;
        }
    }
    Ok(())
}

pub fn added(dir: &Path, listed: &[String], ext: &str) -> Res<Vec<String>> {
    let set: HashSet<String> = listed.iter().map(|s| s.to_lowercase()).collect();
    let mut o = Vec::new();
    unlisted(dir, &set, ext, "", &mut o)?;
    Ok(o)
}

struct MegaEntry {
    hash: u32,
    index: u32,
    size: u32,
    at: u64,
}

pub struct Mega;

fn mega_head(i: &mut Input) -> Res<(Vec<MegaEntry>, Endian)> {
    let h = i.head(8);
    let e = match h.get(..4) {
        Some(b"00PM") => Endian::Le,
        Some(b"MP00") => Endian::Be,
        _ => return err("not a megapack (no MP00 magic)"),
    };
    let n = e.get::<u32>(&h[4..]).unwrap_or(0) as usize;
    if 8 + n as u64 * 28 > i.len {
        return err("entry table is larger than the file");
    }
    let t = i.at(8, n * 20)?;
    let mut r = ac_core::Reader::new(&t, e);
    let v = r.list(n, |r| Ok(MegaEntry { hash: r.u32()?, index: r.u32()?, size: r.u32()?, at: r.u64()? }))?;
    if let Some(x) = v.iter().find(|x| x.at + x.size as u64 > i.len) {
        return err(format!("pack 0x{:08x} runs past the end of the file", x.hash));
    }
    Ok((v, e))
}

fn shard(stem: &str) -> u32 {
    let d: String = stem.chars().take_while(char::is_ascii_digit).collect();
    d.parse().unwrap_or_else(|_| names::hash(stem))
}

fn stem(rel: &str) -> &str {
    let f = rel.rsplit('/').next().unwrap_or(rel);
    f.rsplit_once('.').map_or(f, |x| x.0)
}

fn mega_name(x: &MegaEntry, head: &[u8]) -> String {
    if let Some(n) = names::name(x.hash) {
        let p = clean(&n);
        return if p.contains('.') { p } else { format!("{p}.pack") };
    }
    let albs = match head.get(..4) {
        Some(b"ALBS") => Some(Endian::Le),
        Some(b"SBLA") => Some(Endian::Be),
        _ => None,
    };
    if let Some(e) = albs.filter(|e| e.get::<u32>(&head[4..]) == Some(0)) {
        if let Some(n) = head.get(8..12).and_then(|b| e.get::<u32>(b)).and_then(names::name) {
            return format!("{}.pack", clean(&n));
        }
    }
    let s = x.index.to_string();
    if s.len() > 2 {
        format!("{}/{s}.pack", &s[..2])
    } else {
        format!("{s}.pack")
    }
}

const SECTOR: u64 = 2048;
const FILL: u8 = 0xcb;

impl Format for Mega {
    fn id(&self) -> &'static str {
        "megapack"
    }
    fn about(&self) -> &'static str {
        "archive of streamed world packs"
    }
    fn exts(&self) -> &'static [&'static str] {
        &["megapack", "kilopack"]
    }
    fn folder(&self) -> bool {
        true
    }
    fn probe(&self, i: &mut Input) -> bool {
        mega_head(i).is_ok()
    }
    fn info(&self, i: &mut Input) -> Res<Vec<(&'static str, String)>> {
        let (v, e) = mega_head(i)?;
        Ok(vec![("contents", format!("{} packs", v.len())), ("byte order", endian_name(e).into())])
    }
    fn list(&self, i: &mut Input) -> Res<Vec<Item>> {
        let (v, _) = mega_head(i)?;
        v.iter()
            .map(|x| {
                let h = i.at(x.at, 12.min(x.size as usize))?;
                Ok(Item { name: mega_name(x, &h), size: x.size as u64, note: format!("index {}", x.index) })
            })
            .collect()
    }
    fn unpack(&self, i: &mut Input, o: &mut Out) -> Res<PathBuf> {
        let (v, e) = mega_head(i)?;
        let mut used = HashSet::new();
        let mut list = Vec::with_capacity(v.len());
        for x in &v {
            let d = i.at(x.at, x.size as usize)?;
            let name = uniq(&mut used, &mega_name(x, &d));
            let mut extra = Vec::new();
            if names::hash(&game_path(&name)) != x.hash {
                extra.push(kv("Hash", names::show(x.hash)));
            }
            if shard(stem(&name)) != x.index {
                extra.push(kv("Index", Val::Int(x.index as i64)));
            }
            list.push((None, entry(&name, extra)));
            o.child(&name, d)?;
        }
        let mut t = header(&i.name, self.about());
        t.extend(meta(self.id(), &i.name, e));
        t.push((None, Val::Note("Packs in game order; each is a file or an unpacked folder next to this index.\nNew .pack files or folders put here are added automatically.\nHash: name hash (when it differs from the path), Index: stream index (when it differs from the number in the name)".into())));
        t.push(kv("Packs", Val::Tbl(list)));
        o.text(INDEX, &Val::Tbl(t))
    }
    fn pack(&self, src: &Path, meta: &Val, opts: Opts, out: &mut Sink) -> Res<()> {
        let e = meta_endian(meta, opts);
        let mut list = Vec::new();
        for (_, v) in meta.key("Packs").ok_or(Error::Bad("index.lua has no Packs = { ... }"))?.items().iter().filter(|(_, v)| !matches!(v, Val::Note(_))) {
            let p = entry_path(v)?;
            let hash = entry_num(v, "Hash")?.unwrap_or_else(|| names::key(&game_path(&p)));
            let index = entry_num(v, "Index")?.unwrap_or_else(|| shard(stem(&p)));
            list.push((p, hash, index));
        }
        let have: Vec<String> = list.iter().map(|x| x.0.clone()).collect();
        for p in added(src, &have, ".pack")? {
            let h = names::key(&game_path(&p));
            if list.iter().any(|x| x.1 == h) {
                return err(format!("{p} has the same name hash as another pack"));
            }
            list.push((p.clone(), h, shard(stem(&p))));
        }
        let n = list.len() as u64;
        let first = (8 + n * 28).next_multiple_of(SECTOR);
        out.put(&vec![0; first as usize])?;
        let mut sizes = Vec::with_capacity(list.len());
        for (p, _, _) in &list {
            let d = build(&ac_core::fs::join_safe(src, p)?, opts).map_err(|x| Error::Msg(format!("{p}: {x}")))?;
            let at = out.len;
            out.put(&d)?;
            out.pad(out.len.next_multiple_of(SECTOR), FILL)?;
            sizes.push((d.len() as u32, at));
        }
        let mut w = Writer::new(e);
        w.bytes(if e == Endian::Le { b"00PM" } else { b"MP00" }).u32(n as u32);
        for ((_, h, i), (s, at)) in list.iter().zip(&sizes) {
            w.u32(*h).u32(*i).u32(*s).u64(*at);
        }
        for (_, h, i) in &list {
            w.u32(*h).u32(*i);
        }
        let mut hd = w.finish();
        hd.resize(first as usize, FILL);
        out.patch(0, &hd)
    }
}

fn units(dir: &Path, base: &str, out: &mut Vec<String>) -> Res<()> {
    let mut es: Vec<_> = std::fs::read_dir(dir)?.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    es.sort();
    for p in es {
        let n = p.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let rel = if base.is_empty() { n.clone() } else { format!("{base}/{n}") };
        if n.starts_with('.') || (base.is_empty() && n.eq_ignore_ascii_case(INDEX)) {
            continue;
        }
        if p.is_dir() {
            if p.join(INDEX).is_file() {
                out.push(rel);
            } else {
                units(&p, &rel, out)?;
            }
        } else {
            let data = rel.strip_suffix(".lua").filter(|s| s.rsplit('/').next().is_some_and(|f| f.contains('.')));
            out.push(data.unwrap_or(&rel).to_string());
        }
    }
    Ok(())
}

pub fn new_units(src: &Path, listed: &[String]) -> Res<Vec<String>> {
    let set: HashSet<String> = listed.iter().map(|s| s.to_lowercase()).collect();
    let mut o = Vec::new();
    units(src, "", &mut o)?;
    o.retain(|u| !set.contains(&u.to_lowercase()));
    Ok(o)
}

const LOOSE_HEAD: usize = 128;
const NO_DATA: u32 = u32::MAX;

struct Loose {
    crc: u32,
    name: String,
    size: u32,
    at: u64,
}

fn loose_crc(name: &str) -> u32 {
    ac_core::hash::BZIP2.sum(name.to_ascii_lowercase().as_bytes())
}

fn loose_head(i: &mut Input) -> Res<Vec<Loose>> {
    let mut v = Vec::new();
    let mut p = 0u64;
    while p < i.len {
        let h = i.at(p, LOOSE_HEAD)?;
        let mut r = ac_core::Reader::new(&h, Endian::Le);
        let (crc, size) = (r.u32()?, r.u32()?);
        let n = r.rest();
        let Some(z) = n.iter().position(|&b| b == 0) else { return err(format!("file name at offset {p} is not zero-terminated")) };
        if z == 0 || n[z..].iter().any(|&b| b != 0) {
            return err(format!("bad file name at offset {p}"));
        }
        let name = String::from_utf8_lossy(&n[..z]).into_owned();
        let at = p + LOOSE_HEAD as u64;
        if size == NO_DATA {
            p = at;
        } else {
            if at + size as u64 > i.len {
                return err(format!("{name} runs past the end of the file"));
            }
            p = (at + size as u64).next_multiple_of(16);
            if p > i.len {
                return err("file does not end on a 16-byte boundary");
            }
        }
        v.push(Loose { crc, name, size, at });
    }
    if v.is_empty() || !v.iter().any(|x| x.crc == loose_crc(&x.name)) {
        return err("not a loose files pack");
    }
    Ok(v)
}

pub struct LooseFiles;

impl Format for LooseFiles {
    fn id(&self) -> &'static str {
        "loosefiles"
    }
    fn about(&self) -> &'static str {
        "pack of loose game files (maps, shaders, cinematics...)"
    }
    fn exts(&self) -> &'static [&'static str] {
        &["pack"]
    }
    fn folder(&self) -> bool {
        true
    }
    fn probe(&self, i: &mut Input) -> bool {
        i.head(4) != b"ALBS" && loose_head(i).is_ok()
    }
    fn info(&self, i: &mut Input) -> Res<Vec<(&'static str, String)>> {
        Ok(vec![("contents", format!("{} files", loose_head(i)?.len()))])
    }
    fn list(&self, i: &mut Input) -> Res<Vec<Item>> {
        Ok(loose_head(i)?.into_iter().map(|x| Item { name: x.name, size: if x.size == NO_DATA { 0 } else { x.size as u64 }, note: String::new() }).collect())
    }
    fn unpack(&self, i: &mut Input, o: &mut Out) -> Res<PathBuf> {
        let v = loose_head(i)?;
        let mut used = HashSet::new();
        let mut list = Vec::with_capacity(v.len());
        for x in &v {
            let rel = uniq(&mut used, &clean(&x.name));
            let mut extra = Vec::new();
            if game_path(&rel) != x.name {
                extra.push(kv("Name", Val::Str(x.name.clone())));
            }
            if x.crc != loose_crc(&x.name) {
                extra.push(kv("Crc", Val::Raw(format!("0x{:08x}", x.crc))));
            }
            if x.size == NO_DATA {
                extra.push(kv("NoData", Val::Bool(true)));
            } else {
                let d = i.at(x.at, x.size as usize)?;
                o.child(&rel, d)?;
            }
            list.push((None, entry(&rel, extra)));
        }
        let mut t = header(&i.name, self.about());
        t.extend(meta(self.id(), &i.name, Endian::Le));
        t.push((None, Val::Note("Files in pack order; each is a file or an unpacked folder next to this index.\nNew files put here are added at the end. Name: name in the game (when it differs from the path),\nNoData: listed without data (the game looks for it elsewhere)".into())));
        t.push(kv("Files", Val::Tbl(list)));
        o.text(INDEX, &Val::Tbl(t))
    }
    fn pack(&self, src: &Path, meta: &Val, opts: Opts, out: &mut Sink) -> Res<()> {
        let mut list = Vec::new();
        for (_, v) in meta.key("Files").ok_or(Error::Bad("index.lua has no Files = { ... }"))?.items().iter().filter(|(_, v)| !matches!(v, Val::Note(_))) {
            let p = entry_path(v)?;
            let name = v.key("Name").and_then(Val::str).map_or_else(|| game_path(&p), str::to_string);
            let crc = entry_num(v, "Crc")?.unwrap_or_else(|| loose_crc(&name));
            list.push((p, name, crc, matches!(v.key("NoData"), Some(Val::Bool(true)))));
        }
        let have: Vec<String> = list.iter().map(|x| x.0.clone()).collect();
        for p in new_units(src, &have)? {
            let name = game_path(&p);
            list.push((p, name.clone(), loose_crc(&name), false));
        }
        for (p, name, crc, none) in list {
            if name.len() >= LOOSE_HEAD - 8 {
                return err(format!("{name}: name is longer than {} characters", LOOSE_HEAD - 9));
            }
            let d = if none { Vec::new() } else { build(&ac_core::fs::join_safe(src, &p)?, opts).map_err(|x| Error::Msg(format!("{p}: {x}")))? };
            let mut w = Writer::new(Endian::Le);
            w.u32(crc).u32(if none { NO_DATA } else { d.len() as u32 }).bytes(name.as_bytes());
            let mut h = w.finish();
            h.resize(LOOSE_HEAD, 0);
            out.put(&h)?;
            if !none {
                out.put(&d)?;
                out.pad(out.len.next_multiple_of(16), 0)?;
            }
        }
        Ok(())
    }
}
