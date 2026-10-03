use super::arch::{clean, new_units, entry, entry_num, entry_path, game_path, uniq};
use super::tex;
use super::{endian_name, header, meta, meta_endian, Format, Input, Item, Opts, Out, Sink, INDEX};
use crate::io::{err, Dump, Io, Len, Load, Mode, Read, Show, Write};
use crate::names;
use ac_core::{Endian, Error, Reader, Res, Writer};
use ac_lua::Val;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

const ORDER: [usize; 9] = [6, 7, 0, 2, 8, 4, 3, 1, 5];
const LATE: u32 = 8;
const ZIP: u32 = 2;
const TEX_FLAGS: u32 = 6;

#[derive(Default)]
pub(super) struct Block {
    pub(super) show_counts: bool,
    tile: bool,
    size: [u32; 2],
    high: f32,
    low: f32,
    heights: Vec<u8>,
    lists: [Vec<[u32; 2]>; 2],
    unused: u32,
    counts: [u32; 9],
    unused2: Vec<u32>,
    palettes: Vec<u32>,
    sets: Vec<Set>,
}

#[derive(Default)]
struct Set {
    key: u32,
    items: Vec<u32>,
}

impl Block {
    pub(super) fn walk<S: Io>(&mut self, s: &mut S) -> Res<()> {
        if s.mode() == Mode::Load {
            self.tile = s.has("Tile");
        }
        if self.tile {
            s.node("Tile", |s| {
                s.magic(b"1IEH")?;
                s.val("Size", &mut self.size, Show::Dec)?;
                s.f32("High", &mut self.high)?;
                s.f32("Low", &mut self.low)?;
                let n = s.n(self.size[0] * self.size[1]);
                s.grid("Heights", &mut self.heights, self.size[0] as usize, n)
            })?;
        }
        for (k, l) in ["List1", "List2"].into_iter().zip(&mut self.lists) {
            if !s.skip(k, l.is_empty()) {
                s.list(k, l, Len::U32, |s, x| s.val("", x, Show::Hex))?;
            }
        }
        s.opt("Unused", &mut self.unused, Show::Hex)?;
        if self.show_counts {
            s.opt("Counts", &mut self.counts, Show::Dec)?;
        } else {
            s.hide(&mut self.counts)?;
        }
        if !s.skip("Unused2", self.unused2.is_empty()) {
            s.list("Unused2", &mut self.unused2, Len::U32, |s, x| s.hex("", x))?;
        }
        if !s.skip("Palettes", self.palettes.is_empty()) {
            s.hashes("Palettes", &mut self.palettes, Len::U32)?;
        }
        if !s.skip("Sets", self.sets.is_empty()) {
            s.list("Sets", &mut self.sets, Len::U32, |s, x| {
                s.hash("Key", &mut x.key)?;
                s.hashes("Items", &mut x.items, Len::U32)
            })?;
        }
        Ok(())
    }

    fn read(d: &[u8], e: Endian) -> Res<Block> {
        let mut b = Block { tile: d.starts_with(if e == Endian::Le { b"1IEH" } else { b"HEI1" }), ..Default::default() };
        let mut s = Read::new(d, e);
        b.walk(&mut s)?;
        match s.r.left() {
            0 => Ok(b),
            n => err(format!("{n} unexpected bytes at the end of the pack header")),
        }
    }

    fn total(&self) -> usize {
        self.counts.iter().sum::<u32>() as usize
    }
}

#[derive(Default, Clone)]
struct Rec {
    hash: u32,
    at: u32,
    size: u32,
    unc: u32,
    zero: u32,
    tag: u32,
}

impl Rec {
    fn late(&self) -> bool {
        self.hash == 0
    }
    fn key(&self) -> u32 {
        if self.late() {
            self.tag
        } else {
            self.hash
        }
    }
}

struct Pack {
    e: Endian,
    block: Option<Block>,
    recs: Vec<Rec>,
    kinds: Vec<Option<u32>>,
    base: usize,
}

fn endian(d: &[u8]) -> Option<Endian> {
    match d.get(..4)? {
        b"ALBS" => Some(Endian::Le),
        b"SBLA" => Some(Endian::Be),
        _ => None,
    }
}

fn parse(d: &[u8]) -> Res<Pack> {
    let e = endian(d).ok_or(Error::Bad("not an ALBS pack (no ALBS magic)"))?;
    let mut r = Reader::new(d, e);
    r.skip(4)?;
    let ext = r.u32()? as usize;
    let block = if ext > 0 { Some(Block::read(r.take(ext)?, e)?) } else { None };
    let rec = |r: &mut Reader| -> Res<Rec> { Ok(Rec { hash: r.u32()?, at: r.u32()?, size: r.u32()?, unc: r.u32()?, zero: r.u32()?, tag: r.u32()? }) };
    let (mut recs, mut kinds) = (Vec::new(), Vec::new());
    let mut pos = 0u32;
    if let Some(b) = &block {
        if b.total() * 24 > r.left() {
            return err("record table is larger than the file");
        }
        for k in ORDER {
            for _ in 0..b.counts[k] {
                recs.push(rec(&mut r)?);
                kinds.push(Some(k as u32));
            }
        }
    } else {
        while r.left() >= 24 {
            let x = rec(&mut Reader::at(d, e, r.pos())?)?;
            if x.at != pos && !(x.hash == 0 && x.size == 0 && x.at <= pos) {
                break;
            }
            r.skip(24)?;
            if !x.late() {
                pos = x.at + x.size;
            }
            recs.push(x);
            kinds.push(None);
        }
    }
    let base = r.pos();
    let mut pos = 0u64;
    for (i, x) in recs.iter().enumerate() {
        if x.zero != 0 {
            return err(format!("record {} has a non-zero reserved field", i + 1));
        }
        if !x.late() {
            if x.at as u64 != pos {
                return err(format!("record {} is not stored right after the previous one", i + 1));
            }
            pos += x.size as u64;
        }
        if kinds[i].is_some_and(|k| (k == LATE) != x.late()) {
            return err(format!("record {} has a hash that does not match its group", i + 1));
        }
    }
    let late: u64 = recs.iter().filter(|x| x.late()).map(|x| x.size as u64).sum();
    if base as u64 + pos + late != d.len() as u64 {
        return err(format!("file is {} bytes, records describe {}", d.len(), base as u64 + pos + late));
    }
    Ok(Pack { e, block, recs, kinds, base })
}

fn mesh(b: &[u8], e: Endian) -> Option<u32> {
    let mut r = Reader::new(b, e);
    (r.u32().ok()? == u32::from_be_bytes(*b"MSHA")).then(|| r.skip(4).and_then(|_| r.u32()).ok()).flatten()
}

fn unzip(b: &[u8]) -> Option<Vec<u8>> {
    if b.len() < 6 || b[0] != 0x78 {
        return None;
    }
    let u = ac_pack::flate::unzlib(b).ok()?;
    (ac_pack::flate::zlib_zc(&u, 1) == b).then_some(u)
}

fn zip(d: &[u8]) -> Vec<u8> {
    ac_pack::flate::zlib_zc(d, 1)
}

fn unflash(b: &[u8]) -> Option<Vec<u8>> {
    let (head, z) = (b.get(..8)?, b.get(8..)?);
    if &head[..3] != b"CFX" {
        return None;
    }
    let u = ac_pack::flate::unzlib(z).ok()?;
    (ac_pack::flate::zlib_zc(&u, 9) == z).then(|| [b"GFX", &head[3..], &u].concat())
}

fn flash(b: &[u8]) -> Res<Vec<u8>> {
    match b.get(..8) {
        Some(h) if &h[..3] == b"GFX" => Ok([b"CFX", &h[3..], &ac_pack::flate::zlib_zc(&b[8..], 9)].concat()),
        _ => err("a Compressed flash movie must be a GFX file"),
    }
}

fn is_flash(b: &[u8]) -> bool {
    b.starts_with(b"CFX") || b.starts_with(b"GFX")
}

fn kv(k: &str, v: Val) -> (Option<String>, Val) {
    (Some(k.into()), v)
}

const EXTS: [&str; 9] = [".mesh", ".tex", ".physics", ".pathgraph", ".aifence", ".bin", ".sound", ".gfx", ".wsd"];

fn ext_of(b: &[u8], e: Endian, kind: Option<u32>, zipped: bool) -> &'static str {
    if mesh(b, e).is_some() {
        ".mesh"
    } else if is_flash(b) {
        ".gfx"
    } else if tex::parse(b, e).is_some() {
        ".tex"
    } else if let Some(k) = kind {
        EXTS.get(k as usize).copied().unwrap_or(".bin")
    } else if zipped {
        ".physics"
    } else {
        ".bin"
    }
}

fn unc_of(b: &[u8], e: Endian, size: u32) -> u32 {
    if let Some(m) = mesh(b, e).filter(|&m| m != 0) {
        return m;
    }
    tex::parse(b, e).map_or(size, |t| t.unc)
}

fn stem(rel: &str) -> &str {
    rel.rsplit_once('.').map_or(rel, |x| x.0)
}

fn key_of(rel: &str) -> u32 {
    let s = stem(rel);
    match s.strip_prefix("0x").filter(|h| h.len() == 8) {
        Some(h) => u32::from_str_radix(h, 16).unwrap_or_else(|_| names::key(&game_path(s))),
        None => names::key(&game_path(s)),
    }
}

pub struct Albs;

impl Albs {
    fn bodies<'a>(&self, d: &'a [u8], p: &Pack) -> Vec<&'a [u8]> {
        let main = p.base + p.recs.iter().filter(|x| !x.late()).map(|x| x.size as usize).sum::<usize>();
        let mut late = main;
        p.recs
            .iter()
            .map(|x| {
                if x.late() {
                    late += x.size as usize;
                    &d[late - x.size as usize..late]
                } else {
                    &d[p.base + x.at as usize..][..x.size as usize]
                }
            })
            .collect()
    }
}

fn late_at(recs: &[Rec], i: usize) -> u32 {
    recs[..i].iter().filter(|x| !x.late()).map(|x| x.size).sum()
}

impl Format for Albs {
    fn id(&self) -> &'static str {
        "albs"
    }
    fn about(&self) -> &'static str {
        "stream pack (meshes, textures and data of one world block)"
    }
    fn exts(&self) -> &'static [&'static str] {
        &["pack", "dynpack"]
    }
    fn folder(&self) -> bool {
        true
    }
    fn probe(&self, i: &mut Input) -> bool {
        endian(&i.head(4)).is_some() && i.all().is_ok_and(|d| parse(d).is_ok())
    }
    fn info(&self, i: &mut Input) -> Res<Vec<(&'static str, String)>> {
        let p = parse(i.all()?)?;
        Ok(vec![("contents", format!("{} files", p.recs.len())), ("byte order", endian_name(p.e).into())])
    }
    fn list(&self, i: &mut Input) -> Res<Vec<Item>> {
        let d = i.all()?;
        let p = parse(d)?;
        Ok(self.bodies(d, &p).iter().zip(&p.recs).zip(&p.kinds).map(|((b, x), k)| Item { name: format!("{}{}", names::label(x.key()), ext_of(b, p.e, *k, false)), size: x.size as u64, note: String::new() }).collect())
    }
    fn unpack(&self, i: &mut Input, o: &mut Out) -> Res<PathBuf> {
        let name = i.name.clone();
        let d = i.all()?;
        let mut p = parse(d)?;
        let e = p.e;
        let bodies = self.bodies(d, &p);
        let mut used = HashSet::new();
        let mut files = Vec::with_capacity(p.recs.len());
        let mut gpu = None;
        for (n, (x, b)) in p.recs.iter().zip(&bodies).enumerate() {
            let kind = p.kinds[n];
            let zdef = kind == Some(ZIP);
            let t = tex::parse(b, e);
            let dds = t.as_ref().filter(|_| !o.opts.raw).and_then(|t| tex::to_dds(t, e));
            let unz = if dds.is_some() { None } else { unzip(b) };
            let zipped = unz.is_some() && (zdef || unz.as_deref().is_some_and(|u| u.len() == x.unc as usize));
            let mut texf = Vec::new();
            let (body, ext, unc, base): (&[u8], &str, u32, Option<String>) = match (&dds, &t, &unz) {
                (Some(d), Some(t), _) => {
                    gpu = Some(tex::Gpu::of(t, e));
                    if t.flags != TEX_FLAGS {
                        texf.push(kv("TexFlags", Val::Int(t.flags as i64)));
                    }
                    (d, ".dds", t.unc, Some(clean(&t.name)))
                }
                (_, _, Some(u)) if zipped => (u, ext_of(u, e, kind, true), u.len() as u32, None),
                _ => (b, ext_of(b, e, kind, false), unc_of(b, e, x.size), None),
            };
            let gfx = if o.opts.raw { None } else { unflash(body) };
            let body: &[u8] = gfx.as_deref().unwrap_or(body);
            let base = base.or_else(|| names::name(x.key()).filter(|_| x.key() != 0).map(|s| clean(&s))).unwrap_or_else(|| format!("0x{:08x}", x.key()));
            let rel = uniq(&mut used, &format!("{base}{ext}"));
            if let (Some(_), Some(t)) = (&dds, &t) {
                if game_path(stem(&rel)) != t.name {
                    texf.insert(0, kv("TexName", Val::Str(t.name.clone())));
                }
            }
            let mut extra = Vec::new();
            if let Some(k) = kind {
                extra.push(kv("Kind", Val::Int(k as i64)));
            }
            if key_of(&rel) != x.key() {
                extra.push(kv("Hash", names::show(x.key())));
            }
            if x.late() && kind.is_none() {
                extra.push(kv("Late", Val::Bool(true)));
            }
            if x.late() && x.at != late_at(&p.recs, n) {
                extra.push(kv("Offset", Val::Int(x.at as i64)));
            }
            if !x.late() && x.tag != 0 {
                extra.push(kv("Flags", Val::Int(x.tag as i64)));
            }
            if zipped != zdef {
                extra.push(kv("Zlib", Val::Bool(zipped)));
            }
            if gfx.is_some() {
                extra.push(kv("Compressed", Val::Bool(true)));
            }
            extra.extend(texf);
            if x.unc != unc {
                extra.push(kv("Unpacked", Val::Int(x.unc as i64)));
            }
            o.write(&rel, body)?;
            files.push((None, entry(&rel, extra)));
        }
        let mut t = header(&name, self.about());
        t.extend(meta(self.id(), &name, e));
        if let Some(g) = gpu.filter(|&g| g != tex::Gpu::Pc) {
            t.push(kv("Platform", Val::Str(g.name().into())));
        }
        if let Some(b) = &mut p.block {
            let mut s = Dump::new(e);
            b.walk(&mut s)?;
            t.push((None, Val::Note("Block: world block header (Tile = terrain heights, Palettes, Sets = { Key, Items })".into())));
            t.push(kv("Block", Val::Tbl(s.finish())));
        }
        t.push((None, Val::Note(
            "Files in game order; each is a file next to this index. New files put here are added at the end.\n\
             Kind: group of the file (0 meshes, 1 textures, 2 physics, 3 path graphs, 4 AI fences, 6 sounds, 7 flash movies, 8 wsd);\n\
             Late: stored after all other files. File names come from the name dictionary (0x... when unknown)\n\
             Hash: name hash (when it differs from the file name), Unpacked: size in memory (when it differs from the computed one)\n\
             .gfx: flash movie (Scaleform, opens in JPEXS FFDec); Compressed: stored compressed (CFX) in the pack"
                .into(),
        )));
        t.push(kv("Files", Val::Tbl(files)));
        o.text(INDEX, &Val::Tbl(t))
    }
    fn pack(&self, src: &Path, meta: &Val, opts: Opts, out: &mut Sink) -> Res<()> {
        let e = meta_endian(meta, opts);
        let gpu = match (meta.key("Platform").and_then(Val::str), e) {
            (Some("ps3"), _) => tex::Gpu::Ps3,
            (_, Endian::Le) => tex::Gpu::Pc,
            _ => tex::Gpu::Xbox360,
        };
        let mut block = match meta.key("Block") {
            Some(v) => {
                let mut s = Load::new(v.clone(), e);
                let mut b = Block::default();
                b.walk(&mut s)?;
                s.rest()?;
                Some(b)
            }
            None => None,
        };
        let list: Vec<&Val> = meta.key("Files").ok_or(Error::Bad("index.lua has no Files = { ... }"))?.items().iter().map(|x| &x.1).filter(|v| !matches!(v, Val::Note(_))).collect();
        let mut files: Vec<(String, Option<&Val>)> = list.iter().map(|v| entry_path(v).map(|p| (p, Some(*v)))).collect::<Res<_>>()?;
        let have: Vec<String> = files.iter().map(|x| x.0.clone()).collect();
        files.extend(new_units(src, &have)?.into_iter().map(|p| (p, None)));
        let mut items = Vec::with_capacity(files.len());
        for (rel, v) in files {
            let at = |m: String| Error::Msg(format!("{rel}: {m}"));
            let num = |k: &str| v.map_or(Ok(None), |v| entry_num(v, k)).map_err(|x| at(x.to_string()));
            let flag = |k: &str| v.and_then(|v| v.key(k)).map(|x| matches!(x, Val::Bool(true)));
            let kind = num("Kind")?;
            if block.is_some() && kind.is_none_or(|k| k > 8) {
                return Err(at("needs Kind = 0..8 (this pack has a Block header)".into()));
            }
            let body = std::fs::read(ac_core::fs::join_safe(src, &rel)?).map_err(|x| at(x.to_string()))?;
            let zipped = flag("Zlib").unwrap_or(kind == Some(ZIP));
            let late = kind.map_or(flag("Late").unwrap_or(false), |k| k == LATE);
            let key = num("Hash")?.unwrap_or_else(|| key_of(&rel));
            let (body, unc) = if rel.to_lowercase().ends_with(".dds") {
                let name = v.and_then(|v| v.key("TexName")).and_then(Val::str).map_or_else(|| game_path(stem(&rel)), str::to_string);
                tex::from_dds(&body, &name, num("TexFlags")?.unwrap_or(TEX_FLAGS), gpu).map_err(|x| at(x.to_string()))?
            } else if zipped {
                (zip(&body), body.len() as u32)
            } else if flag("Compressed") == Some(true) {
                let b = flash(&body).map_err(|x| at(x.to_string()))?;
                let u = b.len() as u32;
                (b, u)
            } else {
                let u = unc_of(&body, e, body.len() as u32);
                (body, u)
            };
            let unc = num("Unpacked")?.unwrap_or(unc);
            let (hash, tag) = if late { (0, key) } else { (key, num("Flags")?.unwrap_or(0)) };
            if !late && key == 0 {
                return Err(at("hash 0 is only allowed for Late files".into()));
            }
            let rec = Rec { hash, at: num("Offset")?.unwrap_or(u32::MAX), size: body.len() as u32, unc, zero: 0, tag };
            items.push((ORDER.iter().position(|&k| Some(k as u32) == kind).unwrap_or(0), rec, body));
        }
        if let Some(b) = &mut block {
            items.sort_by_key(|x| x.0);
            b.counts = [0; 9];
            for (g, _, _) in &items {
                b.counts[ORDER[*g]] += 1;
            }
        }
        let mut pos = 0u32;
        for (_, r, b) in &mut items {
            if r.late() {
                if r.at == u32::MAX {
                    r.at = pos;
                }
            } else {
                r.at = pos;
                pos = pos.checked_add(b.len() as u32).ok_or(Error::Bad("pack is larger than 4 GB"))?;
            }
        }
        let ext = match &mut block {
            Some(b) => {
                let mut s = Write::new(e);
                b.walk(&mut s)?;
                s.w.finish()
            }
            None => Vec::new(),
        };
        let mut w = Writer::new(e);
        w.bytes(if e == Endian::Le { b"ALBS" } else { b"SBLA" }).u32(ext.len() as u32).bytes(&ext);
        for (_, r, _) in &items {
            w.u32(r.hash).u32(r.at).u32(r.size).u32(r.unc).u32(r.zero).u32(r.tag);
        }
        out.put(&w.finish())?;
        for (_, _, b) in items.iter().filter(|x| !x.1.late()).chain(items.iter().filter(|x| x.1.late())) {
            out.put(b)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod t {
    use super::*;

    pub fn packs_pub(p: &Path, f: impl FnMut(&[u8])) {
        packs(p, f)
    }

    fn packs(p: &Path, mut f: impl FnMut(&[u8])) {
        let mut i = Input::open(p).unwrap();
        let h = i.head(8);
        let e = if &h[..4] == b"00PM" { Endian::Le } else { Endian::Be };
        let n = e.get::<u32>(&h[4..]).unwrap() as usize;
        let t = i.at(8, n * 20).unwrap();
        let mut r = Reader::new(&t, e);
        for _ in 0..n {
            let (_, _, size, at) = (r.u32().unwrap(), r.u32().unwrap(), r.u32().unwrap(), r.u64().unwrap());
            f(&i.at(at, size as usize).unwrap());
        }
    }

    pub fn textures(dir: &str, mut f: impl FnMut(tex::Tex, Endian)) {
        let g = PathBuf::from(r"C:\Users\Andrew\Desktop\Saboteur\GAMEFILES").join(dir);
        for p in ac_core::fs::walk(&g).unwrap().into_iter().filter(|p| p.to_string_lossy().to_lowercase().ends_with("megapack")) {
            packs(&p, |d| {
                let Ok(pk) = parse(d) else { return };
                for b in Albs.bodies(d, &pk) {
                    if let Some(t) = tex::parse(b, pk.e) {
                        f(t, pk.e);
                    }
                }
            });
        }
    }

    #[test]
    #[ignore]
    fn all_megapack_albs() {
        for dir in ["SUBPC", "SUBXE", "SUBPS3"] {
            let g = PathBuf::from(r"C:\Users\Andrew\Desktop\Saboteur\GAMEFILES").join(dir);
            let (mut n, mut ok, mut bad) = (0, 0, Vec::new());
            for p in ac_core::fs::walk(&g).unwrap().into_iter().filter(|p| p.to_string_lossy().to_lowercase().ends_with("pack")) {
                if !matches!(Input::open(&p).unwrap().head(4).as_slice(), b"00PM" | b"MP00") {
                    continue;
                }
                packs(&p, |d| {
                    if endian(d).is_none() {
                        return;
                    }
                    n += 1;
                    match parse(d) {
                        Ok(_) => ok += 1,
                        Err(e) => bad.push(format!("{}: {e}", p.display())),
                    }
                });
            }
            eprintln!("{dir}: {ok} of {n} ALBS parsed");
            for b in bad.iter().take(5) {
                eprintln!("  {b}");
            }
            assert!(bad.is_empty());
        }
    }

    #[test]
    #[ignore]
    fn survey() {
        let mut st: std::collections::BTreeMap<String, (usize, Vec<String>)> = Default::default();
        let g = PathBuf::from(r"C:\Users\Andrew\Desktop\Saboteur\GAMEFILES\SUBPC");
        for p in ac_core::fs::walk(&g).unwrap().into_iter().filter(|p| p.to_string_lossy().to_lowercase().ends_with("pack")) {
            if !matches!(Input::open(&p).unwrap().head(4).as_slice(), b"00PM" | b"MP00") {
                continue;
            }
            packs(&p, |d| {
                let Ok(pk) = parse(d) else { return };
                for (n, (x, b)) in pk.recs.iter().zip(Albs.bodies(d, &pk)).enumerate() {
                    let u = unzip(b);
                    let body = u.as_deref().unwrap_or(b);
                    let sig = if mesh(body, pk.e).is_some() {
                        "mesh".to_string()
                    } else if tex::parse(body, pk.e).is_some() {
                        "tex".into()
                    } else if is_flash(body) {
                        "gfx".into()
                    } else if body.len() >= 4 && body[..4].iter().all(|c| c.is_ascii_alphanumeric()) {
                        format!("magic {}", String::from_utf8_lossy(&body[..4]))
                    } else {
                        "other".into()
                    };
                    let k = format!("kind {:?} z{} {sig}", pk.kinds[n], u.is_some() as u8);
                    let e = st.entry(k).or_default();
                    e.0 += 1;
                    if e.1.len() < 6 {
                        e.1.push(format!("{}({}) {}", names::label(x.key()), body.len(), body.iter().take(12).map(|c| format!("{c:02x}")).collect::<String>()));
                    }
                }
            });
        }
        for (k, (n, ex)) in &st {
            eprintln!("{n:7} {k}: {}", ex.join(" "));
        }
    }

    #[test]
    #[ignore]
    fn xe_textures() {
        console_textures("SUBXE");
    }

    #[test]
    #[ignore]
    fn ps3_textures() {
        console_textures("SUBPS3");
    }

    fn console_textures(dir: &str) {
        let mut pc = std::collections::HashMap::new();
        textures("SUBPC", |t, e| {
            if let Some(d) = tex::to_dds(&t, e) {
                pc.insert(t.name.to_lowercase(), d);
            }
        });
        let (mut n, mut ok, mut same, mut known, mut bad, mut far) = (0, 0, 0, 0, Vec::new(), Vec::new());
        textures(dir, |t, e| {
            n += 1;
            let Some(d) = tex::to_dds(&t, e) else { return bad.push(format!("{}: cannot decode (code 0x{:x}, {}x{} mips {} unc {}) first {:02x?} len {}", t.name, t.code, t.w, t.h, t.mips, t.unc, &t.streams[0][..32.min(t.streams[0].len())], t.streams[0].len())) };
            if let Some(p) = pc.get(&t.name.to_lowercase()) {
                let (a, b) = (ac_fmt::img::Dds::parse(p).unwrap(), ac_fmt::img::Dds::parse(&d).unwrap());
                if (a.w, a.h, a.fmt) == (b.w, b.h, b.fmt) {
                    known += 1;
                    let worst = (0..a.mips.min(b.mips)).map(|m| super::look::diff_at(p, &d, m)).fold(0.0, f64::max);
                    if worst < 12.0 {
                        same += 1;
                    } else if far.len() < 15 {
                        far.push(format!("{} {}x{} code 0x{:x} pc {:?} worst mip diff {worst:.1}", t.name, a.w, a.h, t.code, a.fmt));
                    }
                }
            }
            let (rec, _) = tex::from_dds(&d, &t.name, t.flags, tex::Gpu::of(&t, e)).unwrap();
            let back = tex::to_dds(&tex::parse(&rec, e).unwrap(), e).unwrap();
            if back == d {
                ok += 1;
            } else {
                bad.push(format!("{}: rebuilt texture decodes differently", t.name));
            }
        });
        eprintln!("{dir} textures {n}, decoded+rebuilt {ok}, same size+format on pc {known}, close to pc {same}");
        for f in &far {
            eprintln!("  far: {f}");
        }
        for b in bad.iter().take(10) {
            eprintln!("  {b}");
        }
        assert!(bad.is_empty());
    }
}

#[cfg(test)]
mod look {
    use super::*;
    use ac_fmt::img::Dds;

    fn diff(a: &[u8], b: &[u8]) -> f64 {
        diff_at(a, b, 0)
    }

    pub fn diff_at(a: &[u8], b: &[u8], m: u32) -> f64 {
        let (a, b) = (Dds::parse(a).unwrap(), Dds::parse(b).unwrap());
        let (x, y) = (a.image(0, m).unwrap(), b.image(0, m).unwrap());
        let s: u64 = x.px.iter().zip(&y.px).map(|(p, q)| p.iter().zip(q).map(|(u, v)| (*u as i32 - *v as i32).unsigned_abs() as u64).sum::<u64>()).sum();
        s as f64 / (x.px.len() * 4) as f64
    }

    #[test]
    #[ignore]
    fn xe_vs_pc() {
        let want = ["gb_sp_lumber01", "p_clouds_ems", "noise_dt", "fstdatamap_nc", "tut_ambient_image1"];
        let grab = |dir: &str| {
            let mut m = std::collections::HashMap::new();
            super::t::textures(dir, |t, e| {
                let k = t.name.to_lowercase();
                if want.contains(&k.as_str()) && !m.contains_key(&k) {
                    m.insert(k, tex::to_dds(&t, e).unwrap());
                }
            });
            m
        };
        let (pc, xe) = (grab("SUBPC"), grab(&std::env::var("WST_CON").unwrap_or("SUBXE".into())));
        let one = |m: &std::collections::HashMap<String, Vec<u8>>, k: &str| m[&k.to_lowercase()].clone();
        for (a, b) in [("GB_SP_Lumber01", "P_Clouds_EMS"), ("Noise_DT", "Noise_DT"), ("FSTDataMap_NC", "FSTDataMap_NC"), ("tut_ambient_image1", "tut_ambient_image1")] {
            let (p, x) = (one(&pc, a), one(&xe, a));
            let o = one(&pc, b);
            eprintln!("{a}: pc~xe {:.1}  pc~other {:.1}", diff(&p, &x), if a != b { diff(&p, &o) } else { -1.0 });
            let m = Dds::parse(&p).unwrap().mips;
            eprintln!("  mips: {:?}", (0..m).map(|i| (diff_at(&p, &x, i) * 10.0).round() / 10.0).collect::<Vec<_>>());
            for (tag, d) in [("pc", &p), ("xe", &x)] {
                let dd = Dds::parse(d).unwrap();
                for m in 0..dd.mips.min(6) {
                    let i = dd.image(0, m).unwrap();
                    let raw: Vec<u8> = i.px.iter().flatten().copied().collect();
                    std::fs::write(std::env::temp_dir().join(format!("{a}_{tag}_{m}_{}x{}.rgba", i.w, i.h)), raw).unwrap();
                }
            }
            std::fs::write(std::env::temp_dir().join(format!("{a}_pc.dds")), &p).unwrap();
            std::fs::write(std::env::temp_dir().join(format!("{a}_xe.dds")), &x).unwrap();
        }
    }
}

#[cfg(test)]
mod ps3 {
    use super::*;

    #[test]
    #[ignore]
    fn ps3_texture_kinds() {
        let mut c = std::collections::BTreeMap::new();
        let mut ex = std::collections::BTreeMap::new();
        super::t::textures("SUBPS3", |t, e| {
            let k = (t.code, t.streams.len().min(2), tex::to_dds(&t, e).is_some());
            *c.entry(k).or_insert(0) += 1;
            ex.entry(t.code).or_insert_with(|| format!("{} {}x{} mips {} unc {} first {:02x?}", t.name, t.w, t.h, t.mips, t.unc, &t.streams[0][..16.min(t.streams[0].len())]));
        });
        eprintln!("{c:x?}");
        for (k, v) in ex {
            eprintln!("0x{k:08x}: {v}");
        }
    }
}

#[cfg(test)]
mod find {
    use super::*;

    #[test]
    #[ignore]
    fn pc_texture_rebuild() {
        let p = PathBuf::from(r"C:\Users\Andrew\Desktop\Saboteur\GAMEFILES\SUBPC\france\start0.kilopack");
        let mut bad = 0;
        super::t::packs_pub(&p, |d| {
            let Ok(pk) = parse(d) else { return };
            for b in Albs.bodies(d, &pk) {
                let Some(t) = tex::parse(b, pk.e) else { continue };
                let Some(dds) = tex::to_dds(&t, pk.e) else { continue };
                let (rec, _) = tex::from_dds(&dds, &t.name, t.flags, tex::Gpu::Pc).unwrap();
                if rec != b && bad < 5 {
                    bad += 1;
                    let at = rec.iter().zip(b).position(|(x, y)| x != y).unwrap_or(rec.len().min(b.len()));
                    eprintln!("{} {}x{} mips {} code {:x} streams {} len {} vs {} first diff at {at}", t.name, t.w, t.h, t.mips, t.code, t.streams.len(), rec.len(), b.len());
                }
            }
        });
        assert_eq!(bad, 0);
        let tmp = std::env::temp_dir().join("wst_find");
        let mut k = 0;
        super::t::packs_pub(&p, |d| {
            k += 1;
            if parse(d).is_err() {
                return;
            }
            let _ = std::fs::remove_dir_all(&tmp);
            let mut i = Input::from(&format!("{k}.pack"), d.to_vec());
            let mut o = Out::new(&tmp, Opts::default());
            Albs.unpack(&mut i, &mut o).unwrap();
            let meta = super::super::read_text(&tmp.join(INDEX)).unwrap();
            let mut s = Sink::mem();
            Albs.pack(&tmp, &meta, Opts::default(), &mut s).unwrap();
            let r = s.done().unwrap();
            if r != d && bad < 3 {
                bad += 1;
                let at = r.iter().zip(d).position(|(x, y)| x != y).unwrap_or(r.len().min(d.len()));
                eprintln!("pack #{k}: len {} vs {}, first diff at {at}", r.len(), d.len());
                let _ = std::fs::rename(&tmp, std::env::temp_dir().join(format!("wst_find_{k}")));
                std::fs::write(std::env::temp_dir().join(format!("wst_find_{k}.orig")), d).unwrap();
                std::fs::write(std::env::temp_dir().join(format!("wst_find_{k}.new")), &r).unwrap();
            }
        });
    }
}
