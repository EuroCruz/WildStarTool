use crate::names;
use ac_core::{Endian, Error, Reader, Res, Writer};
use ac_lua::Val;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Read,
    Write,
    Dump,
    Load,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Show {
    Dec,
    Hex,
    Hash,
    Bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Len {
    U8,
    U16,
    U32,
    N(usize),
    Rest,
}

pub fn err<T>(m: impl Into<String>) -> Res<T> {
    Err(Error::Msg(m.into()))
}

pub trait Prim: Copy + Default {
    fn get(r: &mut Reader) -> Res<Self>;
    fn put(self, w: &mut Writer);
    fn dump(self, s: Show) -> Val;
    fn load(v: &Val) -> Option<Self>;
    fn same(self, o: Self) -> bool;
}

macro_rules! int {
    ($($t:ty),*) => {$(
        impl Prim for $t {
            fn get(r: &mut Reader) -> Res<Self> {
                r.get()
            }
            fn put(self, w: &mut Writer) {
                w.put(self);
            }
            fn dump(self, s: Show) -> Val {
                let n = std::mem::size_of::<$t>() * 2;
                match s {
                    Show::Bool if self as i64 == 0 || self as i64 == 1 => Val::Bool(self as i64 == 1),
                    Show::Hash if n == 8 => names::show(self as u32),
                    Show::Hex | Show::Hash => Val::Raw(format!("0x{:0n$x}", self)),
                    _ => Val::Int(self as i64),
                }
            }
            fn same(self, o: Self) -> bool {
                self == o
            }
            fn load(v: &Val) -> Option<Self> {
                match v {
                    Val::Bool(b) => Some(*b as $t),
                    Val::Str(s) => Some(names::key(s) as $t),
                    Val::Int(i) => <$t>::try_from(*i).ok().or_else(|| (*i < 0 && std::mem::size_of::<$t>() < 8).then(|| *i as $t).filter(|x| *x as i64 == *i)),
                    _ => None,
                }
            }
        }
    )*};
}

int!(u8, i8, u16, i16, u32, i32, u64, i64);

pub fn float(f: f32) -> Val {
    if f == 0.0 || (f.is_normal() && f.abs() < 1e16) {
        Val::Raw(format!("{f:?}"))
    } else {
        Val::Call("bits".into(), vec![Val::Raw(format!("0x{:08x}", f.to_bits()))])
    }
}

pub fn unfloat(v: &Val) -> Option<f32> {
    match v {
        Val::Call(n, a) if n == "bits" => a.first().and_then(Val::int).and_then(|i| u32::try_from(i).ok()).map(f32::from_bits),
        v => v.f32(),
    }
}

impl Prim for f32 {
    fn get(r: &mut Reader) -> Res<Self> {
        r.f32()
    }
    fn put(self, w: &mut Writer) {
        w.f32(self);
    }
    fn dump(self, _: Show) -> Val {
        float(self)
    }
    fn load(v: &Val) -> Option<Self> {
        unfloat(v)
    }
    fn same(self, o: Self) -> bool {
        self.to_bits() == o.to_bits()
    }
}

impl<T: Prim, const N: usize> Prim for [T; N]
where
    [T; N]: Default,
{
    fn get(r: &mut Reader) -> Res<Self> {
        let mut a = [T::default(); N];
        for x in &mut a {
            *x = T::get(r)?;
        }
        Ok(a)
    }
    fn put(self, w: &mut Writer) {
        self.iter().for_each(|x| x.put(w));
    }
    fn dump(self, s: Show) -> Val {
        Val::Tbl(self.iter().map(|x| (None, x.dump(s))).collect())
    }
    fn load(v: &Val) -> Option<Self> {
        let t = v.items();
        if t.len() != N || t.iter().any(|(k, _)| k.is_some()) {
            return None;
        }
        let mut a = [T::default(); N];
        for (x, (_, y)) in a.iter_mut().zip(t) {
            *x = T::load(y)?;
        }
        Some(a)
    }
    fn same(self, o: Self) -> bool {
        self.iter().zip(o.iter()).all(|(a, b)| a.same(*b))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Str {
    Z,
    L16,
    L16Z,
    L16Z0,
    L32,
    L32Z,
    N(usize),
}

pub trait Io: Sized {
    fn e(&self) -> Endian;
    fn mode(&self) -> Mode;
    fn val<T: Prim>(&mut self, k: &str, v: &mut T, s: Show) -> Res<()>;
    fn hide<T: Prim>(&mut self, v: &mut T) -> Res<()>;
    fn raw(&mut self, k: &str, v: &mut Vec<u8>, n: Len) -> Res<()>;
    fn text(&mut self, k: &str, v: &mut String, f: Str) -> Res<()>;
    fn list<T: Default>(&mut self, k: &str, v: &mut Vec<T>, n: Len, f: impl FnMut(&mut Self, &mut T) -> Res<()>) -> Res<()>;
    fn node(&mut self, k: &str, f: impl FnOnce(&mut Self) -> Res<()>) -> Res<()>;
    fn sized(&mut self, n: Len, f: impl FnOnce(&mut Self) -> Res<()>) -> Res<()>;
    fn magic(&mut self, m: &[u8; 4]) -> Res<()>;
    fn zero(&mut self, n: usize) -> Res<()>;
    fn note(&mut self, _: &str) {}
    fn any(&mut self, _: &str, _: &mut Val) -> Res<()> {
        Ok(())
    }
    fn has(&self, _: &str) -> bool {
        true
    }

    fn opt<T: Prim>(&mut self, k: &str, v: &mut T, s: Show) -> Res<()> {
        self.or(k, v, s, T::default())
    }

    fn or<T: Prim>(&mut self, k: &str, v: &mut T, s: Show, d: T) -> Res<()> {
        match self.mode() {
            Mode::Dump if v.same(d) => Ok(()),
            Mode::Load if !self.has(k) => {
                *v = d;
                Ok(())
            }
            _ => self.val(k, v, s),
        }
    }

    fn grid<T: Prim>(&mut self, k: &str, v: &mut Vec<T>, w: usize, n: Len) -> Res<()> {
        if self.bin() {
            return self.list(k, v, n, |s, x| s.val("", x, Show::Dec));
        }
        let mut rows: Vec<Vec<T>> = v.chunks(w.max(1)).map(<[T]>::to_vec).collect();
        self.list(k, &mut rows, Len::Rest, |s, r| s.list("", r, Len::Rest, |s, x| s.val("", x, Show::Dec)))?;
        if self.mode() == Mode::Load {
            if rows.iter().any(|r| r.len() != w) {
                return err(format!("{k}: every row must have {w} values"));
            }
            *v = rows.concat();
        }
        Ok(())
    }

    fn skip(&self, k: &str, empty: bool) -> bool {
        match self.mode() {
            Mode::Dump => empty,
            Mode::Load => !self.has(k),
            _ => false,
        }
    }

    fn blob(&mut self, k: &str, v: &mut Vec<u8>, n: Len) -> Res<()> {
        let text = v.strip_suffix(&[0]).unwrap_or(v);
        if self.mode() == Mode::Dump && text.iter().all(|c| (0x20..0x7f).contains(c)) {
            let mut s = String::from_utf8_lossy(v).into_owned();
            return self.text(k, &mut s, Str::L16);
        }
        self.raw(k, v, n)
    }
    fn tail(&mut self, k: &str, v: &mut Vec<u8>) -> Res<()> {
        match self.mode() {
            Mode::Dump if v.is_empty() => Ok(()),
            Mode::Load if !self.has(k) => Ok(v.clear()),
            _ => self.raw(k, v, Len::Rest),
        }
    }

    fn named(&mut self, k: &str, v: &mut u32, name: &str) -> Res<()> {
        self.or(k, v, Show::Hash, names::hash(name))
    }

    fn n(&self, c: u32) -> Len {
        if self.bin() {
            Len::N(c as usize)
        } else {
            Len::Rest
        }
    }
    fn konst<T: Prim + PartialEq + std::fmt::Debug>(&mut self, want: T) -> Res<()> {
        let mut v = want;
        self.hide(&mut v)?;
        if v != want {
            return err(format!("expected {want:?}, found {v:?}"));
        }
        Ok(())
    }
    fn bin(&self) -> bool {
        matches!(self.mode(), Mode::Read | Mode::Write)
    }
    fn reading(&self) -> bool {
        matches!(self.mode(), Mode::Read | Mode::Load)
    }
    fn u8(&mut self, k: &str, v: &mut u8) -> Res<()> {
        self.val(k, v, Show::Dec)
    }
    fn u16(&mut self, k: &str, v: &mut u16) -> Res<()> {
        self.val(k, v, Show::Dec)
    }
    fn u32(&mut self, k: &str, v: &mut u32) -> Res<()> {
        self.val(k, v, Show::Dec)
    }
    fn i16(&mut self, k: &str, v: &mut i16) -> Res<()> {
        self.val(k, v, Show::Dec)
    }
    fn i32(&mut self, k: &str, v: &mut i32) -> Res<()> {
        self.val(k, v, Show::Dec)
    }
    fn f32(&mut self, k: &str, v: &mut f32) -> Res<()> {
        self.val(k, v, Show::Dec)
    }
    fn hex(&mut self, k: &str, v: &mut u32) -> Res<()> {
        self.val(k, v, Show::Hex)
    }
    fn hash(&mut self, k: &str, v: &mut u32) -> Res<()> {
        self.val(k, v, Show::Hash)
    }
    fn flag<T: Prim>(&mut self, k: &str, v: &mut T) -> Res<()> {
        self.val(k, v, Show::Bool)
    }
    fn vec<const N: usize>(&mut self, k: &str, v: &mut [f32; N]) -> Res<()>
    where
        [f32; N]: Default,
    {
        self.val(k, v, Show::Dec)
    }
    fn hashes(&mut self, k: &str, v: &mut Vec<u32>, n: Len) -> Res<()> {
        self.list(k, v, n, |s, x| s.hash("", x))
    }
    fn floats(&mut self, k: &str, v: &mut Vec<f32>, n: Len) -> Res<()> {
        self.list(k, v, n, |s, x| s.f32("", x))
    }
    fn tagged<T: Default>(&mut self, k: &str, v: &mut Vec<T>, tag: &[u8; 4], end: &[u8; 4], mut f: impl FnMut(&mut Self, &mut T) -> Res<()>) -> Res<()> {
        match self.mode() {
            Mode::Dump | Mode::Load => return self.list(k, v, Len::Rest, f),
            Mode::Write => {
                for x in v.iter_mut() {
                    self.magic(tag)?;
                    f(self, x)?;
                }
                return self.magic(end);
            }
            Mode::Read => v.clear(),
        }
        loop {
            let mut t = 0u32;
            self.val("", &mut t, Show::Hex)?;
            if t == u32::from_le_bytes(*end) {
                return Ok(());
            }
            if t != u32::from_le_bytes(*tag) {
                return err(format!("{k}[{}]: expected {:?} or {:?}", v.len() + 1, String::from_utf8_lossy(tag), String::from_utf8_lossy(end)));
            }
            let mut x = T::default();
            f(self, &mut x).map_err(|e| Error::Msg(format!("{k}[{}]: {e}", v.len() + 1)))?;
            v.push(x);
        }
    }
}

fn count(r: &mut Reader, n: Len) -> Res<Option<usize>> {
    Ok(Some(match n {
        Len::U8 => r.u8()? as usize,
        Len::U16 => r.u16()? as usize,
        Len::U32 => r.u32()? as usize,
        Len::N(n) => n,
        Len::Rest => return Ok(None),
    }))
}

fn put_count(w: &mut Writer, n: Len, c: usize) -> Res<()> {
    let max = match n {
        Len::U8 => u8::MAX as usize,
        Len::U16 => u16::MAX as usize,
        Len::U32 => u32::MAX as usize,
        Len::N(n) if n != c => return err(format!("expected exactly {n} items, got {c}")),
        _ => return Ok(()),
    };
    if c > max {
        return err(format!("{c} items do not fit (at most {max})"));
    }
    match n {
        Len::U8 => w.u8(c as u8),
        Len::U16 => w.u16(c as u16),
        _ => w.u32(c as u32),
    };
    Ok(())
}

fn magic_bytes(m: &[u8; 4], e: Endian) -> [u8; 4] {
    let mut b = *m;
    if e == Endian::Be {
        b.reverse();
    }
    b
}

pub struct Read<'a> {
    pub r: Reader<'a>,
}

impl<'a> Read<'a> {
    pub fn new(d: &'a [u8], e: Endian) -> Self {
        Read { r: Reader::new(d, e) }
    }
}

impl Io for Read<'_> {
    fn e(&self) -> Endian {
        self.r.e
    }
    fn mode(&self) -> Mode {
        Mode::Read
    }
    fn val<T: Prim>(&mut self, _: &str, v: &mut T, _: Show) -> Res<()> {
        *v = T::get(&mut self.r)?;
        Ok(())
    }
    fn hide<T: Prim>(&mut self, v: &mut T) -> Res<()> {
        *v = T::get(&mut self.r)?;
        Ok(())
    }
    fn raw(&mut self, _: &str, v: &mut Vec<u8>, n: Len) -> Res<()> {
        *v = match count(&mut self.r, n)? {
            Some(n) => self.r.take(n)?.to_vec(),
            None => self.r.rest().to_vec(),
        };
        Ok(())
    }
    fn text(&mut self, _: &str, v: &mut String, f: Str) -> Res<()> {
        let b = match f {
            Str::Z => self.r.cstr()?,
            Str::L16 | Str::L16Z | Str::L16Z0 => {
                let n = self.r.u16()? as usize;
                self.r.take(n)?
            }
            Str::L32 | Str::L32Z => {
                let n = self.r.u32()? as usize;
                self.r.take(n)?
            }
            Str::N(n) => {
                let b = self.r.take(n)?;
                &b[..b.iter().position(|&c| c == 0).unwrap_or(n)]
            }
        };
        let b = match f {
            Str::L16Z0 if b.is_empty() => b,
            Str::L16Z | Str::L16Z0 | Str::L32Z => b.strip_suffix(&[0]).ok_or(Error::Bad("string is not zero-terminated"))?,
            _ => b,
        };
        *v = String::from_utf8(b.to_vec()).unwrap_or_else(|_| b.iter().map(|&c| c as char).collect());
        Ok(())
    }
    fn list<T: Default>(&mut self, k: &str, v: &mut Vec<T>, n: Len, mut f: impl FnMut(&mut Self, &mut T) -> Res<()>) -> Res<()> {
        v.clear();
        let at = |i: usize, e: Error| {
            let m = e.to_string();
            let deep = m.split(':').next().is_some_and(|p| p.contains('[') && !p.contains(' '));
            Error::Msg(format!("{k}[{}]{}{m}", i + 1, if deep { "." } else { ": " }))
        };
        match count(&mut self.r, n)? {
            Some(n) => {
                if n > self.r.left() {
                    return err(format!("count {n} is larger than the data left ({} bytes)", self.r.left()));
                }
                for i in 0..n {
                    let mut x = T::default();
                    f(self, &mut x).map_err(|e| at(i, e))?;
                    v.push(x);
                }
            }
            None => {
                while self.r.left() > 0 {
                    let mut x = T::default();
                    f(self, &mut x).map_err(|e| at(v.len(), e))?;
                    v.push(x);
                }
            }
        }
        Ok(())
    }
    fn node(&mut self, _: &str, f: impl FnOnce(&mut Self) -> Res<()>) -> Res<()> {
        f(self)
    }
    fn sized(&mut self, n: Len, f: impl FnOnce(&mut Self) -> Res<()>) -> Res<()> {
        let n = count(&mut self.r, n)?.unwrap_or(self.r.left());
        let sub = self.r.sub(n)?;
        let old = std::mem::replace(&mut self.r, sub);
        let res = f(self);
        let left = self.r.left();
        self.r = old;
        res?;
        if left > 0 {
            return err(format!("{left} unread bytes in a sized block"));
        }
        Ok(())
    }
    fn magic(&mut self, m: &[u8; 4]) -> Res<()> {
        let e = self.r.e;
        self.r.magic(&magic_bytes(m, e)).map_err(|_| Error::Msg(format!("expected magic {:?}", String::from_utf8_lossy(&magic_bytes(m, Endian::Be)))))
    }
    fn zero(&mut self, n: usize) -> Res<()> {
        if self.r.take(n)?.iter().any(|&b| b != 0) {
            return err(format!("expected {n} zero bytes"));
        }
        Ok(())
    }
}

pub struct Write {
    pub w: Writer,
}

impl Write {
    pub fn new(e: Endian) -> Self {
        Write { w: Writer::new(e) }
    }
}

impl Io for Write {
    fn e(&self) -> Endian {
        self.w.e
    }
    fn mode(&self) -> Mode {
        Mode::Write
    }
    fn val<T: Prim>(&mut self, _: &str, v: &mut T, _: Show) -> Res<()> {
        v.put(&mut self.w);
        Ok(())
    }
    fn hide<T: Prim>(&mut self, v: &mut T) -> Res<()> {
        v.put(&mut self.w);
        Ok(())
    }
    fn raw(&mut self, _: &str, v: &mut Vec<u8>, n: Len) -> Res<()> {
        put_count(&mut self.w, n, v.len())?;
        self.w.bytes(v);
        Ok(())
    }
    fn text(&mut self, _: &str, v: &mut String, f: Str) -> Res<()> {
        let b = v.as_bytes();
        let z = (matches!(f, Str::Z | Str::L16Z | Str::L32Z) || (f == Str::L16Z0 && !b.is_empty())) as usize;
        match f {
            Str::L16 | Str::L16Z | Str::L16Z0 => put_count(&mut self.w, Len::U16, b.len() + z)?,
            Str::L32 | Str::L32Z => put_count(&mut self.w, Len::U32, b.len() + z)?,
            Str::N(n) if b.len() > n => return err(format!("text {v:?} is longer than {n} bytes")),
            _ => {}
        }
        self.w.bytes(b);
        match f {
            Str::N(n) => self.w.pad(n - b.len()),
            _ if z == 1 => self.w.u8(0),
            _ => &mut self.w,
        };
        Ok(())
    }
    fn list<T: Default>(&mut self, _: &str, v: &mut Vec<T>, n: Len, mut f: impl FnMut(&mut Self, &mut T) -> Res<()>) -> Res<()> {
        put_count(&mut self.w, n, v.len())?;
        v.iter_mut().try_for_each(|x| f(self, x))
    }
    fn node(&mut self, _: &str, f: impl FnOnce(&mut Self) -> Res<()>) -> Res<()> {
        f(self)
    }
    fn sized(&mut self, n: Len, f: impl FnOnce(&mut Self) -> Res<()>) -> Res<()> {
        let at = self.w.pos();
        put_count(&mut self.w, n, 0)?;
        let st = self.w.pos();
        f(self)?;
        let size = self.w.pos() - st;
        match n {
            Len::U8 => self.w.set(at, u8::try_from(size).map_err(|_| Error::Bad("block too large"))?),
            Len::U16 => self.w.set(at, u16::try_from(size).map_err(|_| Error::Bad("block too large"))?),
            Len::U32 => self.w.set(at, size as u32),
            Len::N(k) if k != size => return err(format!("block is {size} bytes, expected {k}")),
            _ => &mut self.w,
        };
        Ok(())
    }
    fn magic(&mut self, m: &[u8; 4]) -> Res<()> {
        let e = self.w.e;
        self.w.bytes(&magic_bytes(m, e));
        Ok(())
    }
    fn zero(&mut self, n: usize) -> Res<()> {
        self.w.pad(n);
        Ok(())
    }
}

pub struct Dump {
    pub e: Endian,
    st: Vec<Vec<(Option<String>, Val)>>,
}

impl Dump {
    pub fn new(e: Endian) -> Self {
        Dump { e, st: vec![Vec::new()] }
    }

    fn push(&mut self, k: &str, v: Val) {
        let k = (!k.is_empty()).then(|| k.to_string());
        self.st.last_mut().unwrap().push((k, v));
    }

    pub fn finish(mut self) -> Vec<(Option<String>, Val)> {
        self.st.pop().unwrap_or_default()
    }

    fn frame(&mut self, f: impl FnOnce(&mut Self) -> Res<()>) -> Res<Val> {
        self.st.push(Vec::new());
        let r = f(self);
        let t = self.st.pop().unwrap();
        r?;
        Ok(match <[_; 1]>::try_from(t) {
            Ok([(None, v)]) => v,
            Ok(t) => Val::Tbl(t.into()),
            Err(t) => Val::Tbl(t),
        })
    }
}

pub fn hexs(b: &[u8]) -> Val {
    Val::Call("hex".into(), vec![Val::Str(b.chunks(4).map(ac_core::hex).collect::<Vec<_>>().join(" "))])
}

pub fn unhexs(v: &Val) -> Option<Vec<u8>> {
    match v {
        Val::Call(n, a) if n == "hex" => a.first().and_then(Val::str).and_then(|s| ac_core::unhex(s).ok()),
        Val::Str(s) => Some(s.as_bytes().to_vec()),
        _ => None,
    }
}

impl Io for Dump {
    fn e(&self) -> Endian {
        self.e
    }
    fn mode(&self) -> Mode {
        Mode::Dump
    }
    fn val<T: Prim>(&mut self, k: &str, v: &mut T, s: Show) -> Res<()> {
        self.push(k, v.dump(s));
        Ok(())
    }
    fn hide<T: Prim>(&mut self, _: &mut T) -> Res<()> {
        Ok(())
    }
    fn raw(&mut self, k: &str, v: &mut Vec<u8>, _: Len) -> Res<()> {
        self.push(k, hexs(v));
        Ok(())
    }
    fn text(&mut self, k: &str, v: &mut String, _: Str) -> Res<()> {
        self.push(k, Val::Str(v.clone()));
        Ok(())
    }
    fn list<T: Default>(&mut self, k: &str, v: &mut Vec<T>, _: Len, mut f: impl FnMut(&mut Self, &mut T) -> Res<()>) -> Res<()> {
        let mut t = Vec::with_capacity(v.len());
        for x in v.iter_mut() {
            t.push((None, self.frame(|s| f(s, x))?));
        }
        self.push(k, Val::Tbl(t));
        Ok(())
    }
    fn node(&mut self, k: &str, f: impl FnOnce(&mut Self) -> Res<()>) -> Res<()> {
        self.st.push(Vec::new());
        let r = f(self);
        let t = self.st.pop().unwrap();
        r?;
        self.push(k, Val::Tbl(t));
        Ok(())
    }
    fn sized(&mut self, _: Len, f: impl FnOnce(&mut Self) -> Res<()>) -> Res<()> {
        f(self)
    }
    fn magic(&mut self, _: &[u8; 4]) -> Res<()> {
        Ok(())
    }
    fn zero(&mut self, _: usize) -> Res<()> {
        Ok(())
    }
    fn note(&mut self, s: &str) {
        self.push("", Val::Note(s.to_string()));
    }
    fn any(&mut self, k: &str, v: &mut Val) -> Res<()> {
        self.push(k, v.clone());
        Ok(())
    }
}

pub struct Load {
    pub e: Endian,
    cur: Val,
    path: Vec<String>,
}

impl Load {
    pub fn new(v: Val, e: Endian) -> Self {
        Load { e, cur: v, path: Vec::new() }
    }

    fn at(&self, k: &str) -> String {
        let mut p = String::new();
        for s in self.path.iter().map(String::as_str).chain((!k.is_empty()).then_some(k)) {
            if !p.is_empty() && !s.starts_with('[') {
                p.push('.');
            }
            p.push_str(s);
        }
        if p.is_empty() {
            "value".into()
        } else {
            p
        }
    }

    fn fail<T>(&self, k: &str, m: impl std::fmt::Display) -> Res<T> {
        err(format!("{}: {m}", self.at(k)))
    }

    fn take(&mut self, k: &str) -> Res<Val> {
        if k.is_empty() {
            return Ok(std::mem::take(&mut self.cur));
        }
        let Val::Tbl(t) = &mut self.cur else { return self.fail(k, "expected a table here") };
        match t.iter().position(|(n, _)| n.as_deref() == Some(k)) {
            Some(i) => Ok(t.remove(i).1),
            None => self.fail(k, "missing"),
        }
    }

    pub fn rest(&self) -> Res<()> {
        let left: Vec<&str> = self.cur.items().iter().filter_map(|(k, _)| k.as_deref()).collect();
        let pos = self.cur.items().iter().filter(|(k, v)| k.is_none() && !matches!(v, Val::Note(_))).count();
        match (left.first(), pos) {
            (Some(_), _) => self.fail("", format!("unknown field{} {}", if left.len() > 1 { "s" } else { "" }, left.join(", "))),
            (None, n) if n > 0 => self.fail("", format!("{n} unexpected value(s) without a name")),
            _ => Ok(()),
        }
    }

    fn enter(&mut self, k: &str, v: Val, f: impl FnOnce(&mut Self) -> Res<()>) -> Res<()> {
        let old = std::mem::replace(&mut self.cur, v);
        self.path.push(k.to_string());
        let r = f(self).and_then(|_| if matches!(self.cur, Val::Tbl(_)) { self.rest() } else { Ok(()) });
        self.path.pop();
        self.cur = old;
        r
    }

    pub fn get(&mut self, k: &str) -> Option<Val> {
        self.take(k).ok()
    }
}

impl Io for Load {
    fn e(&self) -> Endian {
        self.e
    }
    fn has(&self, k: &str) -> bool {
        self.cur.key(k).is_some()
    }
    fn any(&mut self, k: &str, v: &mut Val) -> Res<()> {
        *v = self.take(k)?;
        Ok(())
    }
    fn mode(&self) -> Mode {
        Mode::Load
    }
    fn val<T: Prim>(&mut self, k: &str, v: &mut T, s: Show) -> Res<()> {
        let x = self.take(k)?;
        *v = match T::load(&x) {
            Some(y) => y,
            None => {
                let want = match (s, std::any::type_name::<T>()) {
                    (_, t) if t.starts_with('[') => format!("a list {{ ... }} of {}", t.split(';').nth(1).unwrap_or("").trim_end_matches(']').trim()),
                    (Show::Hash, _) => "a name in quotes or a hex hash".into(),
                    (Show::Bool, _) => "true or false".into(),
                    (_, "f32") => "a number".into(),
                    (_, t) => format!("a whole number that fits {t}"),
                };
                return self.fail(k, format!("expected {want}, found {}", ac_lua::data::pretty(&x, &Default::default())));
            }
        };
        Ok(())
    }
    fn hide<T: Prim>(&mut self, _: &mut T) -> Res<()> {
        Ok(())
    }
    fn raw(&mut self, k: &str, v: &mut Vec<u8>, n: Len) -> Res<()> {
        let x = self.take(k)?;
        *v = unhexs(&x).map_or_else(|| self.fail(k, "expected hex\"..\" bytes"), Ok)?;
        if let Len::N(n) = n {
            if v.len() != n {
                return self.fail(k, format!("expected {n} bytes, got {}", v.len()));
            }
        }
        Ok(())
    }
    fn text(&mut self, k: &str, v: &mut String, _: Str) -> Res<()> {
        match self.take(k)? {
            Val::Str(s) => *v = s,
            x => return self.fail(k, format!("expected text in quotes, found {}", ac_lua::data::pretty(&x, &Default::default()))),
        }
        Ok(())
    }
    fn list<T: Default>(&mut self, k: &str, v: &mut Vec<T>, n: Len, mut f: impl FnMut(&mut Self, &mut T) -> Res<()>) -> Res<()> {
        let Val::Tbl(t) = self.take(k)? else { return self.fail(k, "expected a list { ... }") };
        v.clear();
        let t: Vec<Val> = t.into_iter().filter(|(_, x)| !matches!(x, Val::Note(_))).map(|(_, x)| x).collect();
        if let Len::N(n) = n {
            if t.len() != n {
                return self.fail(k, format!("expected exactly {n} items, got {}", t.len()));
            }
        }
        self.path.push(k.to_string());
        let r = t.into_iter().enumerate().try_for_each(|(i, x)| {
            let mut y = T::default();
            self.enter(&format!("[{}]", i + 1), x, |s| f(s, &mut y))?;
            v.push(y);
            Ok(())
        });
        self.path.pop();
        r
    }
    fn node(&mut self, k: &str, f: impl FnOnce(&mut Self) -> Res<()>) -> Res<()> {
        let x = self.take(k)?;
        if !matches!(x, Val::Tbl(_)) {
            return self.fail(k, "expected a table { ... }");
        }
        self.enter(k, x, f)
    }
    fn sized(&mut self, _: Len, f: impl FnOnce(&mut Self) -> Res<()>) -> Res<()> {
        f(self)
    }
    fn magic(&mut self, _: &[u8; 4]) -> Res<()> {
        Ok(())
    }
    fn zero(&mut self, _: usize) -> Res<()> {
        Ok(())
    }
}
