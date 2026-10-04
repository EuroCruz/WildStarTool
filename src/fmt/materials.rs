use super::Data;
use crate::io::{Io, Len, Show};
use ac_core::{Endian, Res};

#[derive(Default)]
pub struct Materials {
    capacity: u32,
    unknown: u32,
    render: Vec<Vec<[u32; 2]>>,
    texture: Vec<Vec<[u32; 2]>>,
    cpu: Vec<Vec<(u32, [f32; 4])>>,
    pixel: Vec<Params>,
    vertex: Vec<Params>,
    textures: Vec<u32>,
    passes: Vec<Pass>,
    list: Vec<Material>,
}

#[derive(Default)]
struct Params {
    vectors: Vec<[f32; 4]>,
    ints: Vec<[i32; 4]>,
    bools: Vec<u32>,
}

#[derive(Default)]
struct Pass {
    name: u32,
    flags: u32,
    refs: [i32; 5],
    shaders: [u32; 2],
}

#[derive(Default)]
struct Material {
    key: u32,
    aliases: Vec<u32>,
    id: u32,
    textures: u32,
    texture: i32,
    pass: i32,
}

const REFS: [&str; 5] = ["RenderState", "TextureState", "CpuParams", "PixelParams", "VertexParams"];

fn states<S: Io>(s: &mut S, k: &str, v: &mut Vec<Vec<[u32; 2]>>, n: u32) -> Res<()> {
    let n = s.n(n);
    s.list(k, v, n, |s, a| s.list("", a, Len::U32, |s, p| s.val("", p, Show::Dec)))
}

fn params<S: Io>(s: &mut S, k: &str, v: &mut Vec<Params>, n: u32) -> Res<()> {
    let n = s.n(n);
    s.list(k, v, n, |s, p| {
        s.list("Vectors", &mut p.vectors, Len::U32, |s, x| s.vec("", x))?;
        s.list("Ints", &mut p.ints, Len::U32, |s, x| s.val("", x, Show::Dec))?;
        s.list("Bools", &mut p.bools, Len::U32, |s, x| s.val("", x, Show::Dec))
    })
}

impl Materials {
    fn walk<S: Io>(&mut self, s: &mut S) -> Res<()> {
        s.magic(b"OASW")?;
        let sum = |v: &[Params], f: fn(&Params) -> usize| v.iter().map(f).sum::<usize>() as u32;
        let mut c = [
            self.capacity,
            self.list.len() as u32,
            self.render.len() as u32,
            self.render.iter().map(Vec::len).sum::<usize>() as u32,
            self.texture.len() as u32,
            self.texture.iter().map(Vec::len).sum::<usize>() as u32,
            self.vertex.len() as u32,
            sum(&self.vertex, |p| p.vectors.len()),
            sum(&self.vertex, |p| p.ints.len()),
            sum(&self.vertex, |p| p.bools.len()),
            self.pixel.len() as u32,
            sum(&self.pixel, |p| p.vectors.len()),
            sum(&self.pixel, |p| p.ints.len()),
            sum(&self.pixel, |p| p.bools.len()),
            self.cpu.len() as u32,
            self.cpu.iter().map(Vec::len).sum::<usize>() as u32,
            self.textures.len() as u32,
            self.unknown,
            self.passes.len() as u32,
        ];
        s.u32("Capacity", &mut c[0])?;
        for (i, x) in c.iter_mut().enumerate().skip(1) {
            if i == 17 {
                s.u32("Unknown", x)?;
            } else {
                s.hide(x)?;
            }
        }
        (self.capacity, self.unknown) = (c[0], c[17]);
        s.note("Sections in file order: WSST render and texture states { state, value },\nWSCP CPU params { register, {x, y, z, w} }, WSPP/WSVP shader params, WSTX textures, WSPA passes, WSMA materials.\nIndexes point into those lists (-1 = none)");
        s.magic(b"TSSW")?;
        states(s, "RenderStates", &mut self.render, c[2])?;
        states(s, "TextureStates", &mut self.texture, c[4])?;
        s.magic(b"PCSW")?;
        let n = s.n(c[14]);
        s.list("CpuParams", &mut self.cpu, n, |s, a| {
            s.list("", a, Len::U32, |s, p| {
                s.u32("Register", &mut p.0)?;
                s.vec("Value", &mut p.1)
            })
        })?;
        s.magic(b"PPSW")?;
        params(s, "PixelParams", &mut self.pixel, c[10])?;
        s.magic(b"PVSW")?;
        params(s, "VertexParams", &mut self.vertex, c[6])?;
        s.magic(b"XTSW")?;
        let n = s.n(c[16]);
        s.hashes("Textures", &mut self.textures, n)?;
        s.magic(b"APSW")?;
        let n = s.n(c[18]);
        s.list("Passes", &mut self.passes, n, |s, p| {
            s.hash("Name", &mut p.name)?;
            s.hex("Flags", &mut p.flags)?;
            for (k, r) in REFS.iter().zip(&mut p.refs) {
                s.i32(k, r)?;
            }
            s.hash("PixelShader", &mut p.shaders[0])?;
            s.hash("VertexShader", &mut p.shaders[1])
        })?;
        s.magic(b"AMSW")?;
        let n = s.n(c[1]);
        s.list("Materials", &mut self.list, n, |s, m| {
            s.hash("Key", &mut m.key)?;
            s.hashes("Aliases", &mut m.aliases, Len::U32)?;
            s.hash("Id", &mut m.id)?;
            s.u32("TextureCount", &mut m.textures)?;
            s.i32("Texture", &mut m.texture)?;
            s.i32("Pass", &mut m.pass)
        })
    }
}

impl Data for Materials {
    const ID: &'static str = "materials";
    const ABOUT: &'static str = "level materials (render states, shader params, passes)";
    const EXT: &'static [&'static str] = &["materials"];
    fn io<S: Io>(&mut self, s: &mut S) -> Res<()> {
        self.walk(s)
    }
    fn size(&self) -> String {
        format!("{} materials, {} passes", self.list.len(), self.passes.len())
    }
    fn endian(d: &[u8]) -> Option<Endian> {
        match d.get(..4)? {
            b"OASW" => Some(Endian::Le),
            b"WSAO" => Some(Endian::Be),
            _ => None,
        }
    }
}
