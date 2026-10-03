use super::{header, meta, meta_endian, Format, Input, Opts, Out, Sink};
use crate::io::{err, Dump, Io, Load, Read, Write};
use ac_core::{Endian, Error, Res};
use ac_lua::Val;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

pub trait Data: Default + Sized {
    const ID: &'static str;
    const ABOUT: &'static str;
    const EXT: &'static [&'static str];
    const MAX: u64 = 64 << 20;

    fn io<S: Io>(&mut self, s: &mut S) -> Res<()>;

    fn fix(&mut self) -> Res<()> {
        Ok(())
    }

    fn size(&self) -> String;

    fn endian(_: &[u8]) -> Option<Endian> {
        None
    }

    fn read_as(d: &[u8], e: Endian) -> Res<Self> {
        let mut x = Self::default();
        let mut s = Read::new(d, e);
        x.io(&mut s)?;
        match s.r.left() {
            0 => Ok(x),
            n => err(format!("{n} unexpected bytes at the end (offset {})", s.r.pos())),
        }
    }

    fn read(d: &[u8]) -> Res<(Self, Endian)> {
        if let Some(e) = Self::endian(d) {
            return Ok((Self::read_as(d, e)?, e));
        }
        match Self::read_as(d, Endian::Le) {
            Ok(x) => Ok((x, Endian::Le)),
            Err(a) => Self::read_as(d, Endian::Be).map(|x| (x, Endian::Be)).map_err(|_| a),
        }
    }

    fn write(&mut self, e: Endian) -> Res<Vec<u8>> {
        self.fix()?;
        let mut s = Write::new(e);
        self.io(&mut s)?;
        Ok(s.w.finish())
    }

    fn dump(&mut self, e: Endian) -> Res<Vec<(Option<String>, Val)>> {
        let mut s = Dump::new(e);
        self.io(&mut s)?;
        Ok(s.finish())
    }

    fn load(v: Val, e: Endian) -> Res<Self> {
        let mut x = Self::default();
        let mut s = Load::new(v, e);
        x.io(&mut s)?;
        s.rest()?;
        Ok(x)
    }
}

pub struct D<T>(PhantomData<fn() -> T>);

impl<T> D<T> {
    pub const fn new() -> Self {
        D(PhantomData)
    }
}

fn parse<T: Data>(i: &mut Input) -> Res<(T, Endian)> {
    if i.len > T::MAX {
        return Err(Error::Bad("file is too large for this format"));
    }
    T::read(i.all()?)
}

pub fn text_of<T: Data>(file: &str, x: &mut T, e: Endian) -> Res<Val> {
    let mut t = header(file, T::ABOUT);
    t.extend(meta(T::ID, file, e));
    t.extend(x.dump(e)?);
    Ok(Val::Tbl(t))
}

pub fn from_text<T: Data>(v: &Val, opts: Opts) -> Res<(T, Endian)> {
    let e = meta_endian(v, opts);
    let body = Val::Tbl(v.items().iter().filter(|(k, _)| !matches!(k.as_deref(), Some("Format" | "File" | "Endian"))).cloned().collect());
    Ok((T::load(body, e)?, e))
}

impl<T: Data> Format for D<T> {
    fn id(&self) -> &'static str {
        T::ID
    }
    fn about(&self) -> &'static str {
        T::ABOUT
    }
    fn exts(&self) -> &'static [&'static str] {
        T::EXT
    }
    fn probe(&self, i: &mut Input) -> bool {
        parse::<T>(i).is_ok()
    }
    fn info(&self, i: &mut Input) -> Res<Vec<(&'static str, String)>> {
        let (x, e) = parse::<T>(i)?;
        Ok(vec![("contents", x.size()), ("byte order", super::endian_name(e).into())])
    }
    fn unpack(&self, i: &mut Input, o: &mut Out) -> Res<PathBuf> {
        let (mut x, e) = parse::<T>(i)?;
        let name = i.name.clone();
        o.text(&format!("{name}.lua"), &text_of(&name, &mut x, e)?)
    }
    fn pack(&self, src: &Path, meta: &Val, opts: Opts, out: &mut Sink) -> Res<()> {
        let (mut x, e) = from_text::<T>(meta, opts).map_err(|e| Error::Msg(format!("{}: {e}", src.display())))?;
        out.put(&x.write(e)?)
    }
}
