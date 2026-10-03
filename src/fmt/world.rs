use super::Data;
use crate::io::{err, Io, Len, Mode, Show, Str};
use ac_core::{Endian, Res};

macro_rules! data {
    ($t:ty, $id:literal, $ext:literal, $about:literal, |$x:ident| $size:expr $(, $fix:ident)?) => {
        impl Data for $t {
            const ID: &'static str = $id;
            const ABOUT: &'static str = $about;
            const EXT: &'static [&'static str] = &[$ext];
            fn io<S: Io>(&mut self, s: &mut S) -> Res<()> {
                self.walk(s)
            }
            $(fn fix(&mut self) -> Res<()> {
                self.$fix();
                Ok(())
            })?
            fn size(&self) -> String {
                let $x = self;
                $size
            }
        }
    };
}
pub(crate) use data;


fn pos_rot<S: Io>(s: &mut S, pos: &mut [f32; 3], rot: &mut [f32; 9]) -> Res<()> {
    s.vec("Position", pos)?;
    s.vec("Rotation", rot)
}

#[derive(Default)]
pub struct Ambush {
    points: Vec<Ambushed>,
}

#[derive(Default)]
struct Ambushed {
    object: u32,
    id: u32,
    unused: [u32; 2],
    pos: [f32; 3],
    rot: [f32; 9],
}

impl Ambush {
    fn walk<S: Io>(&mut self, s: &mut S) -> Res<()> {
        s.magic(b"I0LA")?;
        s.list("Points", &mut self.points, Len::U32, |s, p| {
            s.hash("Object", &mut p.object)?;
            s.hash("Id", &mut p.id)?;
            s.opt("Unused", &mut p.unused, Show::Hex)?;
            pos_rot(s, &mut p.pos, &mut p.rot)
        })
    }
}

data!(Ambush, "ambush", "ambush", "ambush spawn points", |x| format!("{} points", x.points.len()));

#[derive(Default)]
pub struct WaterCtrl {
    points: Vec<(u32, [f32; 3], Vec<u8>)>,
}

impl WaterCtrl {
    fn walk<S: Io>(&mut self, s: &mut S) -> Res<()> {
        s.magic(b"70CW")?;
        s.list("Points", &mut self.points, Len::U32, |s, p| {
            s.hash("Name", &mut p.0)?;
            s.vec("Position", &mut p.1)?;
            if s.skip("Data", p.2.is_empty()) {
                return Ok(());
            }
            s.raw("Data", &mut p.2, Len::U32)
        })
    }
}

data!(WaterCtrl, "waterctrl", "waterctrl", "water control points", |x| format!("{} points", x.points.len()));

#[derive(Default)]
pub struct WaterFlow {
    points: Vec<Flow>,
}

#[derive(Default)]
struct Flow {
    name: u32,
    pos: [f32; 3],
    rot: [f32; 4],
    strength: f32,
}

impl WaterFlow {
    fn walk<S: Io>(&mut self, s: &mut S) -> Res<()> {
        s.magic(b"70FW")?;
        s.note("Id: ignored by the game; Rotation: quaternion; flow = Rotation applied to the forward axis, times Strength");
        s.list("Points", &mut self.points, Len::U32, |s, p| {
            s.hash("Id", &mut p.name)?;
            s.vec("Position", &mut p.pos)?;
            s.vec("Rotation", &mut p.rot)?;
            s.f32("Strength", &mut p.strength)
        })
    }
}

data!(WaterFlow, "waterflow", "waterflow", "water flow points", |x| format!("{} points", x.points.len()));

#[derive(Default)]
pub struct FreePlay {
    count: u32,
    targets: u32,
    points: Vec<Free>,
}

#[derive(Default)]
struct Free {
    node: u32,
    object: u32,
    kind: u32,
    also: u32,
    pos: [f32; 3],
    targets: Vec<u32>,
}

impl FreePlay {
    fn walk<S: Io>(&mut self, s: &mut S) -> Res<()> {
        s.magic(b"40PF")?;
        s.hide(&mut self.count)?;
        s.hide(&mut self.targets)?;
        let n = s.n(self.count);
        s.note("Node: freeplay node, Object: fp_amb_* object, Kind: type tag (e.g. Armored_CT)\nAlso: second object enabled together with the point (0 = none), Targets: target locator nodes");
        s.list("Points", &mut self.points, n, |s, p| {
            s.hash("Node", &mut p.node)?;
            s.hash("Object", &mut p.object)?;
            s.hash("Kind", &mut p.kind)?;
            s.opt("Also", &mut p.also, Show::Hash)?;
            s.vec("Position", &mut p.pos)?;
            s.hashes("Targets", &mut p.targets, Len::U32)
        })?;
        if s.mode() == Mode::Read && self.points.iter().map(|p| p.targets.len()).sum::<usize>() != self.targets as usize {
            return err("target total in the header does not match the points");
        }
        Ok(())
    }
    fn fixup(&mut self) {
        self.count = self.points.len() as u32;
        self.targets = self.points.iter().map(|p| p.targets.len() as u32).sum();
    }
}

data!(FreePlay, "freeplay", "freeplay", "free-play activity points", |x| format!("{} points", x.points.len()), fixup);

#[derive(Default)]
pub struct HqPoints {
    points: Vec<Hq>,
}

#[derive(Default)]
struct Hq {
    a: u32,
    b: u32,
    parent: String,
    name: String,
    pos: [f32; 3],
    rot: [f32; 9],
}

impl HqPoints {
    fn walk<S: Io>(&mut self, s: &mut S) -> Res<()> {
        s.magic(b"60PH")?;
        s.list("Points", &mut self.points, Len::U32, |s, p| {
            s.hash("Id", &mut p.a)?;
            s.hash("Object", &mut p.b)?;
            s.text("Parent", &mut p.parent, Str::L16Z)?;
            s.text("Name", &mut p.name, Str::L16Z)?;
            pos_rot(s, &mut p.pos, &mut p.rot)
        })
    }
}

data!(HqPoints, "hqpoints", "hqpoints", "resistance HQ points", |x| format!("{} points", x.points.len()));

#[derive(Default)]
pub struct Spore {
    points: Vec<Spawn>,
}

#[derive(Default)]
struct Spawn {
    a: u32,
    b: u32,
    id: u32,
    pos: [f32; 3],
    rot: [f32; 9],
    props: Vec<Prop>,
}

#[derive(Default)]
struct Prop {
    tag: u32,
    len: u32,
    value: f32,
    bytes: Vec<u8>,
}

impl Spore {
    fn walk<S: Io>(&mut self, s: &mut S) -> Res<()> {
        s.magic(b"I0SS")?;
        s.list("Points", &mut self.points, Len::U32, |s, p| {
            s.hash("Object", &mut p.a)?;
            s.hash("Id", &mut p.b)?;
            s.opt("Reserved", &mut p.id, Show::Hex)?;
            pos_rot(s, &mut p.pos, &mut p.rot)?;
            s.sized(Len::U32, |s| {
                s.list("Props", &mut p.props, Len::U32, |s, q| {
                    s.hash("Tag", &mut q.tag)?;
                    if s.mode() == Mode::Load {
                        q.len = if s.has("Value") { 4 } else { 0 };
                    }
                    s.hide(&mut q.len)?;
                    if q.len == 4 {
                        s.f32("Value", &mut q.value)
                    } else {
                        let n = s.n(q.len);
                        s.raw("Bytes", &mut q.bytes, n)
                    }
                })
            })
        })
    }
    fn fixup(&mut self) {
        for q in self.points.iter_mut().flat_map(|p| p.props.iter_mut()) {
            if q.len != 4 {
                q.len = q.bytes.len() as u32;
            }
        }
    }
}

data!(Spore, "spore", "spore", "spawn points with properties", |x| format!("{} points", x.points.len()), fixup);

#[derive(Default)]
pub struct Defen {
    count: u32,
    nodes: Vec<u32>,
}

impl Defen {
    fn walk<S: Io>(&mut self, s: &mut S) -> Res<()> {
        s.hide(&mut self.count)?;
        s.note("EditNodes entries that are switched on (if Count is set, the game reads only the first Count)");
        s.hashes("Nodes", &mut self.nodes, Len::Rest)?;
        match s.mode() {
            Mode::Read if self.count as usize > self.nodes.len() || self.nodes.len() - self.count as usize > 8 => err("count does not match the file size"),
            Mode::Dump if self.count as usize != self.nodes.len() => s.u32("Count", &mut self.count),
            Mode::Load if s.has("Count") => s.u32("Count", &mut self.count),
            Mode::Load => {
                self.count = self.nodes.len() as u32;
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

data!(Defen, "defen", "defen", "active EditNodes list", |x| format!("{} nodes", x.nodes.len()));

#[derive(Default)]
pub struct RndNodes {
    nodes: Vec<Rnd>,
}

#[derive(Default)]
struct Rnd {
    id: u32,
    pos: [f32; 3],
    options: Vec<(u32, f32)>,
}

impl RndNodes {
    fn walk<S: Io>(&mut self, s: &mut S) -> Res<()> {
        s.magic(b"70NR")?;
        s.note("Options: hash and value pairs, the game uses the first 6");
        s.list("Nodes", &mut self.nodes, Len::U32, |s, n| {
            s.hash("Id", &mut n.id)?;
            s.vec("Position", &mut n.pos)?;
            s.list("Options", &mut n.options, Len::U16, |s, o| {
                s.hash("Hash", &mut o.0)?;
                s.f32("Value", &mut o.1)
            })
        })
    }
}

data!(RndNodes, "rndnodes", "rndnodes", "random nodes", |x| format!("{} nodes", x.nodes.len()));

#[derive(Default)]
pub struct Tuv {
    cells: Vec<(u32, [f32; 4])>,
}

impl Tuv {
    fn walk<S: Io>(&mut self, s: &mut S) -> Res<()> {
        s.note("Cell: texture position U, V and cell size W, H in the particle atlas");
        s.list("Cells", &mut self.cells, Len::U32, |s, c| {
            s.hash("Name", &mut c.0)?;
            s.vec("Cell", &mut c.1)
        })?;
        if s.bin() && self.cells.is_empty() {
            return err("empty atlas");
        }
        Ok(())
    }
}

impl Data for Tuv {
    const ID: &'static str = "tuv";
    const ABOUT: &'static str = "particle texture atlas";
    const EXT: &'static [&'static str] = &["tuv"];
    fn io<S: Io>(&mut self, s: &mut S) -> Res<()> {
        self.walk(s)
    }
    fn size(&self) -> String {
        format!("{} cells", self.cells.len())
    }
    fn endian(_: &[u8]) -> Option<Endian> {
        Some(Endian::Be)
    }
}
