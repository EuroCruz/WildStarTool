use super::arch::{entry, entry_num, entry_path, uniq};
use super::{endian_name, header, meta, meta_endian, Format, Input, Item, Opts, Out, Sink, INDEX};
use crate::io::err;
use crate::names;
use ac_core::{Endian, Error, Reader, Res, Writer};
use ac_lua::luac::{self, Header};
use ac_lua::Val;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

const REC: usize = 21;
const VER: u8 = 4;
const MODULES: &str = "Modules/";
const MARK: &str = "--// ";
const SITE: &str = "https://github.com/EuroCruz/WildStarTool";
const MINE: &str = "Includes/WildStarTool/";
const INIT: &str = "Includes/WildStarToolInit.lua";
const ENTRY: &str = "Modules/France.lua";
const ENTRY_FN: &str = "function France.OnEnter(";

struct Rec {
    key: u32,
    name: u32,
    at: u32,
    size: u32,
    unc: u32,
    preload: bool,
}

fn endian(h: &[u8]) -> Endian {
    if h.get(..4).is_some_and(|b| u32::from_le_bytes(b.try_into().unwrap_or_default()) < 0x10000) {
        Endian::Le
    } else {
        Endian::Be
    }
}

fn head(i: &mut Input) -> Res<(Vec<Rec>, Endian)> {
    let e = endian(&i.head(4));
    let n = e.get::<u32>(&i.head(4)).unwrap_or(0) as usize;
    if n == 0 || 4 + (n * REC) as u64 > i.len {
        return err("not a Lua script pack (bad script count)");
    }
    let t = i.at(4, n * REC)?;
    let mut r = Reader::new(&t, e);
    let v = r.list(n, |r| Ok(Rec { key: r.u32()?, name: r.u32()?, at: r.u32()?, size: r.u32()?, unc: r.u32()?, preload: r.u8()? != 0 }))?;
    if v.iter().any(|x| x.at as u64 + x.size as u64 > i.len || (x.at as usize) < 4 + n * REC) {
        return err("a script runs past the end of the file");
    }
    Ok((v, e))
}

fn lua_header(e: Endian) -> Header {
    Header { endian: e, int_sz: 4, size_sz: 4, num_sz: if e == Endian::Le { 4 } else { 8 }, integral: false }
}

fn script_path(rel: &str) -> String {
    format!(r"d:\scripts\{}.luac", stem_path(rel).replace('/', "\\"))
}

fn key_of(rel: &str) -> u32 {
    names::hash(&script_path(rel))
}

fn by_key(key: u32) -> Option<String> {
    let n = names::name(key).filter(|n| names::hash(n) == key);
    let from_name = n.as_deref().and_then(|n| {
        let low = n.to_ascii_lowercase();
        low.starts_with(r"d:\scripts\").then(|| format!("{}.lua", stem_path(&n[11..]).replace('\\', "/")))
    });
    from_name.or_else(|| names::script(key).map(|s| s.replace('\\', "/")))
}

fn stem_path(rel: &str) -> &str {
    rel.strip_suffix(".luac").or_else(|| rel.strip_suffix(".lua")).unwrap_or(rel)
}

fn name_of(rel: &str) -> u32 {
    names::hash(stem_path(rel).rsplit('/').next().unwrap_or(rel))
}

fn split_source(src: &str) -> Option<(String, String)> {
    let k = src.to_ascii_lowercase().find(r"scripts\")? + 8;
    Some((src[..k].to_string(), src[k..].replace('\\', "/")))
}

fn mark(rel: &str) -> String {
    let file = rel.rsplit('/').next().unwrap_or(rel);
    format!("{MARK}{file} — The Saboteur, decompiled by WildStarTool ({SITE})\r\n\r\n")
}

fn unmark(t: &str) -> &str {
    let mut rest = t;
    let mut ours = false;
    while rest.starts_with(MARK.trim_end()) {
        let (line, next) = rest.split_once('\n').unwrap_or((rest, ""));
        ours |= line.contains("WildStarTool");
        rest = next;
    }
    if !ours {
        return t;
    }
    rest.strip_prefix("\r\n").or_else(|| rest.strip_prefix('\n')).unwrap_or(rest)
}

fn strip(p: &mut luac::Proto) {
    p.lines.clear();
    p.locals.clear();
    p.ups.clear();
    p.protos.iter_mut().for_each(strip);
}

fn compile(text: &str, name: &str, h: Header, stripped: bool) -> Res<Vec<u8>> {
    let b = ac_lua::comp::compile(text.as_bytes(), name, h, VER)?;
    if !stripped {
        return Ok(b);
    }
    let mut c = luac::parse(&b)?;
    strip(&mut c.main);
    luac::write(&c)
}

fn decompile(b: &[u8], name: &str) -> Option<(String, bool)> {
    let c = luac::parse(b).ok()?;
    let stripped = c.main.lines.is_empty() && c.main.locals.is_empty();
    let s = ac_lua::dec::source(&c).ok()?;
    let again = compile(&s, name, c.h, stripped).ok()?;
    same(&again, b).then_some((s, stripped))
}

fn plain(b: &[u8]) -> Option<(Option<String>, String)> {
    if b.starts_with(b"\x1bLua") {
        return None;
    }
    let t = String::from_utf8(b.to_vec()).ok()?;
    match t.strip_prefix("--@").and_then(|r| r.split_once('\n')) {
        Some((src, body)) => Some((Some(format!("@{}", src.trim_end())), body.to_string())),
        None => Some((None, t)),
    }
}

fn no_lines(p: &mut luac::Proto) {
    p.lines.clear();
    (p.line, p.last) = (0, 0);
    p.locals.iter_mut().for_each(|l| (l.start, l.end) = (0, 0));
    p.protos.iter_mut().for_each(no_lines);
}

pub fn same(a: &[u8], b: &[u8]) -> bool {
    let (Ok(mut a), Ok(mut b)) = (luac::parse(a), luac::parse(b)) else { return a == b };
    no_lines(&mut a.main);
    no_lines(&mut b.main);
    a == b
}

fn source_name(b: &[u8]) -> Option<String> {
    luac::parse(b).ok()?.main.source.map(|s| String::from_utf8_lossy(&s).into_owned())
}

pub struct Luap;

impl Format for Luap {
    fn id(&self) -> &'static str {
        "luap"
    }
    fn about(&self) -> &'static str {
        "pack of compiled Lua scripts"
    }
    fn exts(&self) -> &'static [&'static str] {
        &["luap"]
    }
    fn folder(&self) -> bool {
        true
    }
    fn probe(&self, i: &mut Input) -> bool {
        head(i).is_ok()
    }
    fn info(&self, i: &mut Input) -> Res<Vec<(&'static str, String)>> {
        let (v, e) = head(i)?;
        Ok(vec![("contents", format!("{} scripts", v.len())), ("byte order", endian_name(e).into())])
    }
    fn list(&self, i: &mut Input) -> Res<Vec<Item>> {
        Ok(head(i)?.0.iter().map(|x| Item { name: names::script(x.key).unwrap_or_else(|| format!("0x{:08x}", x.key)), size: x.unc as u64, note: String::new() }).collect())
    }
    fn unpack(&self, i: &mut Input, o: &mut Out) -> Res<PathBuf> {
        let (v, e) = head(i)?;
        let mut items = Vec::with_capacity(v.len());
        let mut prefixes: Vec<(String, usize)> = Vec::new();
        for x in &v {
            let mut b = i.at(x.at as u64, x.size as usize)?;
            let packed = x.size != x.unc;
            if packed {
                b = ac_pack::lzx::decode(&b, x.unc as usize).map_err(|m| Error::Msg(format!("script 0x{:08x}: {m}", x.key)))?;
            }
            let text = plain(&b);
            let src = match &text {
                Some((s, _)) => s.clone(),
                None => source_name(&b),
            };
            let (prefix, rel) = match src.as_deref().and_then(split_source) {
                Some((p, r)) => (Some(p), r),
                None => (None, names::script(x.key).unwrap_or_else(|| format!("0x{:08x}.lua", x.key)).replace('\\', "/")),
            };
            if let Some(p) = &prefix {
                match prefixes.iter_mut().find(|q| &q.0 == p) {
                    Some(q) => q.1 += 1,
                    None => prefixes.push((p.clone(), 1)),
                }
            }
            items.push((x, b, prefix, rel, text.map(|t| t.1)));
        }
        let main = prefixes.iter().max_by_key(|p| p.1).map(|p| p.0.clone()).unwrap_or_default();
        let mut list = Vec::with_capacity(items.len());
        let mut used = HashSet::new();
        for (x, b, prefix, rel, source) in items {
            let compiled = source.is_none();
            let (text, stripped) = match (source, &prefix) {
                (Some(t), _) => (Some(t), false),
                (None, Some(p)) if !o.opts.raw => decompile(&b, &format!("{p}{}", rel.replace('/', "\\"))).map_or((None, false), |(t, s)| (Some(t), s)),
                _ => (None, false),
            };
            let name = if key_of(&rel) == x.key { rel.clone() } else { by_key(x.key).unwrap_or_else(|| rel.clone()) };
            let rel = match text {
                Some(t) => {
                    let rel = uniq(&mut used, &name);
                    o.write(&rel, format!("{}{t}", mark(&rel)).as_bytes())?;
                    rel
                }
                None => {
                    let rel = uniq(&mut used, &format!("{}.luac", stem_path(&name)));
                    o.write(&rel, &b)?;
                    rel
                }
            };
            let mut extra = Vec::new();
            if x.key != key_of(&rel) {
                extra.push((Some("Key".into()), Val::Raw(format!("0x{:08x}", x.key))));
            }
            if x.name != name_of(&rel) {
                extra.push((Some("Name".into()), Val::Raw(format!("0x{:08x}", x.name))));
            }
            if x.preload != rel.starts_with(MODULES) {
                extra.push((Some("Preload".into()), Val::Bool(x.preload)));
            }
            if stripped {
                extra.push((Some("Strip".into()), Val::Bool(true)));
            }
            if !compiled {
                extra.push((Some("Compile".into()), Val::Bool(false)));
            }
            if let Some(p) = prefix.filter(|p| *p != main) {
                extra.push((Some("Source".into()), Val::Str(p)));
            }
            list.push((None, entry(&rel, extra)));
        }
        let name = i.name.clone();
        let mut t = header(&name, self.about());
        t.extend(meta(self.id(), &name, e));
        t.push((None, Val::Note(format!("Scripts in pack order, as .lua files next to this index (.luac = kept compiled).\nEdit them freely; new .lua files are added at the end. Scripts in {MODULES} are loaded at start.\nYour own scripts in {MINE} are run automatically when the game starts (France.OnEnter).\nSource: the path the game shows in Lua errors. Options (per script, or once in this index for all):
Strip = true (compile without debug info), Compile = false (store the source text, the game compiles it),
Compressed = false (store without LZX compression; by default every script is compressed)"))));
        t.push((Some("Source".into()), Val::Str(main)));
        t.push((Some("Files".into()), Val::Tbl(list)));
        o.text(INDEX, &Val::Tbl(t))
    }
    fn pack(&self, src: &Path, meta: &Val, opts: Opts, out: &mut Sink) -> Res<()> {
        let e = meta_endian(meta, opts);
        let main = meta.key("Source").and_then(Val::str).unwrap_or(r"@d:\projects\wildstar\main\BinCommon\Scripts\").to_string();
        let all = |k: &str, d: bool| match meta.key(k) {
            Some(Val::Bool(b)) => *b,
            _ => d,
        };
        let (packed, stripped, compiled) = (all("Compressed", true), all("Strip", false), all("Compile", true));
        let mut list = Vec::new();
        for (_, v) in meta.key("Files").ok_or(Error::Bad("index.lua has no Files = { ... }"))?.items().iter().filter(|(_, v)| !matches!(v, Val::Note(_))) {
            let rel = entry_path(v)?;
            let flag = |k: &str| match v.key(k) {
                Some(Val::Bool(b)) => Some(*b),
                _ => None,
            };
            let item = Entry {
                key: entry_num(v, "Key")?,
                name: entry_num(v, "Name")?,
                preload: flag("Preload"),
                packed: flag("Compressed").unwrap_or(packed),
                strip: flag("Strip").unwrap_or(stripped),
                compile: flag("Compile").unwrap_or(compiled),
                source: v.key("Source").and_then(Val::str).map(str::to_string),
                rel,
            };
            list.push(item);
        }
        let mut have: Vec<String> = list.iter().map(|x| x.rel.to_lowercase()).collect();
        let mut fresh = Vec::new();
        for p in ac_core::fs::walk(src)? {
            let rel = p.strip_prefix(src).unwrap_or(&p).to_string_lossy().replace('\\', "/");
            let low = rel.to_lowercase();
            if (low.ends_with(".lua") || low.ends_with(".luac")) && low != INDEX && !have.contains(&low) {
                fresh.push(rel);
            }
        }
        fresh.sort();
        let mine: Vec<String> = list.iter().map(|x| x.rel.clone()).chain(fresh.iter().cloned()).filter(|r| r.starts_with(MINE) && r.ends_with(".lua")).collect();
        if !mine.is_empty() && !have.contains(&INIT.to_lowercase()) && !fresh.iter().any(|r| r == INIT) {
            fresh.push(INIT.into());
        }
        for rel in fresh {
            have.push(rel.to_lowercase());
            list.push(Entry { key: None, name: None, preload: None, packed, strip: stripped, compile: compiled, source: None, rel });
        }
        let mut blobs = Vec::with_capacity(list.len());
        for x in &list {
            let at = |m: String| Error::Msg(format!("{}: {m}", x.rel));
            let path = ac_core::fs::join_safe(src, &x.rel)?;
            let b = if x.rel.ends_with(".luac") {
                std::fs::read(&path).map_err(|m| at(m.to_string()))?
            } else {
                let text = if x.rel == INIT && !mine.is_empty() {
                    init_script(&mine)
                } else {
                    let t = std::fs::read(&path).map_err(|m| at(m.to_string()))?;
                    let t = String::from_utf8(t).map_err(|_| at("not UTF-8 text".into()))?;
                    let t = unmark(&t).to_string();
                    if x.rel == ENTRY && !mine.is_empty() { hook(&t).map_err(|m| at(m))? } else { t }
                };
                let name = format!("{}{}", x.source.as_deref().unwrap_or(&main), x.rel.replace('/', "\\"));
                if x.compile {
                    compile(&text, &name, lua_header(e), x.strip).map_err(|m| at(m.to_string()))?
                } else {
                    format!("--{name}\r\n{text}").into_bytes()
                }
            };
            let unc = b.len() as u32;
            let b = if x.packed { ac_pack::lzx::encode(&b, 9) } else { b };
            blobs.push((unc, b));
        }
        let mut w = Writer::new(e);
        w.u32(list.len() as u32);
        let mut at = 4 + (list.len() * REC) as u32;
        for (x, (unc, b)) in list.iter().zip(&blobs) {
            let key = x.key.unwrap_or_else(|| names::key(&script_path(&x.rel)));
            let name = x.name.unwrap_or_else(|| name_of(&x.rel));
            let preload = x.preload.unwrap_or_else(|| x.rel.starts_with(MODULES));
            w.u32(key).u32(name).u32(at).u32(b.len() as u32).u32(*unc).u8(preload as u8);
            at += b.len() as u32;
        }
        out.put(&w.finish())?;
        for (_, b) in &blobs {
            out.put(b)?;
        }
        Ok(())
    }
    fn same(&self, a: &[u8], b: &[u8]) -> bool {
        let scripts = |d: &[u8]| -> Option<Vec<(u32, u32, bool, Vec<u8>)>> {
            let mut i = Input::from("x.luap", d.to_vec());
            let (v, _) = head(&mut i).ok()?;
            v.iter()
                .map(|x| {
                    let b = d.get(x.at as usize..(x.at + x.size) as usize)?;
                    let b = if x.size != x.unc { ac_pack::lzx::decode(b, x.unc as usize).ok()? } else { b.to_vec() };
                    Some((x.key, x.name, x.preload, b))
                })
                .collect()
        };
        match (scripts(a), scripts(b)) {
            (Some(x), Some(y)) => x.len() == y.len() && x.iter().zip(&y).all(|(p, q)| p.0 == q.0 && p.1 == q.1 && p.2 == q.2 && same(&p.3, &q.3)),
            _ => a == b,
        }
    }
}

struct Entry {
    key: Option<u32>,
    name: Option<u32>,
    preload: Option<bool>,
    packed: bool,
    strip: bool,
    compile: bool,
    source: Option<String>,
    rel: String,
}

fn require_name(rel: &str) -> String {
    stem_path(rel).replace('/', "\\\\")
}

fn init_script(mine: &[String]) -> String {
    let mut s = String::from("Util.RegisterLuaUpdate(\"WildStarToolInit\")\r\n\r\nfunction WildStarToolInit()\r\n\tUtil.UnregisterLuaUpdate(\"WildStarToolInit\")\r\n");
    for r in mine {
        s += &format!("\trequire(\"{}\")\r\n", require_name(r));
    }
    s + "end\r\n"
}

fn hook(t: &str) -> Result<String, String> {
    let call = format!("require(\"{}\")", require_name(INIT));
    if t.contains(&call) {
        return Ok(t.to_string());
    }
    let at = t.find(ENTRY_FN).ok_or(format!("cannot find {ENTRY_FN} to start your scripts from"))?;
    let end = at + t[at..].find('\n').map_or(t.len() - at, |n| n + 1);
    Ok(format!("{}\t{call}\r\n{}", &t[..end], &t[end..]))
}
