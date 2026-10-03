use super::arch::{entry, entry_path};
use super::{endian_name, header, meta, meta_endian, Format, Input, Item, Opts, Out, Sink, INDEX};
use crate::io::{err, hexs, unhexs};
use crate::names;
use ac_core::{Endian, Error, Reader, Res, Writer};
use ac_lua::Val;
use std::path::{Path, PathBuf};

const ALIGN: usize = 16;

struct Chunk {
    tag: [u8; 4],
    data: Vec<u8>,
}

fn endian(d: &[u8]) -> Option<Endian> {
    if d.get(..4)? != b"BKHD" {
        return None;
    }
    let n = d.get(4..8)?;
    let le = u32::from_le_bytes(n.try_into().ok()?);
    let be = u32::from_be_bytes(n.try_into().ok()?);
    Some(if le <= be { Endian::Le } else { Endian::Be })
}

fn chunks(d: &[u8]) -> Res<(Vec<Chunk>, Endian)> {
    let e = endian(d).ok_or(Error::Bad("not a Wwise sound bank (no BKHD)"))?;
    let mut r = Reader::new(d, e);
    let mut v = Vec::new();
    while r.left() > 0 {
        let tag: [u8; 4] = r.take(4)?.try_into().unwrap_or_default();
        if !tag.iter().all(|c| c.is_ascii_alphanumeric()) {
            return err("bad chunk tag");
        }
        let n = r.u32()? as usize;
        v.push(Chunk { tag, data: r.take(n)?.to_vec() });
    }
    Ok((v, e))
}

fn sounds(c: &[Chunk], e: Endian) -> Res<Option<Vec<(u32, Vec<u8>)>>> {
    let (Some(idx), Some(data)) = (c.iter().find(|c| &c.tag == b"DIDX"), c.iter().find(|c| &c.tag == b"DATA")) else { return Ok(None) };
    let mut r = Reader::new(&idx.data, e);
    let mut v = Vec::new();
    let mut pos = 0;
    while r.left() > 0 {
        let (id, at, n) = (r.u32()?, r.u32()? as usize, r.u32()? as usize);
        if at != pos || data.data.get(pos + n..(pos + n).next_multiple_of(ALIGN).min(data.data.len())).is_some_and(|p| p.iter().any(|&b| b != 0)) {
            return err("sound data is not laid out the usual way");
        }
        v.push((id, data.data.get(at..at + n).ok_or(Error::Bad("sound runs past DATA"))?.to_vec()));
        pos = (at + n).next_multiple_of(ALIGN);
    }
    if data.data.len() != idx_end(&idx.data, e) {
        return err("DATA has unexpected trailing bytes");
    }
    Ok(Some(v))
}

fn idx_end(d: &[u8], e: Endian) -> usize {
    let n = d.len() / 12;
    if n == 0 {
        return 0;
    }
    let at = |k: usize| e.get::<u32>(&d[12 * (n - 1) + k..]).unwrap_or(0) as usize;
    at(4) + at(8)
}

fn names_of(d: &[u8], e: Endian) -> Option<(u32, Vec<(u32, String)>)> {
    let mut r = Reader::new(d, e);
    let (kind, n) = (r.u32().ok()?, r.u32().ok()?);
    let mut v = Vec::new();
    for _ in 0..n {
        let id = r.u32().ok()?;
        let l = r.u8().ok()? as usize;
        v.push((id, String::from_utf8(r.take(l).ok()?.to_vec()).ok()?));
    }
    (r.left() == 0).then_some((kind, v))
}

fn tag_str(t: &[u8; 4]) -> String {
    String::from_utf8_lossy(t).into_owned()
}

pub struct Bnk;

impl Format for Bnk {
    fn id(&self) -> &'static str {
        "bnk"
    }
    fn about(&self) -> &'static str {
        "Wwise sound bank (sounds and their settings)"
    }
    fn exts(&self) -> &'static [&'static str] {
        &["bnk"]
    }
    fn folder(&self) -> bool {
        true
    }
    fn probe(&self, i: &mut Input) -> bool {
        i.len < 256 << 20 && i.all().is_ok_and(|d| chunks(d).is_ok())
    }
    fn info(&self, i: &mut Input) -> Res<Vec<(&'static str, String)>> {
        let (c, e) = chunks(i.all()?)?;
        let n = sounds(&c, e)?.map_or(0, |s| s.len());
        Ok(vec![("contents", format!("{} chunks, {n} sounds", c.len())), ("byte order", endian_name(e).into())])
    }
    fn list(&self, i: &mut Input) -> Res<Vec<Item>> {
        let (c, e) = chunks(i.all()?)?;
        Ok(sounds(&c, e)?.unwrap_or_default().iter().map(|(id, d)| Item { name: format!("0x{id:08x}.wem"), size: d.len() as u64, note: String::new() }).collect())
    }
    fn unpack(&self, i: &mut Input, o: &mut Out) -> Res<PathBuf> {
        let (c, e) = chunks(i.all()?)?;
        let name = i.name.clone();
        let mut t = header(&name, self.about());
        t.extend(meta(self.id(), &name, e));
        t.push((None, Val::Note("Chunks in bank order. Sounds: the embedded .wem files next to this index (file name = Wwise id);\nreplace or add .wem files freely. Names: the bank names it refers to. Other chunks are kept as files (HIRC.bin...)".into())));
        let mut order = Vec::new();
        let mut f = Vec::new();
        let snd = sounds(&c, e)?;
        for ch in &c {
            let tag = tag_str(&ch.tag);
            order.push((None, Val::Str(tag.clone())));
            match &ch.tag {
                b"BKHD" if ch.data.len() >= 8 => {
                    let mut r = Reader::new(&ch.data, e);
                    let (ver, id) = (r.u32()?, r.u32()?);
                    f.push((Some("Version".into()), Val::Int(ver as i64)));
                    f.push((Some("Id".into()), Val::Raw(format!("0x{id:08x}"))));
                    let rest = r.rest();
                    if rest.iter().all(|&b| b == 0) {
                        f.push((Some("HeaderZeros".into()), Val::Int(rest.len() as i64)));
                    } else {
                        f.push((Some("Header".into()), hexs(rest)));
                    }
                }
                b"DIDX" if snd.is_some() => {}
                b"DATA" if snd.is_some() => {
                    let list = snd.as_ref().map_or_else(Vec::new, |s| {
                        s.iter()
                            .map(|(id, d)| {
                                let rel = format!("0x{id:08x}.wem");
                                let _ = o.write(&rel, d);
                                (None, entry(&rel, Vec::new()))
                            })
                            .collect()
                    });
                    f.push((Some("Sounds".into()), Val::Tbl(list)));
                }
                b"STID" => match names_of(&ch.data, e) {
                    Some((kind, v)) => {
                        f.push((Some("NamesKind".into()), Val::Int(kind as i64)));
                        let l = v.iter().map(|(id, n)| (None, Val::Tbl(vec![(None, Val::Raw(format!("0x{id:08x}"))), (None, Val::Str(n.clone()))]))).collect();
                        f.push((Some("Names".into()), Val::Tbl(l)));
                    }
                    None => {
                        o.write(&format!("{tag}.bin"), &ch.data)?;
                    }
                },
                _ => {
                    o.write(&format!("{tag}.bin"), &ch.data)?;
                }
            }
        }
        t.push((Some("Chunks".into()), Val::Tbl(order)));
        t.extend(f);
        o.text(INDEX, &Val::Tbl(t))
    }
    fn pack(&self, src: &Path, meta: &Val, opts: Opts, out: &mut Sink) -> Res<()> {
        let e = meta_endian(meta, opts);
        let int = |k: &str| meta.key(k).and_then(Val::int).ok_or_else(|| Error::Msg(format!("index.lua needs {k} = number")));
        let num = |k: &str| -> Res<u32> {
            match meta.key(k) {
                Some(Val::Raw(r)) => r.strip_prefix("0x").and_then(|h| u32::from_str_radix(h, 16).ok()).ok_or_else(|| Error::Msg(format!("{k} must be 0x..."))),
                Some(Val::Str(s)) => Ok(names::key(s)),
                Some(v) => v.int().and_then(|i| u32::try_from(i).ok()).ok_or_else(|| Error::Msg(format!("{k} must be a number"))),
                None => Err(Error::Msg(format!("index.lua needs {k}"))),
            }
        };
        let mut listed: Vec<String> = Vec::new();
        if let Some(s) = meta.key("Sounds") {
            for (_, v) in s.items().iter().filter(|(_, v)| !matches!(v, Val::Note(_))) {
                listed.push(entry_path(v)?);
            }
        }
        let mut extra: Vec<String> = std::fs::read_dir(src)?
            .filter_map(|x| x.ok())
            .map(|x| x.file_name().to_string_lossy().into_owned())
            .filter(|n| n.to_lowercase().ends_with(".wem") && !listed.iter().any(|l| l.eq_ignore_ascii_case(n)))
            .collect();
        extra.sort();
        listed.extend(extra);
        let mut snd = Vec::new();
        for rel in &listed {
            let id = rel.strip_suffix(".wem").and_then(|s| s.strip_prefix("0x")).and_then(|h| u32::from_str_radix(h, 16).ok()).ok_or_else(|| Error::Msg(format!("{rel}: sound file name must be its Wwise id, like 0x1234abcd.wem")))?;
            snd.push((id, std::fs::read(ac_core::fs::join_safe(src, rel)?)?));
        }
        let order: Vec<String> = meta.key("Chunks").ok_or(Error::Bad("index.lua has no Chunks = { ... }"))?.items().iter().filter_map(|(_, v)| v.str().map(str::to_string)).collect();
        let mut w = Writer::new(e);
        for tag in &order {
            let body = match tag.as_str() {
                "BKHD" => {
                    let mut b = Writer::new(e);
                    b.u32(int("Version")? as u32).u32(num("Id")?);
                    match meta.key("Header") {
                        Some(h) => {
                            b.bytes(&unhexs(h).ok_or(Error::Bad("Header must be hex\"..\""))?);
                        }
                        None => {
                            b.pad(int("HeaderZeros").unwrap_or(0) as usize);
                        }
                    }
                    b.finish()
                }
                "DIDX" if !snd.is_empty() || meta.key("Sounds").is_some() => {
                    let mut b = Writer::new(e);
                    let mut at = 0usize;
                    for (id, d) in &snd {
                        b.u32(*id).u32(at as u32).u32(d.len() as u32);
                        at = (at + d.len()).next_multiple_of(ALIGN);
                    }
                    b.finish()
                }
                "DATA" if meta.key("Sounds").is_some() => {
                    let mut b = Vec::new();
                    for (k, (_, d)) in snd.iter().enumerate() {
                        b.extend_from_slice(d);
                        if k + 1 < snd.len() {
                            b.resize(b.len().next_multiple_of(ALIGN), 0);
                        }
                    }
                    b
                }
                "STID" if meta.key("Names").is_some() => {
                    let mut b = Writer::new(e);
                    let l = meta.key("Names").map(Val::items).unwrap_or(&[]);
                    b.u32(int("NamesKind").unwrap_or(1) as u32).u32(l.len() as u32);
                    for (_, x) in l {
                        let it = x.items();
                        let id = match it.first().map(|p| &p.1) {
                            Some(Val::Raw(r)) => r.strip_prefix("0x").and_then(|h| u32::from_str_radix(h, 16).ok()),
                            Some(v) => v.int().and_then(|i| u32::try_from(i).ok()),
                            None => None,
                        }
                        .ok_or(Error::Bad("Names entries are { 0xID, \"name\" }"))?;
                        let n = it.get(1).and_then(|p| p.1.str()).ok_or(Error::Bad("Names entries are { 0xID, \"name\" }"))?;
                        b.u32(id).u8(n.len() as u8).bytes(n.as_bytes());
                    }
                    b.finish()
                }
                t => std::fs::read(src.join(format!("{t}.bin"))).map_err(|x| Error::Msg(format!("{t}.bin: {x}")))?,
            };
            w.bytes(tag.as_bytes()).u32(body.len() as u32).bytes(&body);
        }
        out.put(&w.finish())
    }
}
