use super::world::data;
use super::Data;
use crate::io::{err, Io, Show};
use ac_core::{Endian, Res};

#[derive(Default)]
pub struct Hei {
    count: u32,
    grid: [u32; 2],
    cell: f32,
    corner: [f32; 3],
    tiles: Vec<Tile>,
}

#[derive(Default)]
struct Tile {
    size: [u32; 2],
    low: f32,
    high: f32,
    heights: Vec<u8>,
    start: [f32; 3],
    end: [f32; 3],
}

impl Hei {
    fn walk<S: Io>(&mut self, s: &mut S) -> Res<()> {
        s.magic(b"5IEH")?;
        s.hide(&mut self.count)?;
        s.val("Grid", &mut self.grid, Show::Dec)?;
        s.f32("CellSize", &mut self.cell)?;
        s.vec("Corner", &mut self.corner)?;
        s.note("Heights: one byte per cell, row by row; height = Low + byte * (High - Low) / 255");
        let n = s.n(self.count);
        s.list("Tiles", &mut self.tiles, n, |s, t| {
            s.magic(b"1IEH")?;
            s.val("Size", &mut t.size, Show::Dec)?;
            s.f32("Low", &mut t.low)?;
            s.f32("High", &mut t.high)?;
            let n = s.n(t.size[0] * t.size[1]);
            s.grid("Heights", &mut t.heights, t.size[0] as usize, n)?;
            s.vec("Min", &mut t.start)?;
            s.vec("Max", &mut t.end)
        })
    }

    fn fixup(&mut self) {
        self.count = self.tiles.len() as u32;
    }
}

data!(Hei, "hei", "hei", "terrain height tiles", |x| format!("{} tiles", x.tiles.len()), fixup);

#[derive(Default)]
pub struct Fst {
    head: [u32; 3],
    maps: [u32; 2],
    min: [f32; 3],
    max: [f32; 3],
}

impl Fst {
    fn walk<S: Io>(&mut self, s: &mut S) -> Res<()> {
        s.konst(0x0fffu32)?;
        s.konst(1u32)?;
        s.u32("Chunks", &mut self.head[1])?;
        s.u32("Vertices", &mut self.head[2])?;
        s.hash("DataMap", &mut self.maps[0])?;
        s.hash("ColorMap", &mut self.maps[1])?;
        s.vec("Min", &mut self.min)?;
        s.vec("Max", &mut self.max)
    }
}

impl Data for Fst {
    const ID: &'static str = "fst";
    const ABOUT: &'static str = "terrain surface info";
    const EXT: &'static [&'static str] = &["info"];
    fn io<S: Io>(&mut self, s: &mut S) -> Res<()> {
        self.walk(s)
    }
    fn size(&self) -> String {
        format!("{} chunks, {} vertices", self.head[1], self.head[2])
    }
    fn endian(d: &[u8]) -> Option<Endian> {
        id_endian(d, 0x0fff)
    }
}

fn id_endian(d: &[u8], id: u32) -> Option<Endian> {
    [Endian::Le, Endian::Be].into_iter().find(|&e| e.get::<u32>(d) == Some(id))
}

#[derive(Default)]
pub struct Fsm {
    counts: [u32; 2],
    sources: Vec<(u32, u32, u32)>,
    objects: Vec<Monument>,
}

#[derive(Default)]
struct Monument {
    id: u32,
    version: u32,
    mesh: u32,
    min: [f32; 3],
    max: [f32; 3],
    matrix: [f32; 16],
}

impl Fsm {
    fn walk<S: Io>(&mut self, s: &mut S) -> Res<()> {
        s.konst(0x0fu32)?;
        s.konst(1u32)?;
        s.hide(&mut self.counts[0])?;
        s.hide(&mut self.counts[1])?;
        let n = s.n(self.counts[0]);
        s.list("Sources", &mut self.sources, n, |s, x| {
            s.hex("Id", &mut x.0)?;
            s.u32("Version", &mut x.1)?;
            s.hash("Name", &mut x.2)
        })?;
        s.note("Matrix: world transform, 16 values row by row; Min/Max: bounding box (version 2)");
        let n = s.n(self.counts[1]);
        s.list("Monuments", &mut self.objects, n, |s, m| {
            s.hex("Id", &mut m.id)?;
            s.u32("Version", &mut m.version)?;
            s.hash("Mesh", &mut m.mesh)?;
            match m.version {
                1 => {}
                2 => {
                    s.vec("Min", &mut m.min)?;
                    s.vec("Max", &mut m.max)?;
                }
                v => return err(format!("unknown monument version {v}")),
            }
            s.vec("Matrix", &mut m.matrix)
        })
    }

    fn fixup(&mut self) {
        self.counts = [self.sources.len() as u32, self.objects.len() as u32];
    }
}

impl Data for Fsm {
    const ID: &'static str = "fsm";
    const ABOUT: &'static str = "monument (landmark) placement";
    const EXT: &'static [&'static str] = &["info"];
    fn io<S: Io>(&mut self, s: &mut S) -> Res<()> {
        self.walk(s)
    }
    fn fix(&mut self) -> Res<()> {
        self.fixup();
        Ok(())
    }
    fn size(&self) -> String {
        format!("{} monuments", self.objects.len())
    }
    fn endian(d: &[u8]) -> Option<Endian> {
        id_endian(d, 0x0f)
    }
}
