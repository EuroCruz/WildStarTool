use super::{endian_name, header, meta, meta_endian, Format, Input, Item, Opts, Out, Sink, INDEX};
use crate::io::err;
use ac_core::{Endian, Error, Reader, Res, Writer};
use ac_lua::Val;
use std::path::{Path, PathBuf};

const ALIGN: u64 = 2048;
const DIRS: [&str; 2] = ["Banks", "Streams"];

struct Head {
    e: Endian,
    unknown: u32,
    lists: [Vec<(u32, u32, u32)>; 2],
}

fn head(i: &mut Input) -> Res<Head> {
    let h = i.head(28);
    let e = match h.get(..4) {
        Some(b"1KCP") => Endian::Le,
        Some(b"PCK1") => Endian::Be,
        _ => return err("not a sound package (no PCK1 magic)"),
    };
    let mut r = Reader::new(&h, e);
    r.skip(4)?;
    if r.u32()? as u64 != ALIGN {
        return err("unexpected block size");
    }
    let unknown = r.u32()?;
    let mut lists: [Vec<(u32, u32, u32)>; 2] = Default::default();
    for l in &mut lists {
        let (n, at) = (r.u32()? as usize, r.u32()? as u64);
        if at + n as u64 * 12 > i.len {
            return err("entry table is larger than the file");
        }
        let t = i.at(at, n * 12)?;
        let mut t = Reader::new(&t, e);
        *l = t.list(n, |t| Ok((t.u32()?, t.u32()?, t.u32()?)))?;
        if let Some(x) = l.iter().find(|x| x.2 as u64 + x.1 as u64 > i.len) {
            return err(format!("entry 0x{:08x} runs past the end of the file", x.0));
        }
    }
    Ok(Head { e, unknown, lists })
}

enum Body {
    File(u64, PathBuf),
    Mem(Vec<u8>),
}

impl Body {
    fn len(&self) -> u64 {
        match self {
            Body::File(n, _) => *n,
            Body::Mem(d) => d.len() as u64,
        }
    }
}

fn ext(d: &[u8]) -> &'static str {
    match d.get(..4) {
        Some(b"BKHD") => ".bnk",
        Some(b"RIFF" | b"RIFX") => ".wem",
        _ => ".bin",
    }
}

fn id_of(name: &str) -> Option<u32> {
    let s = name.rsplit_once('.').map_or(name, |x| x.0);
    u32::from_str_radix(s.strip_prefix("0x")?, 16).ok().filter(|_| s.len() == 10)
}

pub struct Pck;

impl Format for Pck {
    fn id(&self) -> &'static str {
        "pck"
    }
    fn about(&self) -> &'static str {
        "Wwise sound package (sound banks and streamed sounds)"
    }
    fn exts(&self) -> &'static [&'static str] {
        &["pck"]
    }
    fn folder(&self) -> bool {
        true
    }
    fn probe(&self, i: &mut Input) -> bool {
        head(i).is_ok()
    }
    fn info(&self, i: &mut Input) -> Res<Vec<(&'static str, String)>> {
        let h = head(i)?;
        Ok(vec![("contents", format!("{} banks, {} streams", h.lists[0].len(), h.lists[1].len())), ("byte order", endian_name(h.e).into())])
    }
    fn list(&self, i: &mut Input) -> Res<Vec<Item>> {
        let h = head(i)?;
        Ok(DIRS.iter().zip(&h.lists).flat_map(|(d, l)| l.iter().map(move |x| Item { name: format!("{d}/0x{:08x}", x.0), size: x.1 as u64, note: String::new() })).collect())
    }
    fn unpack(&self, i: &mut Input, o: &mut Out) -> Res<PathBuf> {
        let h = head(i)?;
        for (d, l) in DIRS.iter().zip(&h.lists) {
            for &(id, size, at) in l {
                let b = i.at(at as u64, size as usize)?;
                let rel = format!("{d}/0x{id:08x}{}", ext(&b));
                if rel.ends_with(".bnk") {
                    o.child(&rel, b)?;
                } else {
                    o.write(&rel, &b)?;
                }
            }
        }
        let name = i.name.clone();
        let mut t = header(&name, self.about());
        t.extend(meta(self.id(), &name, h.e));
        t.push((None, Val::Note("Banks/ and Streams/ next to this index hold the sounds; a file name is its Wwise id (0x...).\nAdd, replace or delete files freely — they are sorted by id when packing. A bank (.bnk) is unpacked into a folder.".into())));
        if h.unknown != 0 {
            t.push((Some("Unknown".into()), Val::Int(h.unknown as i64)));
        }
        o.text(INDEX, &Val::Tbl(t))
    }
    fn pack(&self, src: &Path, meta: &Val, opts: Opts, out: &mut Sink) -> Res<()> {
        let e = meta_endian(meta, opts);
        let unknown = match meta.key("Unknown") {
            None => 0,
            Some(v) => v.int().and_then(|x| u32::try_from(x).ok()).ok_or(Error::Bad("Unknown must be a whole number"))?,
        };
        let mut lists: [Vec<(u32, Body)>; 2] = Default::default();
        for (d, l) in DIRS.iter().zip(&mut lists) {
            let dir = src.join(d);
            if !dir.is_dir() {
                continue;
            }
            for u in super::arch::new_units(&dir, &[])? {
                let id = id_of(&u).ok_or_else(|| Error::Msg(format!("{d}/{u}: name must be the Wwise id, like 0x1234abcd.wem")))?;
                let p = dir.join(&u);
                let body = if p.is_file() { Body::File(std::fs::metadata(&p)?.len(), p) } else { Body::Mem(super::build(&p, opts)?) };
                l.push((id, body));
            }
            l.sort_by_key(|x| x.0);
            if let Some(w) = l.windows(2).find(|w| w[0].0 == w[1].0) {
                return err(format!("{d}: two files have the id 0x{:08x}", w[0].0));
            }
        }
        let (nb, ns) = (lists[0].len() as u64, lists[1].len() as u64);
        let mut at = (28 + (nb + ns) * 12).next_multiple_of(ALIGN);
        let mut w = Writer::new(e);
        w.bytes(if e == Endian::Le { b"1KCP" } else { b"PCK1" }).u32(ALIGN as u32).u32(unknown);
        w.u32(nb as u32).u32(28).u32(ns as u32).u32(28 + nb as u32 * 12);
        for (id, b) in lists.iter().flatten() {
            let size = u32::try_from(b.len()).map_err(|_| Error::Bad("sound file larger than 4 GB"))?;
            w.u32(*id).u32(size).u32(u32::try_from(at).map_err(|_| Error::Bad("package larger than 4 GB"))?);
            at = (at + size as u64).next_multiple_of(ALIGN);
        }
        out.put(&w.finish())?;
        for (_, b) in lists.iter().flatten() {
            out.pad(out.len.next_multiple_of(ALIGN), 0)?;
            match b {
                Body::File(_, p) => out.put(&std::fs::read(p)?)?,
                Body::Mem(d) => out.put(d)?,
            }
        }
        out.pad(out.len.next_multiple_of(ALIGN), 0)
    }
}
