use super::albs::Block;
use super::Data;
use crate::io::{err, Io, Len, Mode, Show, Str};
use crate::names;
use ac_core::{Endian, Res};

fn endian(d: &[u8]) -> Option<Endian> {
    match d.get(..4)? {
        b"6PAM" => Some(Endian::Le),
        b"MAP6" => Some(Endian::Be),
        _ => None,
    }
}

#[derive(Default)]
struct Cell {
    id: u32,
    name: String,
    min: [f32; 3],
    max: [f32; 3],
    unknown: u16,
    level: u16,
    block: Block,
}

fn name<S: Io>(s: &mut S, c: &mut Cell) -> Res<()> {
    if !s.skip("Name", c.name.is_empty()) {
        s.text("Name", &mut c.name, Str::L16Z0)?;
    }
    Ok(())
}

fn cell<S: Io>(s: &mut S, c: &mut Cell, always: bool) -> Res<()> {
    let load = s.mode() == Mode::Load;
    if load {
        name(s, c)?;
    }
    match c.name.is_empty() {
        true => s.hash("Id", &mut c.id)?,
        false => s.or("Id", &mut c.id, Show::Hash, names::hash(&c.name))?,
    }
    if !load {
        name(s, c)?;
    }
    s.vec("Min", &mut c.min)?;
    s.vec("Max", &mut c.max)?;
    s.opt("Unknown", &mut c.unknown, Show::Dec)?;
    s.u16("Level", &mut c.level)?;
    if always || c.level >= 2 {
        c.block.show_counts = true;
        c.block.walk(s)?;
    }
    Ok(())
}

fn cells<S: Io>(s: &mut S, k: &str, v: &mut Vec<Cell>, n: Len, always: bool) -> Res<()> {
    s.list(k, v, n, |s, c| cell(s, c, always))
}

#[derive(Default)]
struct Start {
    path: String,
    name: String,
    pos: [f32; 3],
    rot: [f32; 9],
    unknown: String,
}

#[derive(Default)]
struct Level {
    min: [f32; 3],
    max: [f32; 3],
    grid: [u16; 2],
}

#[derive(Default)]
pub struct LevelMap {
    name: String,
    unknown: u32,
    unknown2: u32,
    starts: Vec<Start>,
    levels: Vec<Level>,
    cells: Vec<Cell>,
    list2: Vec<Cell>,
    list3: Vec<Cell>,
}

impl LevelMap {
    fn walk<S: Io>(&mut self, s: &mut S) -> Res<()> {
        s.magic(b"6PAM")?;
        s.text("Name", &mut self.name, Str::Z)?;
        let mut n = self.cells.len() as u32;
        s.hide(&mut n)?;
        s.u32("Unknown", &mut self.unknown)?;
        let mut k = self.starts.len() as u32;
        s.hide(&mut k)?;
        let has2 = if s.bin() { k > 0 } else { !s.skip("Unknown2", k == 0) };
        if has2 {
            s.u32("Unknown2", &mut self.unknown2)?;
        }
        s.note("Starts: start points (path, name, Position, Rotation 3x3)");
        let k = s.n(k);
        s.list("Starts", &mut self.starts, k, |s, x| {
            s.text("Path", &mut x.path, Str::L32Z)?;
            s.text("Name", &mut x.name, Str::L32Z)?;
            s.vec("Position", &mut x.pos)?;
            s.vec("Rotation", &mut x.rot)?;
            if !s.skip("Unknown", x.unknown.is_empty()) {
                s.text("Unknown", &mut x.unknown, Str::L32Z)?;
            }
            Ok(())
        })?;
        s.note("Levels: 3 detail levels of the world grid (bounds and Grid = cells across, down)");
        if s.bin() {
            if s.mode() == Mode::Read {
                self.levels.resize_with(3, Default::default);
            }
            for l in &mut self.levels {
                s.vec("", &mut l.min)?;
                s.vec("", &mut l.max)?;
            }
            for l in &mut self.levels {
                s.val("", &mut l.grid, Show::Dec)?;
            }
        } else {
            s.list("Levels", &mut self.levels, Len::Rest, |s, l| {
                s.vec("Min", &mut l.min)?;
                s.vec("Max", &mut l.max)?;
                s.val("Grid", &mut l.grid, Show::Dec)
            })?;
        }
        s.note("Cells: world blocks; Level 0 and 1 only mark the grid of that level, Level 2 has the block data (Counts = records per kind in its pack)\nList2 / List3: blocks loaded separately (by their names: interiors and cinematics); Id = hash of Name unless shown");
        let n = s.n(n);
        cells(s, "Cells", &mut self.cells, n, false)?;
        cells(s, "List2", &mut self.list2, Len::U32, true)?;
        cells(s, "List3", &mut self.list3, Len::U32, true)
    }

    fn check(&self) -> Res<()> {
        match self.levels.len() {
            3 => Ok(()),
            n => err(format!("Levels must have 3 entries, found {n}")),
        }
    }
}

impl Data for LevelMap {
    const ID: &'static str = "levelmap";
    const ABOUT: &'static str = "level map: world grid, start points and world blocks";
    const EXT: &'static [&'static str] = &["map"];
    fn io<S: Io>(&mut self, s: &mut S) -> Res<()> {
        self.walk(s)
    }
    fn fix(&mut self) -> Res<()> {
        self.check()
    }
    fn size(&self) -> String {
        format!("{} starts, {} cells, {} + {} blocks", self.starts.len(), self.cells.len(), self.list2.len(), self.list3.len())
    }
    fn endian(d: &[u8]) -> Option<Endian> {
        endian(d)
    }
}

#[derive(Default)]
pub struct DlcMap {
    name: String,
    unknown: u32,
    cells: Vec<Cell>,
    list2: Vec<Cell>,
    list3: Vec<Cell>,
}

impl DlcMap {
    fn walk<S: Io>(&mut self, s: &mut S) -> Res<()> {
        s.magic(b"6PAM")?;
        s.text("Name", &mut self.name, Str::Z)?;
        s.u32("Unknown", &mut self.unknown)?;
        s.note("Cells: world blocks; Level 0 and 1 only mark the grid of that level, Level 2 has the block data (Counts = records per kind in its pack)\nList2 / List3: blocks loaded separately (by their names: interiors and cinematics); Id = hash of Name unless shown");
        cells(s, "Cells", &mut self.cells, Len::U32, false)?;
        cells(s, "List2", &mut self.list2, Len::U32, true)?;
        cells(s, "List3", &mut self.list3, Len::U32, true)
    }
}

impl Data for DlcMap {
    const ID: &'static str = "dlcmap";
    const ABOUT: &'static str = "level map additions from a DLC (world blocks)";
    const EXT: &'static [&'static str] = &["map"];
    fn io<S: Io>(&mut self, s: &mut S) -> Res<()> {
        self.walk(s)
    }
    fn size(&self) -> String {
        format!("{} cells, {} + {} blocks", self.cells.len(), self.list2.len(), self.list3.len())
    }
    fn endian(d: &[u8]) -> Option<Endian> {
        endian(d)
    }
}

#[derive(Default)]
pub struct GlobalMap {
    lists: [Vec<Cell>; 3],
}

impl GlobalMap {
    fn walk<S: Io>(&mut self, s: &mut S) -> Res<()> {
        s.magic(b"6PAM")?;
        let mut n = self.lists[2].len() as u32;
        s.hide(&mut n)?;
        s.note("Global blocks in three lists (List1 and List2 hold the same names); Id = hash of Name unless shown");
        let [a, b, c] = &mut self.lists;
        cells(s, "List1", a, Len::U32, true)?;
        cells(s, "List2", b, Len::U32, true)?;
        let n = s.n(n);
        cells(s, "List3", c, n, true)
    }
}

impl Data for GlobalMap {
    const ID: &'static str = "globalmap";
    const ABOUT: &'static str = "global map: blocks shared by all levels";
    const EXT: &'static [&'static str] = &["map"];
    fn io<S: Io>(&mut self, s: &mut S) -> Res<()> {
        self.walk(s)
    }
    fn size(&self) -> String {
        format!("{} + {} + {} blocks", self.lists[0].len(), self.lists[1].len(), self.lists[2].len())
    }
    fn endian(d: &[u8]) -> Option<Endian> {
        endian(d)
    }
}
