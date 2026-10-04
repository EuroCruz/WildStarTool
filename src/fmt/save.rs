use super::Data;
use crate::io::{err, float, unfloat, Io, Len, Mode, Read, Show, Str, Write};
use ac_core::{Endian, Res};
use ac_lua::Val;

const SIZE: usize = 0x26ff8;
const STREAM: usize = 0x858;
const BUFFER: usize = 0x25800;
const FILL: u8 = 0x55;
const TABLE: u32 = 0xc0de_c0de;
const END: u32 = 0xdead_f00d;
const PERKS: &[u8; 30] = b"AAAAABAAAACCACAAAAAABAAAAAABAA";
const PERK_NAMES: [&str; 30] = [
    "Perks_01Brawling_1FightinIri",
    "Perks_01Brawling_2SuckerPunc",
    "Perks_01Brawling_3GrimReaper",
    "Perks_02Hardware_1Gunslinger",
    "Perks_02Hardware_2ExplosiveT",
    "Perks_02Hardware_3PaintTheTo",
    "Perks_03Sniping_1Marksman",
    "Perks_03Sniping_2EagleEye",
    "Perks_03Sniping_3LongShot",
    "Perks_04Explosives_1CheapThr",
    "Perks_04Explosives_2MoreBang",
    "Perks_04Explosives_3ARealHel",
    "Perks_05Demolitions_1ANastyS",
    "Perks_05Demolitions_2MadBomb",
    "Perks_05Demolitions_3Blockbu",
    "Perks_06Sabotage_1ShortFuse",
    "Perks_06Sabotage_2Structural",
    "Perks_06Sabotage_3UrbanRenew",
    "Perks_07Mayhem_1YouTosser",
    "Perks_07Mayhem_2RoadRage",
    "Perks_07Mayhem_3TheRightOfWa",
    "Perks_08Racing_1LeadFoot",
    "Perks_08Racing_2SpeedDemon",
    "Perks_08Racing_3GodSpeed",
    "Perks_09Mechanics_1JoyRider",
    "Perks_09Mechanics_2Hijacker",
    "Perks_09Mechanics_3Wheelman",
    "Perks_10Evasion_1Fugitive",
    "Perks_10Evasion_2EscapeArtis",
    "Perks_10Evasion_3EuropesMost",
];
const STATS: [(&str, u8); 101] = [
    ("WehrmachtKilled", b'u'),
    ("KriegsmarineKilled", b'u'),
    ("OtherNazisKilled", b'u'),
    ("GestapoKilled", b'u'),
    ("SSKilled", b'u'),
    ("TerrorSquadKilled", b'u'),
    ("DoppelziegCrewKilled", b'u'),
    ("Stat34", b'u'),
    ("MostNazisKilledInOneLife", b'u'),
    ("Stat3c", b'u'),
    ("MostNazisKilledAtOnce", b'u'),
    ("MostNazisKilledWhileDriving", b'u'),
    ("NazisKilledBySurprise", b'u'),
    ("Stat4c", b'f'),
    ("BirdsKilled", b'u'),
    ("CiviliansKilledByPlayer", b'u'),
    ("CiviliansKilledByNazis", b'u'),
    ("ResistanceKilled", b'u'),
    ("CigarettesSmoked", b'u'),
    ("BombsPlanted", b'u'),
    ("CiviliansSaved", b'u'),
    ("FlamethrowerFuelSpent", b'u'),
    ("Flamethrower70", b'u'),
    ("Flamethrower74", b'u'),
    ("FlamethrowerKills", b'u'),
    ("GrenadesThrown", b'u'),
    ("Grenade80", b'u'),
    ("Grenade84", b'u'),
    ("GrenadeKills", b'u'),
    ("MachineGunFired", b'u'),
    ("MachineGunHits", b'u'),
    ("MachineGunHeadshots", b'u'),
    ("MachineGunKills", b'u'),
    ("PistolFired", b'u'),
    ("PistolHits", b'u'),
    ("PistolHeadshots", b'u'),
    ("PistolKills", b'u'),
    ("RifleFired", b'u'),
    ("RifleHits", b'u'),
    ("RifleHeadshots", b'u'),
    ("RifleKills", b'u'),
    ("RocketsLaunched", b'u'),
    ("RocketHits", b'u'),
    ("RocketHeadshots", b'u'),
    ("RocketKills", b'u'),
    ("ShotgunShellsFired", b'u'),
    ("ShotgunPelletsHit", b'u'),
    ("ShotgunHeadshots", b'u'),
    ("ShotgunKills", b'u'),
    ("TimesOneBulletHitMultipleHumans", b'u'),
    ("MaxHumansHitWithOneBullet", b'b'),
    ("ShotgunPelletsFired", b'u'),
    ("BombVehiclesDestroyed", b'u'),
    ("BombKills", b'u'),
    ("RocketVehiclesDestroyed", b'u'),
    ("PunchesKicksThrown", b'u'),
    ("PunchesKicksHit", b'u'),
    ("MeleeKills", b'u'),
    ("SuckerPunchKills", b'u'),
    ("Grabs", b'u'),
    ("NazisThrownFromAHeight", b'u'),
    ("Stat108", b'u'),
    ("CarsDestroyed", b'u'),
    ("TrucksDestroyed", b'u'),
    ("APCsDestroyed", b'u'),
    ("TanksDestroyed", b'u'),
    ("PlanesDestroyed", b'u'),
    ("ZeppelinsDestroyed", b'u'),
    ("FuelBarrelsDestroyed", b'u'),
    ("CratesDestroyed", b'u'),
    ("CheckpointsPassed", b'u'),
    ("Stat134", b'u'),
    ("CarsWreckedWhileDriving", b'u'),
    ("Bailouts", b'u'),
    ("OnFoot", b'f'),
    ("Swimming", b'f'),
    ("Climbed", b'f'),
    ("Falling", b'f'),
    ("ByZipline", b'f'),
    ("ByVehicle", b'f'),
    ("VehicleJumpedDistance", b'f'),
    ("VehicleDriftedDistance", b'f'),
    ("InATank", b'f'),
    ("Stat164", b'u'),
    ("Stat168", b'u'),
    ("HighestEscalationEscaped", b'b'),
    ("TimesEscalated", b'u'),
    ("TimesDeEscalated", b'u'),
    ("DeEscalationsByKissing", b'u'),
    ("DeEscalationsByAlarmDeactivation", b'u'),
    ("DeEscalationsByTrapDoor", b'u'),
    ("DeEscalationsByShantyHiding", b'u'),
    ("DeEscalationsByBathroom", b'u'),
    ("DeEscalationsByFightback", b'u'),
    ("DeEscalationsByBrothel", b'u'),
    ("TotalGameTime", b'f'),
    ("MissionProgress", b'u'),
    ("Stat19c", b'u'),
    ("MissionProgressMax", b'u'),
    ("Stat1a4", b'u'),
    ("Stat15dc", b'u'),
];
const LUA: [(&str, bool); 15] = [
    ("SaveVersion", false),
    ("OpenMissions", true),
    ("HiddenStarters", true),
    ("CompletedMissions", true),
    ("WorldNodes", true),
    ("WorldCinematicNodes", true),
    ("WorldStaticTags", true),
    ("DisabledMissions", true),
    ("ActiveArc", false),
    ("PotentialMissions", true),
    ("ActiveMissions", true),
    ("SpecialCaseUnlocked", true),
    ("HQPoints", true),
    ("Misc", true),
    ("Interior", false),
];

#[derive(Clone)]
enum Key {
    S(String),
    N(u32),
}

#[derive(Clone)]
enum Lv {
    S(String),
    F(f32),
    B(u8),
    U(u32),
    T(Vec<(Key, Lv)>),
}

impl Default for Lv {
    fn default() -> Self {
        Lv::T(Vec::new())
    }
}

fn lua_read<S: Io>(s: &mut S) -> Res<Vec<(Key, Lv)>> {
    let (mut m, mut n) = (0u32, 0u32);
    s.hide(&mut m)?;
    if m != TABLE {
        return err(format!("expected a table (0x{TABLE:08x}), found 0x{m:08x}"));
    }
    s.hide(&mut n)?;
    let mut t = Vec::new();
    for _ in 0..n {
        let (mut ty, mut kk) = (0u32, 0u8);
        s.hide(&mut ty)?;
        s.hide(&mut kk)?;
        let k = if kk == 0 {
            let mut x = String::new();
            s.text("", &mut x, Str::Z)?;
            Key::S(x)
        } else {
            let mut x = 0u32;
            s.hide(&mut x)?;
            Key::N(x)
        };
        let v = match ty {
            0 => {
                let mut x = String::new();
                s.text("", &mut x, Str::Z)?;
                Lv::S(x)
            }
            1 => {
                let mut x = 0f32;
                s.hide(&mut x)?;
                Lv::F(x)
            }
            2 => {
                let mut x = 0u8;
                s.hide(&mut x)?;
                Lv::B(x)
            }
            3 => {
                let mut x = 0u32;
                s.hide(&mut x)?;
                Lv::U(x)
            }
            4 => Lv::T(lua_read(s)?),
            t => return err(format!("unknown Lua value type {t}")),
        };
        t.push((k, v));
    }
    s.hide(&mut m)?;
    if m != END {
        return err(format!("expected the end of a table (0x{END:08x}), found 0x{m:08x}"));
    }
    Ok(t)
}

fn lua_write<S: Io>(s: &mut S, t: &[(Key, Lv)]) -> Res<()> {
    s.hide(&mut TABLE.clone())?;
    s.hide(&mut (t.len() as u32))?;
    for (k, v) in t {
        let ty: u32 = match v {
            Lv::S(_) => 0,
            Lv::F(_) => 1,
            Lv::B(_) => 2,
            Lv::U(_) => 3,
            Lv::T(_) => 4,
        };
        s.hide(&mut ty.clone())?;
        match k {
            Key::S(x) => {
                s.hide(&mut 0u8)?;
                s.text("", &mut x.clone(), Str::Z)?;
            }
            Key::N(x) => {
                s.hide(&mut 1u8)?;
                s.hide(&mut x.clone())?;
            }
        }
        match v {
            Lv::S(x) => s.text("", &mut x.clone(), Str::Z)?,
            Lv::F(x) => s.hide(&mut x.clone())?,
            Lv::B(x) => s.hide(&mut x.clone())?,
            Lv::U(x) => s.hide(&mut x.clone())?,
            Lv::T(x) => lua_write(s, x)?,
        }
    }
    s.hide(&mut END.clone())
}

fn digits(k: &str) -> bool {
    !k.is_empty() && k.bytes().all(|c| c.is_ascii_digit())
}

fn to_val(t: &[(Key, Lv)]) -> Val {
    let mut pos = 0u32;
    Val::Tbl(
        t.iter()
            .map(|(k, v)| {
                let k = match k {
                    Key::N(n) if *n == pos + 1 => {
                        pos += 1;
                        None
                    }
                    Key::N(n) => Some(n.to_string()),
                    Key::S(s) if digits(s) => Some(format!("\"{s}\"")),
                    Key::S(s) => Some(s.clone()),
                };
                let v = match v {
                    Lv::S(x) => Val::Str(x.clone()),
                    Lv::F(x) => float(*x),
                    Lv::B(0) => Val::Bool(false),
                    Lv::B(1) => Val::Bool(true),
                    Lv::B(x) => Val::Call("byte".into(), vec![Val::Int(*x as i64)]),
                    Lv::U(x) => Val::Int(*x as i64),
                    Lv::T(x) => to_val(x),
                };
                (k, v)
            })
            .collect(),
    )
}

fn from_val(v: &Val, at: &str) -> Res<Vec<(Key, Lv)>> {
    let Val::Tbl(t) = v else { return err(format!("{at}: expected a table {{ ... }}")) };
    let mut pos = 0u32;
    let mut out = Vec::new();
    for (k, x) in t {
        if matches!(x, Val::Note(_)) {
            continue;
        }
        let key = match k {
            None => {
                pos += 1;
                Key::N(pos)
            }
            Some(k) if digits(k) => Key::N(k.parse().map_err(|_| ac_core::Error::Msg(format!("{at}: key {k} is too large")))?),
            Some(k) => Key::S(k.strip_prefix('"').and_then(|k| k.strip_suffix('"')).unwrap_or(k).to_string()),
        };
        let here = || format!("{at}.{}", k.clone().unwrap_or_else(|| format!("[{pos}]")));
        let val = match x {
            Val::Str(s) => Lv::S(s.clone()),
            Val::Bool(b) => Lv::B(*b as u8),
            Val::Int(i) => Lv::U(u32::try_from(*i).map_err(|_| ac_core::Error::Msg(format!("{}: {i} does not fit 0..4294967295 (write it as {i}.0 for a fractional number)", here())))?),
            Val::Call(n, a) if n == "byte" => Lv::B(a.first().and_then(Val::int).and_then(|i| u8::try_from(i).ok()).ok_or_else(|| ac_core::Error::Msg(format!("{}: byte(0..255)", here())))?),
            Val::Tbl(_) => Lv::T(from_val(x, &here())?),
            v => Lv::F(unfloat(v).ok_or_else(|| ac_core::Error::Msg(format!("{}: expected text, a number, true/false or a table", here())))?),
        };
        out.push((key, val));
    }
    Ok(out)
}

fn lua<S: Io>(s: &mut S, k: &str, t: &mut Vec<(Key, Lv)>) -> Res<()> {
    match s.mode() {
        Mode::Read => *t = lua_read(s)?,
        Mode::Write => lua_write(s, t)?,
        Mode::Dump => s.any(k, &mut to_val(t))?,
        Mode::Load => {
            let mut v = Val::Nil;
            s.any(k, &mut v)?;
            *t = from_val(&v, k)?;
        }
    }
    Ok(())
}

#[derive(Default)]
struct Region {
    hashes: [u32; 4],
    pos: [f32; 4],
    rot: [f32; 4],
    colors: [u32; 4],
    bytes: Vec<u32>,
    flags: [u8; 4],
}

fn region<S: Io>(s: &mut S, r: &mut Region) -> Res<()> {
    s.val("Hashes", &mut r.hashes, Show::Hash)?;
    s.vec("Position", &mut r.pos)?;
    s.vec("Rotation", &mut r.rot)?;
    s.val("Colors", &mut r.colors, Show::Hex)?;
    s.list("Bytes", &mut r.bytes, Len::U32, |s, x| s.u32("", x))?;
    s.val("Flags", &mut r.flags, Show::Dec)
}

#[derive(Default)]
struct RegionSet {
    id: u32,
    items: Vec<(u32, Region)>,
}

#[derive(Default)]
struct Marker {
    kind: u8,
    a: u32,
    b: f32,
    matrix: [f32; 16],
}

#[derive(Default)]
struct Garage {
    garage: u32,
    parked: u8,
    value: u32,
    vehicle: u32,
}

#[derive(Default)]
struct Player {
    hq: u32,
    mission: u32,
    number: u32,
    matrix: [f32; 16],
    matrix2: [f32; 16],
    hash: u32,
    value: u32,
    hashes: Vec<u32>,
    bytes: Vec<u8>,
    extra: u32,
}

#[derive(Default)]
struct Weapon {
    slot: u32,
    weapon: u32,
    instance: u32,
    value: i32,
}

#[derive(Default)]
struct WorldObject {
    object: u32,
    bytes: [u8; 3],
    pos: [f32; 4],
    values: [u32; 2],
    flags: [u8; 2],
}

#[derive(Default)]
struct Point {
    pos: [f32; 3],
    hash: u32,
    value: u32,
    flag: u8,
}

#[derive(Default)]
struct Perk {
    progress: u32,
    value: u32,
    unlocked: u8,
}

#[derive(Default)]
struct Paper {
    value: i32,
    paper: u32,
    matrix: [f32; 16],
}

#[derive(Default)]
struct Options {
    b4: u8,
    invert_y: u8,
    difficulty: u8,
    b7: u8,
    sens: [f32; 2],
    vibration: u8,
    tutorials: u8,
    nudity: u8,
    autosave: u8,
    b14: u8,
    subtitles: u8,
    b16: u8,
    volumes: [u8; 3],
    last: [u16; 32],
    b5a: [u8; 2],
    u5c: u32,
    f60: f32,
    r64: Vec<u8>,
    preorder: u8,
    b1a5: u8,
    b1a6: [u8; 2],
    u1a8: [u32; 2],
    r1b0: Vec<u8>,
    sniper: f32,
    eq: u8,
    r205: Vec<u8>,
}

fn wide<S: Io>(s: &mut S, k: &str, v: &mut [u16; 32]) -> Res<()> {
    if s.bin() {
        return s.hide(v);
    }
    let n = v.iter().position(|&c| c == 0).unwrap_or(32);
    let mut t = String::from_utf16_lossy(&v[..n]);
    s.text(k, &mut t, Str::Z)?;
    if s.reading() {
        let u: Vec<u16> = t.encode_utf16().collect();
        if u.len() > 31 {
            return err(format!("{k}: at most 31 characters"));
        }
        *v = [0; 32];
        v[..u.len()].copy_from_slice(&u);
    }
    Ok(())
}

fn zeros<S: Io>(s: &mut S, k: &str, v: &mut Vec<u8>, n: usize) -> Res<()> {
    if s.skip(k, v.iter().all(|&b| b == 0)) {
        *v = vec![0; n];
        return if s.bin() { s.raw(k, v, Len::N(n)) } else { Ok(()) };
    }
    s.raw(k, v, Len::N(n))
}

impl Options {
    fn walk<S: Io>(&mut self, s: &mut S) -> Res<()> {
        s.konst(0x30u32)?;
        s.opt("Unknown4", &mut self.b4, Show::Dec)?;
        s.flag("InvertY", &mut self.invert_y)?;
        s.u8("Difficulty", &mut self.difficulty)?;
        s.u8("Unknown7", &mut self.b7)?;
        s.f32("XSensitivity", &mut self.sens[0])?;
        s.f32("YSensitivity", &mut self.sens[1])?;
        s.flag("Vibration", &mut self.vibration)?;
        s.flag("Tutorials", &mut self.tutorials)?;
        s.flag("Nudity", &mut self.nudity)?;
        s.flag("Autosave", &mut self.autosave)?;
        s.u8("Unknown14", &mut self.b14)?;
        s.u8("Subtitles", &mut self.subtitles)?;
        s.u8("Unknown16", &mut self.b16)?;
        s.u8("EffectsVolume", &mut self.volumes[0])?;
        s.u8("MusicVolume", &mut self.volumes[1])?;
        s.u8("VoiceVolume", &mut self.volumes[2])?;
        wide(s, "LastSave", &mut self.last)?;
        s.opt("Unknown5a", &mut self.b5a, Show::Dec)?;
        s.opt("Unknown5c", &mut self.u5c, Show::Hex)?;
        s.f32("Unknown60", &mut self.f60)?;
        zeros(s, "Unknown64", &mut self.r64, 0x140)?;
        s.flag("PreorderBonus", &mut self.preorder)?;
        s.opt("Unknown1a5", &mut self.b1a5, Show::Dec)?;
        s.opt("Unknown1a6", &mut self.b1a6, Show::Dec)?;
        s.opt("Unknown1a8", &mut self.u1a8, Show::Hex)?;
        zeros(s, "Unknown1b0", &mut self.r1b0, 0x50)?;
        s.f32("SniperSensitivity", &mut self.sniper)?;
        s.u8("Equalizer", &mut self.eq)?;
        zeros(s, "Unknown205", &mut self.r205, 0x1fb)
    }
}

#[derive(Default)]
pub struct Save {
    version: u32,
    head: [u32; 2],
    time: u64,
    mission: u32,
    b24: u8,
    autosave: u8,
    number: u8,
    options: Options,
    hq: u32,
    clock: (f32, u32),
    pairs: Vec<[u8; 2]>,
    v456: u32,
    markers: Vec<Marker>,
    g_first: u8,
    garages: Vec<Garage>,
    fsm: (f32, Vec<(u32, u32)>),
    region: Region,
    regions: Vec<RegionSet>,
    player: Player,
    weapons: Vec<Weapon>,
    ammo: Vec<(u32, u32)>,
    defen: Vec<u32>,
    zones: Vec<(u32, u8, u32)>,
    v98c: u32,
    v98c_items: Vec<([u32; 6], [f32; 4])>,
    strings: Vec<String>,
    tables: Vec<Vec<(Key, Lv)>>,
    nodes: Vec<String>,
    node_hashes: Vec<u32>,
    b96f: [u8; 2],
    objects: Vec<WorldObject>,
    points: Vec<Point>,
    data5a: Vec<u8>,
    flags5a: Vec<u8>,
    perk_head: [u8; 3],
    perks: Vec<Perk>,
    perk_tail: u8,
    contraband: [u32; 3],
    fp_groups: Vec<Vec<(u8, u32, u32)>>,
    fp_targets: Vec<(u32, u8, u8)>,
    fp_hashes: Vec<u32>,
    fp_flags: [u8; 2],
    e30_flags: [u8; 2],
    e30_hashes: Vec<u32>,
    e30_a: Vec<(u8, u32)>,
    e30_b: Vec<[u8; 2]>,
    e30_c: Vec<(u8, u32)>,
    e30_d: Vec<(u8, u32)>,
    e30_tail: u8,
    stats: Vec<u32>,
    stats_pairs: Vec<[u32; 2]>,
    stats_f: [u32; 2],
    stats_list: Vec<u32>,
    stats_lists: [Vec<[u32; 2]>; 4],
    a090: (u32, Vec<(u8, u32)>),
    papers: Vec<Paper>,
    map: (u32, u32, Vec<u8>),
    f55: Vec<u32>,
    e877: (u32, u32, Vec<u8>, Vec<u8>),
    maxima: [i32; 32],
    rest: Vec<u8>,
    slack: Vec<u8>,
}

fn crc(d: &[u8]) -> u32 {
    let mut c = !0x811c_9dc5u32;
    for &b in d {
        c ^= (b as u32) << 24;
        for _ in 0..8 {
            c = if c & 0x8000_0000 != 0 { (c << 1) ^ 0x04c1_1db7 } else { c << 1 };
        }
    }
    !c
}

fn date(t: u64) -> String {
    let (d, s) = ((t / 86400) as i64, t % 86400);
    let z = d + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + (month <= 2) as i64;
    format!("{year}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC", s / 3600, s / 60 % 60, s % 60)
}

fn pairs<S: Io>(s: &mut S, k: &str, v: &mut Vec<(u8, u32)>, n: Len) -> Res<()> {
    s.list(k, v, n, |s, x| {
        s.u8("Flag", &mut x.0)?;
        s.u32("Value", &mut x.1)
    })
}

fn counted<S: Io, T: Default>(s: &mut S, k: &str, v: &mut Vec<T>, f: impl FnMut(&mut S, &mut T) -> Res<()>) -> Res<()> {
    s.list(k, v, Len::U32, f)
}

impl Save {
    fn stop<S: Io>(&self, s: &S) -> bool {
        match s.mode() {
            Mode::Read => self.player.extra != 0,
            Mode::Load => s.has("Rest"),
            _ => !self.rest.is_empty(),
        }
    }

    fn walk<S: Io>(&mut self, s: &mut S) -> Res<()> {
        s.hide(&mut 0u32)?;
        s.konst(0x30u32)?;
        s.konst(SIZE as u32)?;
        s.opt("Version", &mut self.version, Show::Dec)?;
        s.opt("Unknown", &mut self.head, Show::Hex)?;
        if s.mode() == Mode::Dump {
            s.note(&format!("Saved: {} (seconds since 1970); the load menu shows the newest save first", date(self.time)));
        }
        s.val("Saved", &mut self.time, Show::Dec)?;
        s.hash("Mission", &mut self.mission)?;
        s.opt("Unknown24", &mut self.b24, Show::Dec)?;
        s.flag("Autosave", &mut self.autosave)?;
        s.u8("MenuNumber", &mut self.number)?;
        s.zero(0x31)?;
        s.note("Options: Settings menu values; Difficulty 0..3, Subtitles 0 off / 1 game only / 3 on, Equalizer 0 home theater / 1 other, volumes 0..100");
        let o = &mut self.options;
        s.node("Options", |s| o.walk(s))?;
        s.zero(0x400)?;
        s.magic(b"10VS")?;
        s.hide(&mut 0u32)?;
        s.hash("RespawnHQ", &mut self.hq)?;
        s.note("Block_*: raw save sections, named after the game function that loads them");
        s.node("Block_98d100", |s| {
            s.f32("Value", &mut self.clock.0)?;
            s.u32("Value2", &mut self.clock.1)
        })?;
        s.list("Block_a120d0", &mut self.pairs, Len::N(49), |s, x| s.val("", x, Show::Dec))?;
        s.u32("Block_456730", &mut self.v456)?;
        counted(s, "Block_8261b0", &mut self.markers, |s, m| {
            s.u8("Kind", &mut m.kind)?;
            if m.kind == 1 {
                s.u32("Value", &mut m.a)?;
                s.f32("Value2", &mut m.b)
            } else {
                s.vec("Matrix", &mut m.matrix)
            }
        })?;
        s.note("Garages: the garages and the vehicle parked in each (Parked = false: empty)");
        s.opt("GaragesUnknown", &mut self.g_first, Show::Dec)?;
        s.list("Garages", &mut self.garages, Len::U8, |s, g| {
            s.hash("Garage", &mut g.garage)?;
            s.flag("Parked", &mut g.parked)?;
            s.opt("Value", &mut g.value, Show::Hash)?;
            s.opt("Vehicle", &mut g.vehicle, Show::Hash)
        })?;
        s.node("Block_7f41c0", |s| {
            s.f32("Value", &mut self.fsm.0)?;
            counted(s, "Items", &mut self.fsm.1, |s, x| {
                s.hash("Hash", &mut x.0)?;
                s.u32("Value", &mut x.1)
            })
        })?;
        s.node("Block_9fe5a0", |s| {
            s.node("Head", |s| region(s, &mut self.region))?;
            counted(s, "Sets", &mut self.regions, |s, r| {
                let mut c = r.items.len() as u32;
                s.hide(&mut c)?;
                s.hash("Id", &mut r.id)?;
                let n = s.n(c);
                s.list("Items", &mut r.items, n, |s, x| {
                    s.hash("Hash", &mut x.0)?;
                    s.node("Region", |s| region(s, &mut x.1))
                })
            })
        })?;
        let p = &mut self.player;
        s.node("Player", |s| {
            s.hash("HQ", &mut p.hq)?;
            s.hash("Mission", &mut p.mission)?;
            s.u32("MenuNumber", &mut p.number)?;
            s.vec("Matrix", &mut p.matrix)?;
            s.vec("Matrix2", &mut p.matrix2)?;
            s.hash("Hash", &mut p.hash)?;
            s.u32("Value", &mut p.value)?;
            s.hashes("Hashes", &mut p.hashes, Len::U32)?;
            s.list("Bytes", &mut p.bytes, Len::U32, |s, x| s.u8("", x))?;
            s.opt("Extra", &mut p.extra, Show::Hex)
        })?;
        if self.stop(s) {
            s.note("Rest: remaining save data when Player.Extra is not 0, kept as is");
            s.raw("Rest", &mut self.rest, Len::Rest)?;
            return Ok(());
        }
        s.note("Weapons: { slot, weapon, instance, value }; Ammo: Kind and Amount");
        counted(s, "Weapons", &mut self.weapons, |s, w| {
            s.u32("Slot", &mut w.slot)?;
            s.hash("Weapon", &mut w.weapon)?;
            s.hash("Instance", &mut w.instance)?;
            s.i32("Value", &mut w.value)
        })?;
        counted(s, "Ammo", &mut self.ammo, |s, a| {
            s.hash("Kind", &mut a.0)?;
            s.u32("Amount", &mut a.1)
        })?;
        s.hashes("Defen", &mut self.defen, Len::U32)?;
        s.note("WillToFight: the Will to Fight zones (WtF_Zones) and their state");
        counted(s, "WillToFight", &mut self.zones, |s, z| {
            s.hash("Zone", &mut z.0)?;
            s.u8("Flag", &mut z.1)?;
            s.u32("Value", &mut z.2)
        })?;
        s.node("Block_98c560", |s| {
            s.u32("Value", &mut self.v98c)?;
            counted(s, "Items", &mut self.v98c_items, |s, x| {
                s.val("Values", &mut x.0, Show::Hex)?;
                s.vec("Position", &mut x.1)
            })
        })?;
        s.note("Missions: the Lua tables saved by SabTask.SaveGameCallback (scripts/Modules/SabTask.lua), with the same names");
        if self.strings.len() != 3 || self.tables.len() != 12 {
            self.strings.resize(3, String::new());
            self.tables.resize(12, Vec::new());
        }
        let (strings, tables) = (&mut self.strings, &mut self.tables);
        s.node("Missions", |s| {
            let (mut i, mut j) = (0, 0);
            for (k, table) in LUA {
                if table {
                    lua(s, k, &mut tables[j])?;
                    j += 1;
                } else {
                    s.text(k, &mut strings[i], Str::Z)?;
                    i += 1;
                }
            }
            Ok(())
        })?;
        s.node("LoadedNodes", |s| {
            s.list("Nodes", &mut self.nodes, Len::U32, |s, x| s.text("", x, Str::Z))?;
            s.hashes("Hashes", &mut self.node_hashes, Len::U32)
        })?;
        s.val("Block_96f220", &mut self.b96f, Show::Dec)?;
        s.note("WorldObjects: characters and targets of the world whose state is saved");
        counted(s, "WorldObjects", &mut self.objects, |s, o| {
            s.hash("Object", &mut o.object)?;
            s.val("Bytes", &mut o.bytes, Show::Dec)?;
            s.vec("Position", &mut o.pos)?;
            s.val("Values", &mut o.values, Show::Dec)?;
            s.val("Flags", &mut o.flags, Show::Dec)
        })?;
        counted(s, "Block_9e17c0", &mut self.points, |s, p| {
            s.vec("Position", &mut p.pos)?;
            s.hash("Hash", &mut p.hash)?;
            s.u32("Value", &mut p.value)?;
            s.u8("Flag", &mut p.flag)
        })?;
        s.node("Block_5a5300", |s| {
            let mut c = self.flags5a.len() as u16;
            s.hide(&mut c)?;
            s.raw("Data", &mut self.data5a, Len::N(1024))?;
            let n = s.n(c as u32);
            s.list("Flags", &mut self.flags5a, n, |s, x| s.u8("", x))
        })?;
        s.note("Perks: by GameTemplates name (category, level); Progress counts toward the perk");
        s.node("Perks", |s| {
            s.val("Head", &mut self.perk_head, Show::Dec)?;
            self.perks.resize_with(30, Perk::default);
            for (i, p) in self.perks.iter_mut().enumerate() {
                let k = PERKS[i];
                s.node(PERK_NAMES[i], |s| {
                    if k != b'C' {
                        s.u32("Progress", &mut p.progress)?;
                    }
                    if k == b'B' {
                        s.u32("Value", &mut p.value)?;
                    }
                    s.flag("Unlocked", &mut p.unlocked)
                })?;
            }
            s.u8("Tail", &mut self.perk_tail)
        })?;
        s.note("Contraband: Added is added to Current when the game loads (then limited to 0..Max)");
        let c = &mut self.contraband;
        s.node("Contraband", |s| {
            s.u32("Current", &mut c[0])?;
            s.u32("Max", &mut c[1])?;
            s.u32("Added", &mut c[2])
        })?;
        s.node("Freeplay", |s| {
            counted(s, "Groups", &mut self.fp_groups, |s, g| {
                s.list("", g, Len::N(16), |s, x| {
                    s.u8("Flag", &mut x.0)?;
                    s.u32("Value", &mut x.1)?;
                    s.u32("Value2", &mut x.2)
                })
            })?;
            counted(s, "Targets", &mut self.fp_targets, |s, x| {
                s.hash("Hash", &mut x.0)?;
                s.u8("Flag", &mut x.1)?;
                s.u8("Flag2", &mut x.2)
            })?;
            s.hashes("Hashes", &mut self.fp_hashes, Len::U32)?;
            s.val("Flags", &mut self.fp_flags, Show::Dec)
        })?;
        s.node("Block_9e3030", |s| {
            s.val("Flags", &mut self.e30_flags, Show::Dec)?;
            s.hashes("Hashes", &mut self.e30_hashes, Len::U32)?;
            pairs(s, "List", &mut self.e30_a, Len::U32)?;
            s.list("List2", &mut self.e30_b, Len::U32, |s, x| s.val("", x, Show::Dec))?;
            pairs(s, "List3", &mut self.e30_c, Len::N(24))?;
            pairs(s, "List4", &mut self.e30_d, Len::U32)?;
            s.u8("Tail", &mut self.e30_tail)
        })?;
        s.note("Stats: Stats screen values (distances in meters, game time in seconds; StatXX: not shown on the screen)");
        s.node("Stats", |s| {
            self.stats.resize(STATS.len(), 0);
            for ((name, kind), v) in STATS.iter().zip(self.stats.iter_mut()) {
                match kind {
                    b'f' => {
                        let mut f = f32::from_bits(*v);
                        s.f32(name, &mut f)?;
                        *v = f.to_bits();
                    }
                    b'b' => {
                        let mut b = *v as u8;
                        s.u8(name, &mut b)?;
                        *v = b as u32;
                    }
                    _ => s.u32(name, v)?,
                }
            }
            s.list("ContrabandTypes", &mut self.stats_pairs, Len::U32, |s, x| s.val("", x, Show::Dec))?;
            s.u32("TotalContrabandCollected", &mut self.stats_f[0])?;
            s.u32("TotalContrabandSpent", &mut self.stats_f[1])?;
            s.list("List", &mut self.stats_list, Len::U32, |s, x| s.u32("", x))?;
            let mut c: [u32; 4] = std::array::from_fn(|i| self.stats_lists[i].len() as u32);
            for x in &mut c {
                s.hide(x)?;
            }
            for (i, l) in self.stats_lists.iter_mut().enumerate() {
                let n = s.n(c[i]);
                s.list(&format!("Lists{}", i + 1), l, n, |s, x| s.val("", x, Show::Dec))?;
            }
            Ok(())
        })?;
        s.node("Block_a090e0", |s| {
            s.u32("Value", &mut self.a090.0)?;
            pairs(s, "Items", &mut self.a090.1, Len::N(4))
        })?;
        s.note("Papers: the identity papers lying in the world and where");
        s.list("Papers", &mut self.papers, Len::U8, |s, p| {
            s.i32("Value", &mut p.value)?;
            s.hash("Paper", &mut p.paper)?;
            s.vec("Matrix", &mut p.matrix)
        })?;
        s.node("BigMap", |s| {
            s.u32("Value", &mut self.map.0)?;
            s.u32("Value2", &mut self.map.1)?;
            s.raw("Data", &mut self.map.2, Len::N(0xc000))
        })?;
        s.hashes("Block_9f5520", &mut self.f55, Len::U32)?;
        s.node("Block_7877c0", |s| {
            s.u32("Value", &mut self.e877.0)?;
            s.u32("Value2", &mut self.e877.1)?;
            s.raw("Data", &mut self.e877.2, Len::N(512))?;
            s.raw("Data2", &mut self.e877.3, Len::N(512))
        })?;
        s.val("Block_651f10", &mut self.maxima, Show::Dec)?;
        s.tail("Slack", &mut self.slack)
    }
}

impl Data for Save {
    const ID: &'static str = "save";
    const ABOUT: &'static str = "saved game (.profile in Documents\\My Games\\The Saboteur™\\SaveGames)";
    const EXT: &'static [&'static str] = &["profile"];

    fn io<S: Io>(&mut self, s: &mut S) -> Res<()> {
        self.walk(s)
    }

    fn size(&self) -> String {
        format!("{}, contraband {}", date(self.time), self.contraband[0])
    }

    fn endian(_: &[u8]) -> Option<Endian> {
        Some(Endian::Le)
    }

    fn read_as(d: &[u8], e: Endian) -> Res<Self> {
        if d.len() != SIZE {
            return err(format!("a save is {SIZE} bytes, this file is {}", d.len()));
        }
        if d[STREAM + BUFFER..].iter().any(|&b| b != 0) {
            return err("the end of the file is not zero");
        }
        let end = STREAM + d[STREAM..STREAM + BUFFER].iter().rposition(|&b| b != FILL).map_or(0, |p| p + 1);
        let mut x = Self::default();
        let mut s = Read::new(&d[..end], e);
        x.io(&mut s)?;
        Ok(x)
    }

    fn write(&mut self, e: Endian) -> Res<Vec<u8>> {
        let mut s = Write::new(e);
        self.io(&mut s)?;
        let mut d = s.w.finish();
        if d.len() > STREAM + BUFFER {
            return err(format!("the saved game is {} bytes too large (the game keeps at most {BUFFER} bytes)", d.len() - STREAM - BUFFER));
        }
        d.resize(STREAM + BUFFER, FILL);
        d.resize(SIZE, 0);
        let sum = d[STREAM + 8..STREAM + BUFFER].iter().fold(0i32, |a, &b| a.wrapping_add(b as i8 as i32));
        d[STREAM + 4..STREAM + 8].copy_from_slice(&sum.to_le_bytes());
        let c = crc(&d[4..]);
        d[..4].copy_from_slice(&c.to_le_bytes());
        Ok(d)
    }
}

#[cfg(test)]
mod t {
    use super::*;

    #[test]
    #[ignore]
    fn golden_saves() {
        let Ok(dir) = std::env::var("WST_SAVES") else { return };
        let mut n = 0;
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let d = std::fs::read(e.path()).unwrap();
            let (mut x, e2) = Save::read(&d).unwrap();
            let v = super::super::data::text_of("save.profile", &mut x, e2).unwrap();
            let text = ac_lua::data::chunk(&v, &Default::default());
            let back = ac_lua::data::parse_chunk(&text).unwrap();
            let (mut y, e3) = super::super::data::from_text::<Save>(&back, Default::default()).unwrap();
            assert!(y.write(e3).unwrap() == d, "{}", e.path().display());
            n += 1;
        }
        assert!(n > 0);
    }

    #[test]
    fn crc_matches_the_game() {
        assert_eq!(crc(b""), 0x811c_9dc5);
        assert_eq!(date(0), "1970-01-01 00:00:00 UTC");
        assert_eq!(date(1_468_485_063), "2016-07-14 08:31:03 UTC");
    }
}
