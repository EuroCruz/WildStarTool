use crate::io::err;
use ac_core::{tag, Endian, Reader, Res, Writer};
use ac_fmt::img::{Bc, Dds, Fmt};

const CHUNK: usize = 0x18_0000;

pub struct Tex<'a> {
    pub name: String,
    pub code: u32,
    pub flags: u32,
    pub w: u16,
    pub h: u16,
    pub mips: u16,
    pub unc: u32,
    pub streams: Vec<&'a [u8]>,
}

pub fn parse(b: &[u8], e: Endian) -> Option<Tex<'_>> {
    let mut r = Reader::new(b, e);
    let n = r.u32().ok()? as usize;
    let name = r.take(n).ok()?;
    if n == 0 || n > 0x400 || !name.iter().all(|c| (0x20..0x7f).contains(c)) {
        return None;
    }
    let (code, flags) = (r.u32().ok()?, r.u32().ok()?);
    let (w, h, mips) = (r.u16().ok()?, r.u16().ok()?, r.u16().ok()?);
    let (unc, n) = (r.u32().ok()?, r.u32().ok()?);
    if w == 0 || h == 0 || mips == 0 || n == 0 || n > 4096 {
        return None;
    }
    let mut streams = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let s = r.u32().ok()? as usize;
        streams.push(r.take(s).ok()?);
    }
    let name = String::from_utf8_lossy(name).into_owned();
    (r.left() == 0).then_some(Tex { name, code, flags, w, h, mips, unc, streams })
}

fn fmt(code: u32) -> Option<Fmt> {
    match code {
        c if c == tag(b"DXT1") => Some(Fmt::Bc(Bc::Bc1)),
        c if c == tag(b"DXT5") => Some(Fmt::Bc(Bc::Bc3)),
        21 => Some(Fmt::Bgra8),
        _ => None,
    }
}

fn code(f: Fmt) -> Option<u32> {
    match f {
        Fmt::Bc(Bc::Bc1) => Some(tag(b"DXT1")),
        Fmt::Bc(Bc::Bc3) => Some(tag(b"DXT5")),
        Fmt::Bgra8 => Some(21),
        _ => None,
    }
}

fn dds_len(x: &Dds) -> usize {
    (0..x.mips).map(|m| x.fmt.size((x.w >> m).max(1), (x.h >> m).max(1))).sum()
}

fn dims(w: u16, h: u16, m: u32) -> (u32, u32) {
    ((w as u32 >> m).max(1), (h as u32 >> m).max(1))
}

struct Xe {
    f: Fmt,
    block: u32,
    n: u32,
    swap: usize,
}

const XE: [(u32, Xe); 3] = [
    (0x1a20_0152, Xe { f: Fmt::Bc(Bc::Bc1), block: 4, n: 8, swap: 2 }),
    (0x1a20_0154, Xe { f: Fmt::Bc(Bc::Bc3), block: 4, n: 16, swap: 2 }),
    (0x1828_0186, Xe { f: Fmt::Bgra8, block: 1, n: 4, swap: 4 }),
];

fn swap(d: &mut [u8], k: usize) {
    d.chunks_exact_mut(k).for_each(<[u8]>::reverse);
}

fn dds(t: &Tex, f: Fmt, data: Vec<u8>) -> Vec<u8> {
    Dds { w: t.w as u32, h: t.h as u32, depth: 1, mips: t.mips as u32, layers: 1, cube: false, fmt: f, data }.write()
}

fn xe_dds(t: &Tex) -> Option<Vec<u8>> {
    let x = &XE.iter().find(|x| x.0 == t.code)?.1;
    let mut u = Vec::with_capacity(t.unc as usize);
    for s in &t.streams {
        let n = (t.unc as usize).checked_sub(u.len())?.min(CHUNK);
        u.extend(ac_pack::lzx::decode(s, n).ok()?);
    }
    if u.len() != t.unc as usize || u.len() != ac_fmt::img::tiled360_mips_len(t.w as u32, t.h as u32, t.mips as u32, x.block, x.n) {
        return None;
    }
    swap(&mut u, x.swap);
    Some(dds(t, x.f, ac_fmt::img::untile360_mips(&u, t.w as u32, t.h as u32, t.mips as u32, x.block, x.n)?))
}

struct Ps3 {
    f: Fmt,
    n: u32,
    swizzled: bool,
}

const PS3: [(u32, Ps3); 5] = [
    (0x86, Ps3 { f: Fmt::Bc(Bc::Bc1), n: 0, swizzled: false }),
    (0x88, Ps3 { f: Fmt::Bc(Bc::Bc3), n: 0, swizzled: false }),
    (0x85, Ps3 { f: Fmt::Bgra8, n: 4, swizzled: true }),
    (0x9e, Ps3 { f: Fmt::Bgrx8, n: 4, swizzled: true }),
    (0x9b, Ps3 { f: Fmt::Float { ch: 4, half: false }, n: 16, swizzled: true }),
];

fn ps3_mips(d: &[u8], w: u16, h: u16, mips: u32, p: &Ps3, back: bool) -> Option<Vec<u8>> {
    let mut o = Vec::with_capacity(d.len());
    let mut at = 0;
    for m in 0..mips {
        let (w, h) = dims(w, h, m);
        let k = p.f.size(w, h);
        let mut s = d.get(at..at + k)?.to_vec();
        at += k;
        if p.swizzled {
            swap(&mut s, 4);
            s = if back { ac_fmt::img::morton(&s, w, h, p.n) } else { ac_fmt::img::unmorton(&s, w, h, p.n) };
        }
        o.extend(s);
    }
    (at == d.len()).then_some(o)
}

fn ps3_dds(t: &Tex) -> Option<Vec<u8>> {
    let p = &PS3.iter().find(|x| x.0 == t.code)?.1;
    let mut u = Vec::with_capacity(t.unc as usize);
    for s in &t.streams {
        if ac_pack::segs::is_segs(s) {
            u.extend(ac_pack::segs::decode(s).ok()?);
        } else {
            u.extend_from_slice(s);
        }
    }
    if u.len() != t.unc as usize {
        return None;
    }
    Some(dds(t, p.f, ps3_mips(&u, t.w, t.h, t.mips as u32, p, false)?))
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Gpu {
    Pc,
    Xbox360,
    Ps3,
}

impl Gpu {
    pub fn name(self) -> &'static str {
        match self {
            Gpu::Pc => "pc",
            Gpu::Xbox360 => "xbox360",
            Gpu::Ps3 => "ps3",
        }
    }

    pub fn of(t: &Tex, e: Endian) -> Gpu {
        match e {
            Endian::Le => Gpu::Pc,
            _ if PS3.iter().any(|x| x.0 == t.code) => Gpu::Ps3,
            _ => Gpu::Xbox360,
        }
    }
}

pub fn to_dds(t: &Tex, e: Endian) -> Option<Vec<u8>> {
    match Gpu::of(t, e) {
        Gpu::Xbox360 => return xe_dds(t),
        Gpu::Ps3 => return ps3_dds(t),
        Gpu::Pc => {}
    }
    let f = fmt(t.code)?;
    let mut u = Vec::with_capacity(t.unc as usize);
    for (i, s) in t.streams.iter().enumerate() {
        let d = ac_pack::flate::unzlib(s).ok()?;
        if d.is_empty() || d.len() > CHUNK || (i + 1 < t.streams.len() && d.len() != CHUNK) {
            return None;
        }
        u.extend(d);
    }
    if u.len() != t.unc as usize {
        return None;
    }
    let mut r = Reader::new(&u, e);
    let mut data = Vec::with_capacity(u.len());
    for m in 0..t.mips as u32 {
        let (w, h) = dims(t.w, t.h, m);
        let size = f.size(w, h);
        let head = [r.u32().ok()?, r.u32().ok()?, r.u32().ok()?, r.u32().ok()?, r.u32().ok()?, r.u32().ok()?];
        if head != [m, w, h, 0, 1, size as u32] {
            return None;
        }
        data.extend_from_slice(r.take(size).ok()?);
    }
    (r.left() == 0).then(|| dds(t, f, data))
}

pub fn from_dds(d: &[u8], name: &str, flags: u32, gpu: Gpu) -> Res<(Vec<u8>, u32)> {
    let e = if gpu == Gpu::Pc { Endian::Le } else { Endian::Be };
    let x = Dds::parse(d)?;
    if x.layers != 1 || x.depth != 1 || x.w > 0xffff || x.h > 0xffff {
        return err("texture must be a plain 2D image (no cube, array or volume)");
    }
    let (c, u, streams) = if gpu == Gpu::Ps3 {
        let Some((c, p)) = PS3.iter().find(|k| k.1.f == x.fmt) else { return err("texture must be DXT1, DXT5, 32-bit BGRA/BGRX or RGBA float") };
        let u = ps3_mips(&x.data[..x.data.len().min(dds_len(&x))], x.w as u16, x.h as u16, x.mips, p, true).ok_or(ac_core::Error::Bad("dds data too short"))?;
        let streams: Vec<Vec<u8>> = u.chunks(CHUNK).map(|c| ac_pack::segs::encode(c, Endian::Be, 6)).collect();
        (*c, u, streams)
    } else if gpu == Gpu::Xbox360 {
        let Some((c, k)) = XE.iter().find(|k| k.1.f == x.fmt) else { return err("texture must be DXT1, DXT5 or 32-bit BGRA") };
        let all = x.surface(0, 0).map(|_| &x.data[..]).ok_or(ac_core::Error::Bad("dds data too short"))?;
        let mut u = ac_fmt::img::tile360_mips(all, x.w, x.h, x.mips, k.block, k.n).ok_or(ac_core::Error::Bad("dds data too short"))?;
        swap(&mut u, k.swap);
        let streams: Vec<Vec<u8>> = u.chunks(CHUNK).map(|c| ac_pack::lzx::encode(c, 9)).collect();
        (*c, u, streams)
    } else {
        let Some(c) = code(x.fmt) else { return err("texture must be DXT1, DXT5 or 32-bit BGRA") };
        let mut u = Writer::new(e);
        for m in 0..x.mips {
            let (w, h) = dims(x.w as u16, x.h as u16, m);
            let s = x.surface(0, m).ok_or(ac_core::Error::Bad("dds data too short"))?;
            u.u32(m).u32(w).u32(h).u32(0).u32(1).u32(s.len() as u32).bytes(s);
        }
        let u = u.finish();
        let streams: Vec<Vec<u8>> = u.chunks(CHUNK).map(|c| ac_pack::flate::zlib_zc(c, 1)).collect();
        (c, u, streams)
    };
    let mut w = Writer::new(e);
    w.u32(name.len() as u32).bytes(name.as_bytes()).u32(c).u32(flags).u16(x.w as u16).u16(x.h as u16).u16(x.mips as u16);
    w.u32(u.len() as u32).u32(streams.len() as u32);
    for s in &streams {
        w.u32(s.len() as u32).bytes(s);
    }
    Ok((w.finish(), u.len() as u32))
}
