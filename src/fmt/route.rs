use super::world::data;
use super::Data;
use crate::io::{err, Io, Len, Mode, Show, Str};
use ac_core::Res;
use std::collections::HashMap;

#[derive(Default)]
pub struct Paths {
    paths: Vec<Path>,
}

#[derive(Default)]
struct Path {
    name: u32,
    nodes: Vec<PathNode>,
    unused: Vec<u32>,
    listed: u16,
}

#[derive(Default)]
struct PathNode {
    pos: [f32; 3],
    rot: f32,
    unused: [f32; 2],
    pause: f32,
}

impl Paths {
    fn walk<S: Io>(&mut self, s: &mut S) -> Res<()> {
        s.magic(b"6HTP")?;
        s.note("Listed: the path is registered in the active path list\nRotation: heading in radians, Pause: minimum pause at the node in seconds");
        s.list("Paths", &mut self.paths, Len::U32, |s, p| {
            s.hash("Name", &mut p.name)?;
            s.list("Nodes", &mut p.nodes, Len::U32, |s, n| {
                s.vec("Position", &mut n.pos)?;
                s.f32("Rotation", &mut n.rot)?;
                s.opt("Unused", &mut n.unused, Show::Dec)?;
                s.f32("Pause", &mut n.pause)
            })?;
            if s.has("Unused") || !p.unused.is_empty() || s.bin() {
                s.hashes("Unused", &mut p.unused, Len::U32)?;
            }
            s.flag("Listed", &mut p.listed)
        })
    }
}

data!(Paths, "paths", "paths", "AI walking paths", |x| format!("{} paths", x.paths.len()));

#[derive(Default)]
pub struct Railway {
    lines: Vec<Line>,
}

#[derive(Default)]
struct Line {
    name: String,
    hash: u32,
    length: f32,
    nodes: Vec<Stop>,
}

#[derive(Default)]
struct Stop {
    name: String,
    hash: u32,
    start: f32,
    dist: f32,
    pos: [f32; 3],
    influence: [f32; 3],
    rot: [f32; 4],
    first: u16,
    last: u16,
    station: u16,
    speed: f32,
    wait: f32,
    trains: u32,
    links: Vec<[u32; 2]>,
}

impl Railway {
    fn walk<S: Io>(&mut self, s: &mut S) -> Res<()> {
        s.magic(b"6IAR")?;
        s.list("Lines", &mut self.lines, Len::U32, |s, l| {
            s.text("Name", &mut l.name, Str::L32Z)?;
            s.named("Hash", &mut l.hash, &l.name)?;
            s.f32("Length", &mut l.length)?;
            s.list("Nodes", &mut l.nodes, Len::U16, |s, n| {
                s.text("Name", &mut n.name, Str::L32Z)?;
                s.named("Hash", &mut n.hash, &n.name)?;
                s.f32("Start", &mut n.start)?;
                s.f32("Distance", &mut n.dist)?;
                s.vec("Position", &mut n.pos)?;
                s.vec("Influence", &mut n.influence)?;
                s.vec("Rotation", &mut n.rot)?;
                s.opt("First", &mut n.first, Show::Bool)?;
                s.opt("Last", &mut n.last, Show::Bool)?;
                s.opt("Station", &mut n.station, Show::Bool)?;
                s.f32("MaxSpeed", &mut n.speed)?;
                s.opt("StationWait", &mut n.wait, Show::Dec)?;
                s.opt("Trains", &mut n.trains, Show::Hash)?;
                s.list("Links", &mut n.links, Len::U32, |s, a| s.val("", a, Show::Hash))
            })
        })
    }
}

data!(Railway, "railway", "railway", "train lines", |x| format!("{} lines", x.lines.len()));

#[derive(Default)]
pub struct CinSplines {
    splines: Vec<Spline>,
}

#[derive(Default)]
struct Spline {
    name: u32,
    length: f32,
    time: f32,
    parent: u32,
    object: u32,
    flags: [u16; 4],
    nodes: Vec<Knot>,
}

#[derive(Default)]
struct Knot {
    dist: f32,
    seg: f32,
    speed: f32,
    time: f32,
    seg_time: f32,
    pos: [f32; 3],
    rot: [f32; 4],
    curve: [[f32; 3]; 3],
    end: [f32; 3],
    table: Vec<f32>,
}

impl CinSplines {
    fn walk<S: Io>(&mut self, s: &mut S) -> Res<()> {
        s.magic(b"4LPS")?;
        s.note("Curve: cubic segment coefficients, Table: arc-length lookup (0 to 1)");
        s.list("Splines", &mut self.splines, Len::U32, |s, p| {
            s.hash("Name", &mut p.name)?;
            s.f32("Length", &mut p.length)?;
            s.f32("Duration", &mut p.time)?;
            s.hash("Parent", &mut p.parent)?;
            s.hash("Object", &mut p.object)?;
            s.opt("Flags", &mut p.flags, Show::Dec)?;
            s.list("Nodes", &mut p.nodes, Len::U16, |s, n| {
                s.f32("Distance", &mut n.dist)?;
                s.f32("SegmentLength", &mut n.seg)?;
                s.f32("Speed", &mut n.speed)?;
                s.f32("Time", &mut n.time)?;
                s.f32("SegmentTime", &mut n.seg_time)?;
                s.vec("Position", &mut n.pos)?;
                s.vec("Rotation", &mut n.rot)?;
                s.val("Curve", &mut n.curve, Show::Dec)?;
                let p = n.pos;
                s.or("End", &mut n.end, Show::Dec, p)?;
                s.floats("Table", &mut n.table, Len::U16)
            })
        })
    }
}

data!(CinSplines, "cinsplines", "cinsplines", "cinematic camera and object splines", |x| format!("{} splines", x.splines.len()));

#[derive(Default)]
pub struct GpsGraph {
    nodes: Vec<Gps>,
    zones: Vec<(String, Vec<i32>)>,
    grid: [i16; 6],
    cells: Vec<Vec<i32>>,
    links: u16,
    tail: Vec<u8>,
}

#[derive(Default)]
struct Gps {
    id: i32,
    x: i16,
    y: i16,
    links: Vec<i32>,
}

fn ids<S: Io>(s: &mut S, k: &str, v: &mut Vec<i32>, n: Len) -> Res<()> {
    s.list(k, v, n, |s, x| {
        let mut y = *x as i16;
        s.val("", &mut y, Show::Dec)?;
        *x = y as i32;
        Ok(())
    })
}

impl GpsGraph {
    fn walk<S: Io>(&mut self, s: &mut S) -> Res<()> {
        s.magic(b"1SPG")?;
        let mut counts = [self.nodes.len() as u16, self.links, self.zones.len() as u16];
        for c in &mut counts {
            s.hide(c)?;
        }
        self.links = counts[1];
        s.note("Id: a label used by Links, Zones and Grid; ids may have gaps, packing renumbers them\nCells: a node id per grid cell, row by row, -1 = none\nTail: unused bytes at the end of the file");
        let n = s.n(counts[0] as u32);
        s.list("Nodes", &mut self.nodes, n, |s, g| {
            if !s.bin() {
                s.i32("Id", &mut g.id)?;
            }
            s.i16("X", &mut g.x)?;
            s.i16("Y", &mut g.y)?;
            ids(s, "Links", &mut g.links, Len::U8)
        })?;
        let n = s.n(counts[2] as u32);
        s.list("Zones", &mut self.zones, n, |s, z| {
            s.text("Name", &mut z.0, Str::Z)?;
            ids(s, "Nodes", &mut z.1, Len::U16)
        })?;
        let g = &mut self.grid;
        s.node("Grid", |s| ["Width", "Height", "OriginX", "OriginY", "CellWidth", "CellHeight"].iter().zip(g.iter_mut()).try_for_each(|(k, v)| s.i16(k, v)))?;
        let (w, h) = (self.grid[0].max(0) as u32, self.grid[1].max(0) as u32);
        if s.bin() {
            let mut flat: Vec<i32> = self.cells.concat();
            let n = s.n(w * h);
            ids(s, "", &mut flat, n)?;
            if w > 0 {
                self.cells = flat.chunks(w as usize).map(<[i32]>::to_vec).collect();
            }
        } else {
            s.list("Cells", &mut self.cells, Len::Rest, |s, r| ids(s, "", r, Len::Rest))?;
        }
        s.tail("Tail", &mut self.tail)?;
        if s.mode() == Mode::Read {
            for (i, g) in self.nodes.iter_mut().enumerate() {
                g.id = i as i32;
            }
            let none = self.nodes.len() as i32;
            self.cells.iter_mut().flatten().filter(|c| **c == none).for_each(|c| *c = -1);
        }
        Ok(())
    }

    fn renumber(&mut self) -> Res<()> {
        let map: HashMap<i32, i32> = self.nodes.iter().enumerate().map(|(i, g)| (g.id, i as i32)).collect();
        if map.len() != self.nodes.len() {
            return err("node ids must be unique");
        }
        let get = |x: &mut i32, w: &str| -> Res<()> {
            *x = *map.get(x).ok_or_else(|| ac_core::Error::Msg(format!("{w} uses node id {x}, which does not exist")))?;
            Ok(())
        };
        for g in &mut self.nodes {
            g.links.iter_mut().try_for_each(|x| get(x, "Links"))?;
        }
        for z in &mut self.zones {
            z.1.iter_mut().try_for_each(|x| get(x, &format!("zone {}", z.0)))?;
        }
        let none = self.nodes.len() as i32;
        for c in self.cells.iter_mut().flatten() {
            if *c < 0 {
                *c = none;
            } else {
                get(c, "Cells")?;
            }
        }
        let (w, h) = (self.grid[0].max(0) as usize, self.grid[1].max(0) as usize);
        if self.cells.len() != h || self.cells.iter().any(|r| r.len() != w) {
            return err(format!("Cells must be {h} rows of {w} ids (Grid width and height)"));
        }
        self.links = self.nodes.iter().map(|g| g.links.len() as u16).sum();
        Ok(())
    }
}

impl Data for GpsGraph {
    const ID: &'static str = "gpsgraph";
    const ABOUT: &'static str = "road graph for the map GPS";
    const EXT: &'static [&'static str] = &["gpsgraph"];
    fn io<S: Io>(&mut self, s: &mut S) -> Res<()> {
        self.walk(s)
    }
    fn fix(&mut self) -> Res<()> {
        self.renumber()
    }
    fn size(&self) -> String {
        format!("{} nodes, {} zones", self.nodes.len(), self.zones.len())
    }
    fn endian(d: &[u8]) -> Option<ac_core::Endian> {
        Some(if d.starts_with(b"GPS1") { ac_core::Endian::Be } else { ac_core::Endian::Le })
    }
}
