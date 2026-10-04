use super::arch::{clean, entry, entry_num, entry_path, uniq};
use super::{endian_name, header, meta, meta_endian, Format, Input, Item, Opts, Out, Sink, INDEX};
use crate::io::{err, Dump, Io, Len, Load, Mode, Prim, Read, Show, Str, Write};
use crate::names;
use ac_core::{Endian, Error, Reader, Res, Writer};
use ac_lua::Val;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

const RESIDENT: &str = "Resident.hkx";
const STREAMS: &str = "Streams";
const CONTAINER_BASE: u32 = 0x100;
const KEYS: [&str; 3] = ["Keys1", "Keys2", "Keys3"];

#[derive(Default)]
struct Event {
    hash1: u32,
    hash2: u32,
    hashes: [u32; 4],
    values: [f32; 4],
}

#[derive(Default)]
struct Point {
    flag: u8,
    pos: [f32; 3],
    value: f32,
}

#[derive(Default)]
struct Anim {
    id: u32,
    by_hash: u8,
    streamed: u8,
    name: String,
    value1: f32,
    bones: Vec<u32>,
    value2: f32,
    vector: [f32; 3],
    values: [f32; 4],
    flag: u8,
    events: Vec<Event>,
    points: Vec<Point>,
}

#[derive(Default)]
struct Interval {
    value: u32,
    a: u16,
    b: u16,
    anims: Vec<u32>,
}

#[derive(Default)]
struct Tail {
    value1: f32,
    vector: [f32; 3],
    value2: f32,
    hash: u32,
    range: [f32; 4],
    counts: [i8; 3],
    keys: [Vec<[f32; 2]>; 3],
    flag1: u8,
    flag2: u8,
}

#[derive(Default)]
struct Step {
    hash1: u32,
    hash2: u32,
    list1: Vec<u32>,
    list2: Vec<u32>,
}

#[derive(Default)]
struct Sequence {
    id: u32,
    steps: Vec<Step>,
    tail: Tail,
}

#[derive(Default)]
struct Move {
    hashes: [u32; 4],
    value1: f32,
    hash5: u32,
    value2: f32,
    count: u32,
    seq1: u32,
    seq2: u32,
    tail: Tail,
    hash6: u32,
    value3: f32,
    hash7: u32,
    text: String,
}

#[derive(Default)]
struct Transition {
    id: u32,
    moves: Vec<Move>,
}

#[derive(Default)]
struct BankEntry {
    hash1: u32,
    hash2: u32,
    list: Vec<u32>,
}

#[derive(Default)]
struct Bank {
    id: u32,
    name: String,
    parent: u32,
    entries: Vec<BankEntry>,
}

#[derive(Default)]
struct Part {
    anims: Vec<Anim>,
    resident: u32,
    value: u32,
    intervals: Vec<Interval>,
    sequences: Vec<Sequence>,
    transitions: Vec<Transition>,
    edge: Vec<u32>,
    edge2: Vec<u32>,
    dist: Vec<f32>,
    banks: Vec<Bank>,
    add: Vec<[u32; 3]>,
    alpha1: Vec<u32>,
    alpha2: Vec<u32>,
    ssp: Vec<(u32, [u32; 16])>,
    pairs: Vec<[u32; 2]>,
}

fn hashes<S: Io>(s: &mut S, k: &str, v: &mut Vec<u32>) -> Res<()> {
    if s.skip(k, v.is_empty()) {
        return Ok(());
    }
    s.hashes(k, v, Len::U32)
}

fn named<S: Io>(s: &mut S, id: &mut u32, name: &mut String, f: Str) -> Res<()> {
    if s.mode() == Mode::Load {
        s.text("Name", name, f)?;
        return s.named("Id", id, name);
    }
    s.named("Id", id, name)?;
    s.text("Name", name, f)
}

fn tail<S: Io>(s: &mut S, t: &mut Tail) -> Res<()> {
    s.f32("Value1", &mut t.value1)?;
    s.opt("Vector", &mut t.vector, Show::Dec)?;
    s.f32("Value2", &mut t.value2)?;
    s.hash("Hash", &mut t.hash)?;
    s.opt("Range", &mut t.range, Show::Dec)?;
    if s.mode() == Mode::Write {
        for (c, k) in t.counts.iter_mut().zip(&t.keys) {
            *c = i8::try_from(k.len()).or_else(|_| err("at most 127 keys in a list"))?;
        }
    }
    s.hide(&mut t.counts)?;
    for (i, k) in t.keys.iter_mut().enumerate() {
        if s.skip(KEYS[i], k.is_empty()) {
            continue;
        }
        let n = s.n(t.counts[i].max(0) as u32);
        s.list(KEYS[i], k, n, |s, x| s.vec("", x))?;
    }
    s.opt("Flag1", &mut t.flag1, Show::Bool)?;
    s.opt("Flag2", &mut t.flag2, Show::Bool)
}

impl Part {
    fn head<S: Io>(&mut self, s: &mut S) -> Res<()> {
        s.magic(b"L0PA")?;
        s.magic(b"MINA")?;
        s.note("Animations: Name; Streamed = the animation is a file in Streams/, otherwise it is inside Resident.hkx.\nBones: bone track list (indices, -1 = none, or name hashes when BoneHashes = true). Events: Key, Type, Args, Values.");
        s.list("Animations", &mut self.anims, Len::U32, |s, a| {
            if s.mode() == Mode::Load {
                s.text("Name", &mut a.name, Str::L32)?;
                s.opt("BoneHashes", &mut a.by_hash, Show::Bool)?;
                s.opt("Streamed", &mut a.streamed, Show::Bool)?;
                s.named("Id", &mut a.id, &a.name)?;
            } else {
                s.named("Id", &mut a.id, &a.name)?;
                s.opt("BoneHashes", &mut a.by_hash, Show::Bool)?;
                s.opt("Streamed", &mut a.streamed, Show::Bool)?;
                s.text("Name", &mut a.name, Str::L32)?;
            }
            s.f32("Value1", &mut a.value1)?;
            if a.streamed == 0 {
                let hashed = a.by_hash != 0;
                s.list("Bones", &mut a.bones, Len::U32, |s, b| {
                    if hashed {
                        return s.hash("", b);
                    }
                    let mut i = *b as i32;
                    s.i32("", &mut i)?;
                    *b = i as u32;
                    Ok(())
                })?;
            }
            s.f32("Value2", &mut a.value2)?;
            s.opt("Vector", &mut a.vector, Show::Dec)?;
            s.opt("Values", &mut a.values, Show::Dec)?;
            s.or("Flag", &mut a.flag, Show::Bool, 1)?;
            if !s.skip("Events", a.events.is_empty()) {
                s.list("Events", &mut a.events, Len::U32, |s, e| {
                    s.u32("Key", &mut e.hash1)?;
                    s.hash("Type", &mut e.hash2)?;
                    if s.bin() {
                        for i in 0..4 {
                            s.hash("", &mut e.hashes[i])?;
                            s.f32("", &mut e.values[i])?;
                        }
                        return Ok(());
                    }
                    s.val("Args", &mut e.hashes, Show::Hash)?;
                    s.opt("Values", &mut e.values, Show::Dec)
                })?;
            }
            if s.skip("Points", a.points.is_empty()) {
                return Ok(());
            }
            s.list("Points", &mut a.points, Len::U32, |s, p| {
                s.flag("Flag", &mut p.flag)?;
                s.vec("Position", &mut p.pos)?;
                s.f32("Value", &mut p.value)
            })
        })
    }

    fn resident_count(&self) -> u32 {
        self.anims.iter().filter(|a| a.streamed == 0).count() as u32
    }

    fn body<S: Io>(&mut self, s: &mut S, dlc: bool) -> Res<()> {
        s.hex("Value", &mut self.value)?;
        s.magic(b"VTNI")?;
        s.list("Intervals", &mut self.intervals, Len::U32, |s, x| {
            s.hash("Value", &mut x.value)?;
            s.u16("A", &mut x.a)?;
            s.u16("B", &mut x.b)?;
            s.hashes("Animations", &mut x.anims, Len::U32)
        })?;
        s.magic(b"CQES")?;
        s.list("Sequences", &mut self.sequences, Len::U32, |s, q| {
            s.hash("Id", &mut q.id)?;
            s.list("Steps", &mut q.steps, Len::U32, |s, x| {
                s.hash("Hash1", &mut x.hash1)?;
                s.hash("Hash2", &mut x.hash2)?;
                hashes(s, "List1", &mut x.list1)?;
                hashes(s, "List2", &mut x.list2)
            })?;
            tail(s, &mut q.tail)
        })?;
        s.magic(b"NART")?;
        s.list("Transitions", &mut self.transitions, Len::U32, |s, t| {
            s.hash("Id", &mut t.id)?;
            s.list("Moves", &mut t.moves, Len::U32, |s, m| {
                s.val("Hashes", &mut m.hashes, Show::Hash)?;
                s.f32("Value1", &mut m.value1)?;
                s.hash("Hash5", &mut m.hash5)?;
                s.f32("Value2", &mut m.value2)?;
                if s.bin() {
                    s.u32("", &mut m.count)?;
                    if m.count != 0 {
                        s.hash("", &mut m.seq1)?;
                        s.hash("", &mut m.seq2)?;
                        tail(s, &mut m.tail)?;
                    }
                } else if !s.skip("Sequence", m.count == 0) {
                    s.node("Sequence", |s| {
                        s.u32("Count", &mut m.count)?;
                        s.hash("Hash1", &mut m.seq1)?;
                        s.hash("Hash2", &mut m.seq2)?;
                        tail(s, &mut m.tail)
                    })?;
                }
                s.hash("Hash6", &mut m.hash6)?;
                s.f32("Value3", &mut m.value3)?;
                s.hash("Hash7", &mut m.hash7)?;
                s.text("Text", &mut m.text, Str::L16Z0)
            })
        })?;
        if !dlc {
            s.magic(b"EGDE")?;
            s.hashes("Edge", &mut self.edge, Len::N(0x4920 / 4))?;
            s.hashes("Edge2", &mut self.edge2, Len::N(0x578 / 4))?;
            s.magic(b"TSID")?;
            s.floats("Dist", &mut self.dist, Len::N(0x60 / 4))?;
        }
        s.magic(b"KNAB")?;
        s.list("Banks", &mut self.banks, Len::U32, |s, b| {
            named(s, &mut b.id, &mut b.name, Str::L16Z0)?;
            s.hash("Parent", &mut b.parent)?;
            s.list("Entries", &mut b.entries, Len::U32, |s, x| {
                s.hash("Hash1", &mut x.hash1)?;
                s.hash("Hash2", &mut x.hash2)?;
                hashes(s, "List", &mut x.list)
            })
        })?;
        s.magic(b"1DDA")?;
        s.list("Add", &mut self.add, Len::U32, |s, x| s.val("", x, Show::Hash))?;
        s.magic(b"HPLA")?;
        s.hashes("Alpha1", &mut self.alpha1, Len::U32)?;
        s.hashes("Alpha2", &mut self.alpha2, Len::U32)?;
        s.magic(b"0PSS")?;
        if !dlc {
            s.list("Ssp", &mut self.ssp, Len::U32, |s, x| {
                s.hash("Hash", &mut x.0)?;
                s.val("Values", &mut x.1, Show::Hash)
            })?;
        }
        s.list("Pairs", &mut self.pairs, Len::U32, |s, x| s.val("", x, Show::Hash))
    }
}

struct Stream {
    hash: u32,
    data: Vec<u8>,
}

struct Group {
    hash: u32,
    items: Vec<u32>,
}

struct Pack {
    part: Part,
    blob: Vec<u8>,
    streams: Vec<Stream>,
    groups: Vec<Group>,
}

fn entries(r: &mut Reader, n: usize) -> Res<Vec<(u32, u32, u32)>> {
    r.list(n, |r| Ok((r.u32()?, r.u32()?, r.u32()?)))
}

fn read_part(d: &[u8], e: Endian, dlc: bool) -> Res<Pack> {
    let mut part = Part::default();
    let mut s = Read::new(d, e);
    part.head(&mut s)?;
    s.val("", &mut part.resident, Show::Dec)?;
    let n = s.r.u32()? as usize;
    let blob = s.r.take(n)?.to_vec();
    part.body(&mut s, dlc)?;
    let r = &mut s.r;
    let a = r.u32()? as usize;
    let groups_t = entries(r, a)?;
    let (b, c) = (r.u32()? as usize, r.u32()? as usize);
    let single = entries(r, b)?;
    let grouped = entries(r, c)?;
    let end = r.pos();
    let mut order = single.clone();
    order.sort_by_key(|x| x.2);
    let mut at = end;
    let mut streams = Vec::with_capacity(order.len());
    for &(hash, size, off) in &order {
        if off as usize != at {
            return err(format!("stream 0x{hash:08x} is not where the table says"));
        }
        let data = d.get(at..at + size as usize).ok_or(Error::Bad("stream past the end of the file"))?.to_vec();
        at += size as usize;
        streams.push(Stream { hash, data });
    }
    let by: HashMap<u32, usize> = streams.iter().enumerate().map(|(i, x)| (x.hash, i)).collect();
    let mut groups = Vec::with_capacity(groups_t.len());
    for &(hash, count, first) in &groups_t {
        let from = (first as usize).checked_sub(b).ok_or(Error::Bad("group points into the single streams"))?;
        let mut items = Vec::new();
        for &(h, size, off) in grouped.get(from..from + count as usize).ok_or(Error::Bad("group is past the table"))? {
            let i = *by.get(&h).ok_or_else(|| Error::Msg(format!("group stream 0x{h:08x} has no single copy")))?;
            if off as usize != at || d.get(at..at + size as usize) != Some(&streams[i].data[..]) {
                return err(format!("group stream 0x{h:08x} differs from its single copy"));
            }
            at += size as usize;
            items.push(h);
        }
        groups.push(Group { hash, items });
    }
    if at != d.len() {
        return err(format!("{} unexpected bytes at the end", d.len() - at));
    }
    Ok(Pack { part, blob, streams, groups })
}

fn write_part(p: &mut Pack, e: Endian, dlc: bool) -> Res<Vec<u8>> {
    let mut s = Write::new(e);
    p.part.head(&mut s)?;
    s.val("", &mut p.part.resident, Show::Dec)?;
    s.w.u32(p.blob.len() as u32).bytes(&p.blob);
    p.part.body(&mut s, dlc)?;
    let mut w = s.w;
    p.groups.sort_by_key(|g| g.hash);
    let by: HashMap<u32, usize> = p.streams.iter().enumerate().map(|(i, x)| (x.hash, i)).collect();
    let grouped: usize = p.groups.iter().map(|g| g.items.len()).sum();
    let start = w.pos() + 4 + p.groups.len() * 12 + 8 + (p.streams.len() + grouped) * 12;
    let mut at = start as u32;
    let mut single = Vec::with_capacity(p.streams.len());
    for x in &p.streams {
        single.push((x.hash, x.data.len() as u32, at));
        at = at.checked_add(x.data.len() as u32).ok_or(Error::Bad("animations are larger than 4 GB"))?;
    }
    let mut list = Vec::new();
    let mut table = Vec::new();
    for g in &p.groups {
        table.push((g.hash, g.items.len() as u32, (p.streams.len() + list.len()) as u32));
        for h in &g.items {
            let i = *by.get(h).ok_or_else(|| Error::Msg(format!("group {} lists {}, which is not in Streams", names::label(g.hash), names::label(*h))))?;
            let n = p.streams[i].data.len() as u32;
            list.push((*h, n, at, i));
            at = at.checked_add(n).ok_or(Error::Bad("animations are larger than 4 GB"))?;
        }
    }
    single.sort_by_key(|x| x.0);
    if let Some(x) = single.windows(2).find(|x| x[0].0 == x[1].0) {
        return err(format!("two streams have the hash 0x{:08x}", x[0].0));
    }
    w.u32(table.len() as u32);
    for (h, n, f) in &table {
        w.u32(*h).u32(*n).u32(*f);
    }
    w.u32(single.len() as u32).u32(list.len() as u32);
    for (h, n, o) in &single {
        w.u32(*h).u32(*n).u32(*o);
    }
    for (h, n, o, _) in &list {
        w.u32(*h).u32(*n).u32(*o);
    }
    let mut out = w.finish();
    for x in &p.streams {
        out.extend_from_slice(&x.data);
    }
    for (_, _, _, i) in &list {
        out.extend_from_slice(&p.streams[*i].data);
    }
    Ok(out)
}

fn label(h: u32) -> String {
    match names::show(h) {
        Val::Str(s) => s,
        _ => format!("0x{h:08x}"),
    }
}

fn kv(k: &str, v: Val) -> (Option<String>, Val) {
    (Some(k.into()), v)
}

fn unpack_part(p: &mut Pack, e: Endian, dlc: bool, dir: &str, o: &mut Out) -> Res<Vec<(Option<String>, Val)>> {
    let mut s = Dump::new(e);
    p.part.head(&mut s)?;
    let count = p.part.resident_count();
    s.or("ResidentCount", &mut p.part.resident, Show::Dec, count)?;
    p.part.body(&mut s, dlc)?;
    let resident = format!("{dir}{RESIDENT}");
    o.write(&resident, &p.blob)?;
    let mut t = vec![kv("Resident", Val::Str(resident))];
    t.extend(s.finish());
    let mut used = HashSet::new();
    let mut files = Vec::with_capacity(p.streams.len());
    for x in &p.streams {
        let rel = uniq(&mut used, &format!("{dir}{STREAMS}/{}.hvk", clean(&label(x.hash))));
        o.write(&rel, &x.data)?;
        let stem = rel.rsplit('/').next().unwrap_or(&rel).trim_end_matches(".hvk");
        let extra = if names::hash(stem) != x.hash { vec![kv("Hash", Val::Raw(format!("0x{:08x}", x.hash)))] } else { Vec::new() };
        files.push((None, entry(&rel, extra)));
    }
    t.push((None, Val::Note("Streams: streamed animations in pack order, one .hvk file each ('ANMA', compression type,
Havok packfile, bone hashes). New .hvk files in Streams/ are added at the end. Hash: when it is not the file name".into())));
    t.push(kv("Streams", Val::Tbl(files)));
    t.push((None, Val::Note("Groups: streams loaded together (a second copy is stored after all streams)".into())));
    let groups = p.groups.iter().map(|g| (None, Val::Tbl(std::iter::once(g.hash).chain(g.items.iter().copied()).map(|h| (None, names::show(h))).collect()))).collect();
    t.push(kv("Groups", Val::Tbl(groups)));
    Ok(t)
}

const OWN: [&str; 4] = ["Resident", "Streams", "Groups", "ResidentCount"];

fn pack_part(src: &Path, v: &Val, e: Endian, dlc: bool, dir: &str) -> Res<Pack> {
    let fields = Val::Tbl(v.items().iter().filter(|(k, _)| !matches!(k.as_deref(), Some(k) if OWN.contains(&k) || matches!(k, "Format" | "File" | "Endian" | "Base"))).cloned().collect());
    let mut part = Part::default();
    let mut s = Load::new(fields, e);
    part.head(&mut s)?;
    part.body(&mut s, dlc)?;
    s.rest()?;
    part.resident = match v.key("ResidentCount") {
        Some(x) => u32::load(x).ok_or(Error::Bad("ResidentCount must be a number"))?,
        None => part.resident_count(),
    };
    let resident = v.key("Resident").and_then(Val::str).map_or_else(|| format!("{dir}{RESIDENT}"), str::to_string);
    let blob = std::fs::read(ac_core::fs::join_safe(src, &resident)?).map_err(|x| Error::Msg(format!("cannot read {resident}: {x}")))?;
    let mut streams = Vec::new();
    let mut have = HashSet::new();
    for (_, x) in v.key("Streams").map(Val::items).unwrap_or(&[]).iter().filter(|(_, x)| !matches!(x, Val::Note(_))) {
        let rel = entry_path(x)?;
        have.insert(rel.to_lowercase());
        let stem = rel.rsplit('/').next().unwrap_or(&rel).trim_end_matches(".hvk").to_string();
        let hash = entry_num(x, "Hash")?.unwrap_or_else(|| names::key(&stem));
        let data = std::fs::read(ac_core::fs::join_safe(src, &rel)?).map_err(|x| Error::Msg(format!("cannot read {rel}: {x}")))?;
        streams.push(Stream { hash, data });
    }
    let folder = src.join(format!("{dir}{STREAMS}"));
    if folder.is_dir() {
        let mut fresh: Vec<String> = ac_core::fs::walk(&folder)?
            .iter()
            .map(|p| p.strip_prefix(src).unwrap_or(p).to_string_lossy().replace('\\', "/"))
            .filter(|r| r.to_lowercase().ends_with(".hvk") && !have.contains(&r.to_lowercase()))
            .collect();
        fresh.sort();
        for rel in fresh {
            let stem = rel.rsplit('/').next().unwrap_or(&rel).trim_end_matches(".hvk").to_string();
            streams.push(Stream { hash: names::key(&stem), data: std::fs::read(src.join(&rel))? });
        }
    }
    let mut groups = Vec::new();
    for (i, (_, g)) in v.key("Groups").map(Val::items).unwrap_or(&[]).iter().filter(|(_, x)| !matches!(x, Val::Note(_))).enumerate() {
        let h: Option<Vec<u32>> = g.items().iter().map(|(_, x)| u32::load(x)).collect();
        let h = h.filter(|h| !h.is_empty()).ok_or_else(|| Error::Msg(format!("Groups[{}]: expected {{ group name, stream names... }}", i + 1)))?;
        groups.push(Group { hash: h[0], items: h[1..].to_vec() });
    }
    Ok(Pack { part, blob, streams, groups })
}

fn head(i: &mut Input) -> Res<(Endian, bool)> {
    let h = i.head(8);
    match h.get(..8) {
        Some(b"L0PAMINA") => Ok((Endian::Le, false)),
        Some(b"AP0LANIM") => Ok((Endian::Be, false)),
        _ => match h.get(..4) {
            Some(b"0KCP") => Ok((Endian::Le, true)),
            Some(b"PCK0") => Ok((Endian::Be, true)),
            _ => err("not an animations pack (no AP0L ANIM or PCK0 magic)"),
        },
    }
}

fn sections(d: &[u8], e: Endian) -> Res<(u32, Vec<&[u8]>)> {
    let mut r = Reader::new(d, e);
    r.take(4)?;
    let base = r.u32()?;
    let n = r.u32()? as usize;
    if !(1..8).contains(&n) {
        return err("the game reads 1 to 7 sections");
    }
    let sizes = r.list(n, |r| r.u32())?;
    let mut at = base as usize;
    for &s in &sizes {
        if r.u32()? as usize != at {
            return err("section offsets in the header do not follow the sizes");
        }
        at += s as usize;
    }
    let head = d.get(r.pos()..base as usize).ok_or(Error::Bad("sections start inside the header"))?;
    if head.iter().any(|&b| b != 0) {
        return err("header padding is not zero");
    }
    let mut at = base as usize;
    let mut v = Vec::new();
    for s in sizes {
        v.push(d.get(at..at + s as usize).ok_or(Error::Bad("section past the end of the file"))?);
        at += s as usize;
    }
    if at != d.len() {
        return err(format!("{} unexpected bytes at the end", d.len() - at));
    }
    Ok((base, v))
}

pub struct Animations;

impl Format for Animations {
    fn id(&self) -> &'static str {
        "animations"
    }
    fn about(&self) -> &'static str {
        "character animations (Havok) with their sets, sequences and transitions"
    }
    fn exts(&self) -> &'static [&'static str] {
        &["pack"]
    }
    fn folder(&self) -> bool {
        true
    }
    fn probe(&self, i: &mut Input) -> bool {
        head(i).is_ok()
    }
    fn info(&self, i: &mut Input) -> Res<Vec<(&'static str, String)>> {
        let (e, dlc) = head(i)?;
        let d = i.all()?;
        let parts = if dlc { sections(d, e)?.1 } else { vec![d] };
        let mut n = (0, 0);
        for p in parts {
            let x = read_part(p, e, dlc)?;
            n.0 += x.part.anims.len();
            n.1 += x.streams.len();
        }
        Ok(vec![("contents", format!("{} animations, {} streamed files", n.0, n.1)), ("byte order", endian_name(e).into())])
    }
    fn list(&self, i: &mut Input) -> Res<Vec<Item>> {
        let (e, dlc) = head(i)?;
        let d = i.all()?;
        let parts = if dlc { sections(d, e)?.1 } else { vec![d] };
        let mut v = Vec::new();
        for p in parts {
            let x = read_part(p, e, dlc)?;
            v.extend(x.streams.iter().map(|s| Item { name: format!("{}.hvk", label(s.hash)), size: s.data.len() as u64, note: String::new() }));
        }
        Ok(v)
    }
    fn unpack(&self, i: &mut Input, o: &mut Out) -> Res<PathBuf> {
        let (e, dlc) = head(i)?;
        let name = i.name.clone();
        let d = i.all()?.to_vec();
        let mut t = header(&name, self.about());
        t.extend(meta(self.id(), &name, e));
        t.push((None, Val::Note("Resident.hkx: Havok packfile (Havok 6.5) with the animations that are always loaded.\n.hvk: streamed record around a Havok packfile".into())));
        if dlc {
            let (base, parts) = sections(&d, e)?;
            if base != CONTAINER_BASE {
                t.push(kv("Base", Val::Int(base as i64)));
            }
            let mut list = Vec::new();
            for (n, p) in parts.into_iter().enumerate() {
                let mut x = read_part(p, e, true).map_err(|m| Error::Msg(format!("section {}: {m}", n + 1)))?;
                list.push((None, Val::Tbl(unpack_part(&mut x, e, true, &format!("Section{}/", n + 1), o)?)));
            }
            t.push(kv("Sections", Val::Tbl(list)));
        } else {
            let mut x = read_part(&d, e, false)?;
            t.extend(unpack_part(&mut x, e, false, "", o)?);
        }
        o.text(INDEX, &Val::Tbl(t))
    }
    fn pack(&self, src: &Path, meta: &Val, opts: Opts, out: &mut Sink) -> Res<()> {
        let e = meta_endian(meta, opts);
        let Some(list) = meta.key("Sections") else {
            let mut p = pack_part(src, meta, e, false, "")?;
            return out.put(&write_part(&mut p, e, false)?);
        };
        let mut parts = Vec::new();
        for (n, (_, v)) in list.items().iter().filter(|(_, x)| !matches!(x, Val::Note(_))).enumerate() {
            let at = |m: Error| Error::Msg(format!("Sections[{}]: {m}", n + 1));
            let mut p = pack_part(src, v, e, true, &format!("Section{}/", n + 1)).map_err(at)?;
            parts.push(write_part(&mut p, e, true).map_err(at)?);
        }
        let base = meta.key("Base").and_then(Val::int).map_or(CONTAINER_BASE, |b| b as u32);
        let mut w = Writer::new(e);
        w.bytes(if e == Endian::Le { b"0KCP" } else { b"PCK0" }).u32(base).u32(parts.len() as u32);
        for p in &parts {
            w.u32(p.len() as u32);
        }
        let mut at = base;
        for p in &parts {
            w.u32(at);
            at += p.len() as u32;
        }
        if w.pos() > base as usize {
            return err("too many sections for the header");
        }
        let pad = base as usize - w.pos();
        w.pad(pad);
        out.put(&w.finish())?;
        for p in &parts {
            out.put(p)?;
        }
        Ok(())
    }
}
