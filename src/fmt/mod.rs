mod albs;
mod anim;
mod arch;
mod bnk;
mod cinema;
mod cinpack;
mod cnvpack;
mod data;
mod font;
mod luap;
mod map;
mod materials;
mod particle;
mod pck;
mod route;
mod save;
mod sound;
mod terrain;
mod tex;
mod text;
mod trigs;
mod world;
mod wsd;

pub use data::{Data, D};

use ac_core::{Endian, Error, Res, Writer};
use ac_lua::Val;
use std::fs::File;
use std::io::{Read as _, Seek, SeekFrom, Write as _};
use std::path::{Path, PathBuf};

pub struct Input {
    pub path: PathBuf,
    pub name: String,
    pub len: u64,
    f: Option<File>,
    all: Option<Vec<u8>>,
}

impl Input {
    pub fn open(p: &Path) -> Res<Input> {
        let f = File::open(p).map_err(|e| Error::Msg(format!("cannot open {}: {e}", p.display())))?;
        let len = f.metadata()?.len();
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        Ok(Input { path: p.to_path_buf(), name, len, f: Some(f), all: None })
    }

    pub fn from(name: &str, d: Vec<u8>) -> Input {
        Input { path: PathBuf::from(name), name: name.to_string(), len: d.len() as u64, f: None, all: Some(d) }
    }

    pub fn ext(&self) -> String {
        self.name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default()
    }

    pub fn at(&mut self, o: u64, n: usize) -> Res<Vec<u8>> {
        if let Some(a) = &self.all {
            return a.get(o as usize..o as usize + n).map(<[u8]>::to_vec).ok_or(Error::Bad("read past the end of the file"));
        }
        if o + n as u64 > self.len {
            return Err(Error::Bad("read past the end of the file"));
        }
        let mut b = vec![0; n];
        let f = self.f.as_mut().ok_or(Error::Bad("no file"))?;
        f.seek(SeekFrom::Start(o))?;
        f.read_exact(&mut b)?;
        Ok(b)
    }

    pub fn head(&mut self, n: usize) -> Vec<u8> {
        self.at(0, n.min(self.len as usize)).unwrap_or_default()
    }

    pub fn all(&mut self) -> Res<&[u8]> {
        if self.all.is_none() {
            let mut b = Vec::with_capacity(self.len as usize);
            let f = self.f.as_mut().ok_or(Error::Bad("no file"))?;
            f.seek(SeekFrom::Start(0))?;
            f.read_to_end(&mut b)?;
            self.all = Some(b);
        }
        Ok(self.all.as_deref().unwrap())
    }
}

enum To {
    File(std::io::BufWriter<File>),
    Mem(Vec<u8>),
}

pub struct Sink {
    to: To,
    pub len: u64,
}

impl Sink {
    pub fn create(p: &Path) -> Res<Sink> {
        ac_core::fs::ensure_parent(p)?;
        let f = File::create(p).map_err(|e| Error::Msg(format!("cannot write {}: {e}", p.display())))?;
        Ok(Sink { to: To::File(std::io::BufWriter::with_capacity(1 << 20, f)), len: 0 })
    }

    pub fn mem() -> Sink {
        Sink { to: To::Mem(Vec::new()), len: 0 }
    }

    pub fn put(&mut self, b: &[u8]) -> Res<()> {
        match &mut self.to {
            To::File(f) => f.write_all(b)?,
            To::Mem(v) => v.extend_from_slice(b),
        }
        self.len += b.len() as u64;
        Ok(())
    }

    pub fn patch(&mut self, at: u64, b: &[u8]) -> Res<()> {
        match &mut self.to {
            To::File(f) => {
                f.flush()?;
                let g = f.get_mut();
                g.seek(SeekFrom::Start(at))?;
                g.write_all(b)?;
                g.seek(SeekFrom::End(0))?;
            }
            To::Mem(v) => v[at as usize..at as usize + b.len()].copy_from_slice(b),
        }
        Ok(())
    }

    pub fn pad(&mut self, to: u64, fill: u8) -> Res<()> {
        let n = to.saturating_sub(self.len) as usize;
        self.put(&vec![fill; n])
    }

    pub fn done(self) -> Res<Vec<u8>> {
        match self.to {
            To::File(mut f) => {
                f.flush()?;
                Ok(Vec::new())
            }
            To::Mem(v) => Ok(v),
        }
    }
}

#[derive(Clone, Copy, Default)]
pub struct Opts {
    pub raw: bool,
    pub be: bool,
}

pub struct Out {
    pub dir: PathBuf,
    pub opts: Opts,
    pub files: usize,
    pub warn: Vec<String>,
    pub depth: usize,
}

impl Out {
    pub fn new(dir: &Path, opts: Opts) -> Out {
        Out { dir: dir.to_path_buf(), opts, files: 0, warn: Vec::new(), depth: 0 }
    }

    pub fn child(&mut self, rel: &str, d: Vec<u8>) -> Res<()> {
        if self.depth > 4 {
            return self.write(rel, &d).map(drop);
        }
        let name = rel.rsplit('/').next().unwrap_or(rel).to_string();
        let mut i = Input::from(&name, d);
        let ext = i.ext();
        let Some(f) = all().iter().copied().find(|f| f.exts().contains(&ext.as_str()) && f.probe(&mut i)) else {
            let d = i.all.take().unwrap_or_default();
            return self.write(rel, &d).map(drop);
        };
        let at = ac_core::fs::join_safe(&self.dir, rel)?;
        let dir = if f.folder() { at.clone() } else { at.parent().map(Path::to_path_buf).unwrap_or_default() };
        let mut o = Out { dir, opts: self.opts, files: 0, warn: Vec::new(), depth: self.depth + 1 };
        match f.unpack(&mut i, &mut o) {
            Ok(_) => {
                self.files += o.files;
                self.warn.extend(o.warn.into_iter().map(|w| format!("{rel}: {w}")));
                Ok(())
            }
            Err(e) => {
                let _ = if f.folder() { std::fs::remove_dir_all(&at) } else { Ok(()) };
                self.warn.push(format!("{rel}: kept as is ({e})"));
                let d = i.all.take().unwrap_or_default();
                self.write(rel, &d).map(drop)
            }
        }
    }

    pub fn write(&mut self, rel: &str, d: &[u8]) -> Res<PathBuf> {
        let p = ac_core::fs::join_safe(&self.dir, rel)?;
        ac_core::fs::ensure_parent(&p)?;
        std::fs::write(&p, d).map_err(|e| Error::Msg(format!("cannot write {}: {e}", p.display())))?;
        self.files += 1;
        Ok(p)
    }

    pub fn text(&mut self, rel: &str, v: &Val) -> Res<PathBuf> {
        let s = ac_lua::data::chunk(v, &ac_lua::data::Style::default());
        self.write(rel, s.as_bytes())
    }
}

pub struct Item {
    pub name: String,
    pub size: u64,
    pub note: String,
}

pub trait Format: Sync {
    fn id(&self) -> &'static str;
    fn about(&self) -> &'static str;
    fn exts(&self) -> &'static [&'static str];
    fn folder(&self) -> bool {
        false
    }
    fn probe(&self, i: &mut Input) -> bool;
    fn info(&self, i: &mut Input) -> Res<Vec<(&'static str, String)>>;
    fn list(&self, _: &mut Input) -> Res<Vec<Item>> {
        Ok(Vec::new())
    }
    fn unpack(&self, i: &mut Input, o: &mut Out) -> Res<PathBuf>;
    fn pack(&self, src: &Path, meta: &Val, opts: Opts, out: &mut Sink) -> Res<()>;
    fn same(&self, a: &[u8], b: &[u8]) -> bool {
        a == b
    }
}

static ALL: &[&dyn Format] = &[
    &arch::Mega,
    &albs::Albs,
    &anim::Animations,
    &pck::Pck,
    &bnk::Bnk,
    &arch::LooseFiles,
    &wsd::EditNodes,
    &wsd::Wsd,
    &wsd::GameTemplates,
    &luap::Luap,
    &D::<world::Ambush>::new(),
    &D::<world::WaterCtrl>::new(),
    &D::<world::WaterFlow>::new(),
    &D::<world::FreePlay>::new(),
    &D::<world::HqPoints>::new(),
    &D::<world::Spore>::new(),
    &D::<world::Defen>::new(),
    &D::<world::RndNodes>::new(),
    &D::<world::Tuv>::new(),
    &D::<route::Paths>::new(),
    &D::<route::Railway>::new(),
    &D::<route::CinSplines>::new(),
    &D::<route::GpsGraph>::new(),
    &D::<terrain::Hei>::new(),
    &D::<terrain::Fst>::new(),
    &D::<terrain::Fsm>::new(),
    &D::<sound::WwiseIds>::new(),
    &D::<map::LevelMap>::new(),
    &D::<map::DlcMap>::new(),
    &D::<map::GlobalMap>::new(),
    &D::<cinema::Cxa>::new(),
    &cinpack::CinPack,
    &cnvpack::CnvPack,
    &text::GameText,
    &D::<text::RandomText>::new(),
    &D::<trigs::Trigs>::new(),
    &D::<particle::Particles>::new(),
    &D::<materials::Materials>::new(),
    &D::<font::Font>::new(),
    &D::<save::Save>::new(),
];

pub fn all() -> &'static [&'static dyn Format] {
    ALL
}

const PREORDER_KEY: u32 = 0x1557_f8d2;

const CONFIG: [(&str, u32); 13] = [
    ("DisplayProfile", 0),
    ("ScreenWidth", 1280),
    ("ScreenHeight", 720),
    ("TextureQuality", 3),
    ("SliceQuality", 0),
    ("ClipRange", 3),
    ("ObjectQuality", 2),
    ("RainDensity", 100),
    ("Shadows", 1),
    ("Windowed", 0),
    ("RefreshRate", 60),
    ("PostProcessing", 1),
    ("VSync", 1),
];

pub fn make(kind: &str, value: Option<&str>, opts: Opts) -> Res<Vec<u8>> {
    let e = if opts.be { Endian::Be } else { Endian::Le };
    match kind {
        "sku" => {
            let n = value.unwrap_or("0").parse::<u32>().ok().filter(|n| *n <= 5).ok_or(Error::Bad("sku value must be 0..5"))?;
            let mut w = Writer::new(e);
            w.u32(n);
            Ok(w.finish())
        }
        "preorder" => {
            let mut w = Writer::new(Endian::Le);
            w.u32(0).u32(0).u32(PREORDER_KEY);
            Ok(w.finish())
        }
        "videoconfig" => {
            let size = match value {
                Some(v) => v.split_once('x').and_then(|(w, h)| Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?))).ok_or(Error::Bad("config value must be a screen size like 1920x1080"))?,
                None => (CONFIG[1].1, CONFIG[2].1),
            };
            let mut t = String::from("; The Saboteur video settings: every key the game reads, with its own defaults\r\n");
            for &(k, v) in &CONFIG {
                let v = match k {
                    "ScreenWidth" => size.0,
                    "ScreenHeight" => size.1,
                    _ => v,
                };
                t += &format!("{k} {v}\r\n");
            }
            Ok(t.into_bytes())
        }
        _ => Err(Error::Msg(format!("unknown kind {kind} (sku, preorder, videoconfig)"))),
    }
}

pub fn by_id(id: &str) -> Option<&'static dyn Format> {
    all().iter().copied().find(|f| f.id().eq_ignore_ascii_case(id))
}

pub fn detect(i: &mut Input) -> Option<&'static dyn Format> {
    let ext = i.ext();
    let fs = all();
    fs.iter().copied().find(|f| f.exts().contains(&ext.as_str()) && f.probe(i)).or_else(|| fs.iter().copied().find(|f| !f.exts().contains(&ext.as_str()) && f.probe(i)))
}

pub fn endian_name(e: Endian) -> &'static str {
    match e {
        Endian::Le => "little-endian (PC)",
        Endian::Be => "big-endian (PS3 / Xbox 360)",
    }
}

pub const INDEX: &str = "index.lua";

pub fn read_text(p: &Path) -> Res<Val> {
    let t = std::fs::read_to_string(p).map_err(|e| Error::Msg(format!("cannot read {}: {e}", p.display())))?;
    ac_lua::data::parse_chunk(&t).map_err(|e| Error::Msg(format!("{}: {e}", p.display())))
}

pub fn source(p: &Path) -> Option<(PathBuf, PathBuf)> {
    let idx = p.join(INDEX);
    if p.is_dir() && idx.is_file() {
        return Some((p.to_path_buf(), idx));
    }
    let lua = PathBuf::from(format!("{}.lua", p.display()));
    if lua.is_file() {
        return Some((lua.clone(), lua));
    }
    (p.is_file() && p.extension().is_some_and(|e| e == "lua")).then(|| (p.to_path_buf(), p.to_path_buf()))
}

pub fn pack_into(src: &Path, idx: &Path, opts: Opts, out: &mut Sink) -> Res<&'static dyn Format> {
    let meta = read_text(idx)?;
    let id = meta.key("Format").and_then(Val::str).ok_or_else(|| Error::Msg(format!("{} has no Format = \"...\" line — was it made by wildstartool unpack?", idx.display())))?;
    let f = by_id(id).ok_or_else(|| Error::Msg(format!("{}: unknown Format \"{id}\"", idx.display())))?;
    f.pack(src, &meta, opts, out)?;
    Ok(f)
}

pub fn build(p: &Path, opts: Opts) -> Res<Vec<u8>> {
    if let Some((src, idx)) = source(p).filter(|_| !p.is_file() || p.extension().is_some_and(|e| e == "lua")) {
        let mut s = Sink::mem();
        pack_into(&src, &idx, opts, &mut s)?;
        return s.done();
    }
    std::fs::read(p).map_err(|e| Error::Msg(format!("cannot read {}: {e}", p.display())))
}

pub fn header(file: &str, about: &str) -> Vec<(Option<String>, Val)> {
    vec![(None, Val::Note(format!("{file} — {about}\nEdit freely, then build it back:  wildstartool pack <this file or folder>")))]
}

pub fn meta(fmt: &str, file: &str, e: Endian) -> Vec<(Option<String>, Val)> {
    let mut m = vec![(Some("Format".into()), Val::Str(fmt.into())), (Some("File".into()), Val::Str(file.into()))];
    if e == Endian::Be {
        m.push((Some("Endian".into()), Val::Str("be".into())));
    }
    m
}

pub fn meta_endian(meta: &Val, opts: Opts) -> Endian {
    if opts.be || meta.key("Endian").and_then(Val::str) == Some("be") {
        Endian::Be
    } else {
        Endian::Le
    }
}

#[cfg(test)]
mod t {
    use super::*;

    fn game() -> Vec<PathBuf> {
        match std::env::var("WST_GAME") {
            Ok(g) => vec![PathBuf::from(g)],
            Err(_) => ["SUBPC", "SUBXE", "SUBPS3"].iter().map(|s| PathBuf::from(r"C:\Users\Andrew\Desktop\Saboteur\GAMEFILES").join(s)).collect(),
        }
    }

    pub fn roundtrip(p: &Path, tmp: &Path) -> Res<Option<&'static str>> {
        let s = p.to_string_lossy().to_uppercase();
        let console = s.contains("SUBXE") || s.contains("SUBPS3");
        let opts = Opts { raw: std::env::var("WST_RAW").map_or(console, |v| v == "1"), be: false };
        let mut i = Input::open(p)?;
        let Some(f) = detect(&mut i) else { return Ok(None) };
        let dir = tmp.join(i.name.replace('.', "_"));
        let _ = std::fs::remove_dir_all(&dir);
        let mut o = Out::new(&dir, opts);
        let made = f.unpack(&mut i, &mut o)?;
        if let Some(w) = o.warn.first() {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(Error::Msg(format!("{} warnings, first: {w}", o.warn.len())));
        }
        let src = if f.folder() { dir.clone() } else { made };
        let idx = if f.folder() { src.join(INDEX) } else { src.clone() };
        let mut s = Sink::mem();
        pack_into(&src, &idx, opts, &mut s)?;
        let same = f.same(&s.done()?, i.all()?);
        let _ = std::fs::remove_dir_all(&dir);
        if !same {
            return Err(Error::Msg("packed file differs from the original".into()));
        }
        Ok(Some(f.id()))
    }

    #[test]
    #[ignore]
    fn game_roundtrip() {
        let max: u64 = std::env::var("WST_MAX").ok().and_then(|s| s.parse().ok()).unwrap_or(64 << 20);
        let only = std::env::var("WST_ONLY").unwrap_or_default();
        let tmp = std::env::temp_dir().join("wst_roundtrip");
        let (mut ok, mut bad) = (std::collections::BTreeMap::<&str, usize>::new(), Vec::new());
        for p in game().iter().flat_map(|g| ac_core::fs::walk(g).unwrap()) {
            let len = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
            if len > max || !p.to_string_lossy().to_lowercase().contains(&only.to_lowercase()) {
                continue;
            }
            match roundtrip(&p, &tmp) {
                Ok(Some(id)) => *ok.entry(id).or_default() += 1,
                Ok(None) => {}
                Err(e) => bad.push(format!("{}: {e}", p.display())),
            }
        }
        eprintln!("ok: {ok:?}");
        for b in &bad {
            eprintln!("FAIL {b}");
        }
        assert!(bad.is_empty());
    }
}

#[cfg(test)]
mod zgame {
    #[test]
    #[ignore]
    fn game_zlib_streams() {
        let max: usize = std::env::var("WST_ZN").ok().and_then(|s| s.parse().ok()).unwrap_or(3000);
        let game = std::path::PathBuf::from(std::env::var("WST_GAME").unwrap_or(r"C:\Users\Andrew\Desktop\Saboteur\GAMEFILES\SUBPC".into()));
        let (mut ok, mut bad, mut seen) = (0, Vec::new(), 0);
        for p in ac_core::fs::walk(&game).unwrap() {
            let e = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            if !matches!(e.as_str(), "pack" | "dynpack") || std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0) > 64 << 20 {
                continue;
            }
            let d = std::fs::read(&p).unwrap();
            let mut i = 0;
            while i + 2 < d.len() && seen < max {
                if d[i] == 0x78 && d[i + 1] == 0x01 {
                    if let Ok(u) = ac_pack::flate::unzlib(&d[i..]) {
                        seen += 1;
                        let z = ac_pack::flate::zlib_zc(&u, 1);
                        if d[i..].starts_with(&z) {
                            ok += 1;
                            i += z.len();
                            continue;
                        }
                        bad.push(format!("{}@{i}: {} bytes", p.display(), u.len()));
                    }
                }
                i += 1;
            }
        }
        eprintln!("zlib streams: {ok} identical of {seen}");
        for b in bad.iter().take(10) {
            eprintln!("  differs {b}");
        }
        assert!(bad.is_empty());
    }
}

#[cfg(test)]
mod modtest;
