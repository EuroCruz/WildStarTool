use super::{endian_name, header, meta, meta_endian, Data, Format, Input, Opts, Out, Sink};
use crate::io::{err, Io, Show};
use crate::names;
use ac_core::{Endian, Error, Reader, Res, Writer};
use ac_lua::Val;
use std::path::{Path, PathBuf};

const VERSION: u32 = 5;

fn endian(d: &[u8]) -> Option<Endian> {
    match d.get(..4)? {
        [5, 0, 0, 0] => Some(Endian::Le),
        [0, 0, 0, 5] => Some(Endian::Be),
        _ => None,
    }
}

#[derive(Default)]
pub struct RandomText {
    groups: Vec<Group>,
}

#[derive(Default)]
struct Group {
    id: u32,
    lines: Vec<(u32, f32)>,
}

impl RandomText {
    fn walk<S: Io>(&mut self, s: &mut S) -> Res<()> {
        s.konst(VERSION)?;
        let mut n = [self.groups.len() as u32, self.groups.iter().map(|g| g.lines.len() as u32).sum()];
        s.hide(&mut n)?;
        s.note("Random lines: each group picks one of its Lines (text id, Weight) at random; up to 20 lines per group");
        s.tagged("Groups", &mut self.groups, b"TXTR", b"DNEC", |s, g| {
            s.hash("Id", &mut g.id)?;
            s.tagged("Lines", &mut g.lines, b"DNAR", b"DNEC", |s, l| {
                s.hash("Text", &mut l.0)?;
                s.or("Weight", &mut l.1, Show::Dec, 0.0)
            })
        })
    }
}

impl Data for RandomText {
    const ID: &'static str = "randomtext";
    const ABOUT: &'static str = "random dialog lines";
    const EXT: &'static [&'static str] = &["rnd"];
    fn io<S: Io>(&mut self, s: &mut S) -> Res<()> {
        self.walk(s)
    }
    fn size(&self) -> String {
        format!("{} groups", self.groups.len())
    }
    fn endian(d: &[u8]) -> Option<Endian> {
        endian(d)
    }
}

struct Line {
    id: u32,
    name: String,
    text: Vec<u16>,
}

struct Doc {
    lines: Vec<Line>,
    groups: Vec<(u32, Vec<Line>)>,
}

fn read_lines(r: &mut Reader) -> Res<Vec<Line>> {
    let mut v = Vec::new();
    loop {
        match &r.take(4)?.to_vec()[..] {
            b"TXTD" | b"DTXT" => {}
            b"DNEC" | b"CEND" => return Ok(v),
            t => return err(format!("line {}: unexpected tag {:?}", v.len() + 1, String::from_utf8_lossy(t))),
        }
        let id = r.u32()?;
        let n = r.u16()? as usize;
        let name = r.take(n)?;
        let name = String::from_utf8_lossy(name.strip_suffix(&[0]).ok_or(Error::Bad("line name is not zero-terminated"))?).into_owned();
        let n = r.u16()? as usize;
        let mut text = r.list(n, |r| r.u16())?;
        if text.pop() != Some(0) {
            return err(format!("line 0x{id:08x}: text is not zero-terminated"));
        }
        v.push(Line { id, name, text });
    }
}

fn chars(v: &[Line]) -> usize {
    v.iter().map(|l| l.text.len() + 1).sum()
}

fn parse(d: &[u8]) -> Res<(Doc, Endian)> {
    let e = endian(d).ok_or(Error::Bad("not a game text file (version is not 5)"))?;
    let mut r = Reader::new(d, e);
    r.skip(4)?;
    let (n, total) = (r.u32()? as usize, r.u32()? as usize);
    let lines = read_lines(&mut r)?;
    if lines.len() != n || chars(&lines) != total {
        return err("line count or text size in the header does not match the lines");
    }
    let m = r.u32()? as usize;
    let table = r.list(m, |r| Ok((r.u32()?, r.u32()?)))?;
    let mut groups = Vec::with_capacity(m);
    for (key, at) in table {
        if at as usize != r.pos() {
            return err(format!("group 0x{key:08x} is not where the table says"));
        }
        let (n, total) = (r.u32()? as usize, r.u32()? as usize);
        let v = read_lines(&mut r).map_err(|x| Error::Msg(format!("group 0x{key:08x}: {x}")))?;
        if v.len() != n || chars(&v) != total {
            return err(format!("group 0x{key:08x}: line count or text size does not match"));
        }
        groups.push((key, v));
    }
    match r.left() {
        0 => Ok((Doc { lines, groups }, e)),
        n => err(format!("{n} unexpected bytes at the end")),
    }
}

fn put_lines(w: &mut Writer, v: &[Line], head: bool) {
    if head {
        w.u32(v.len() as u32).u32(chars(v) as u32);
    }
    for l in v {
        w.bytes(if w.e == Endian::Le { b"TXTD" } else { b"DTXT" }).u32(l.id);
        w.u16(l.name.len() as u16 + 1).bytes(l.name.as_bytes()).u8(0);
        w.u16(l.text.len() as u16 + 1);
        l.text.iter().for_each(|&c| {
            w.u16(c);
        });
        w.u16(0);
    }
    w.bytes(if w.e == Endian::Le { b"DNEC" } else { b"CEND" });
}

fn write(d: &Doc, e: Endian) -> Vec<u8> {
    let mut w = Writer::new(e);
    w.u32(VERSION);
    put_lines(&mut w, &d.lines, true);
    let blocks: Vec<Vec<u8>> = d.groups.iter().map(|g| {
        let mut b = Writer::new(e);
        put_lines(&mut b, &g.1, true);
        b.finish()
    }).collect();
    w.u32(d.groups.len() as u32);
    let mut at = w.pos() + d.groups.len() * 8;
    for (g, b) in d.groups.iter().zip(&blocks) {
        w.u32(g.0).u32(at as u32);
        at += b.len();
    }
    blocks.iter().for_each(|b| {
        w.bytes(b);
    });
    w.finish()
}

fn id_val(h: u32) -> Val {
    match names::show(h) {
        Val::Str(s) => Val::Str(s),
        _ => Val::Raw(format!("0x{h:08x}")),
    }
}

fn dump(v: &[Line]) -> Val {
    Val::Tbl(
        v.iter()
            .map(|l| {
                let mut t = vec![(None, id_val(l.id)), (None, Val::Str(String::from_utf16_lossy(&l.text)))];
                if !l.name.is_empty() {
                    t.push((Some("Name".into()), Val::Str(l.name.clone())));
                }
                (None, Val::Tbl(t))
            })
            .collect(),
    )
}

fn num(v: &Val) -> Option<u32> {
    match v {
        Val::Str(s) => Some(names::key(s)),
        Val::Raw(r) => u32::from_str_radix(r.strip_prefix("0x")?, 16).ok(),
        v => v.int().and_then(|i| u32::try_from(i).ok()),
    }
}

fn undump(v: &Val, at: &str) -> Res<Vec<Line>> {
    let mut out = Vec::new();
    for (i, (_, x)) in v.items().iter().filter(|(_, x)| !matches!(x, Val::Note(_))).enumerate() {
        let bad = |m: &str| Error::Msg(format!("{at}[{}]: {m}", i + 1));
        let mut pos = x.items().iter().filter(|(k, _)| k.is_none()).map(|(_, v)| v);
        let id = pos.next().and_then(num).ok_or_else(|| bad("first value must be the text id (a name or 0x...)"))?;
        let text = pos.next().and_then(Val::str).ok_or_else(|| bad("second value must be the text in quotes"))?;
        let name = x.key("Name").and_then(Val::str).unwrap_or("").to_string();
        out.push(Line { id, name, text: text.encode_utf16().collect() });
    }
    Ok(out)
}

pub struct GameText;

impl Format for GameText {
    fn id(&self) -> &'static str {
        "gametext"
    }
    fn about(&self) -> &'static str {
        "game text: subtitles, dialog and UI lines of one language"
    }
    fn exts(&self) -> &'static [&'static str] {
        &["dlg"]
    }
    fn probe(&self, i: &mut Input) -> bool {
        i.len < 64 << 20 && i.all().is_ok_and(|d| parse(d).is_ok())
    }
    fn info(&self, i: &mut Input) -> Res<Vec<(&'static str, String)>> {
        let (d, e) = parse(i.all()?)?;
        Ok(vec![("contents", format!("{} lines, {} groups", d.lines.len(), d.groups.len())), ("byte order", endian_name(e).into())])
    }
    fn unpack(&self, i: &mut Input, o: &mut Out) -> Res<PathBuf> {
        let (d, e) = parse(i.all()?)?;
        let name = i.name.clone();
        let mut t = header(&name, self.about());
        t.extend(meta(self.id(), &name, e));
        t.push((None, Val::Note("Lines: { text id, \"text\", Name = voice line name }; the game finds a line by its id.\nGroups: { group id, Lines = { ... } } are loaded on demand (conversations, missions)".into())));
        t.push((Some("Lines".into()), dump(&d.lines)));
        let groups = d.groups.iter().map(|(k, v)| (None, Val::Tbl(vec![(None, id_val(*k)), (Some("Lines".into()), dump(v))]))).collect();
        t.push((Some("Groups".into()), Val::Tbl(groups)));
        o.text(&format!("{name}.lua"), &Val::Tbl(t))
    }
    fn pack(&self, src: &Path, meta: &Val, opts: Opts, out: &mut Sink) -> Res<()> {
        let e = meta_endian(meta, opts);
        let at = |m: Error| Error::Msg(format!("{}: {m}", src.display()));
        let lines = undump(meta.key("Lines").ok_or(Error::Bad("no Lines = { ... }"))?, "Lines").map_err(at)?;
        let mut groups = Vec::new();
        if let Some(g) = meta.key("Groups") {
            for (i, (_, x)) in g.items().iter().filter(|(_, x)| !matches!(x, Val::Note(_))).enumerate() {
                let key = x.items().iter().find(|(k, _)| k.is_none()).and_then(|(_, v)| num(v)).ok_or_else(|| at(Error::Msg(format!("Groups[{}]: first value must be the group id", i + 1))))?;
                let v = undump(x.key("Lines").unwrap_or(&Val::Tbl(Vec::new())), &format!("Groups[{}].Lines", i + 1)).map_err(at)?;
                groups.push((key, v));
            }
        }
        out.put(&write(&Doc { lines, groups }, e))
    }
}
