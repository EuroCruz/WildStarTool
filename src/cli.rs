use crate::fmt::{self, Input, Opts, Out, INDEX};
use crate::names;
use ac_core::{Error, Res};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

const HELP: &str = "\
WildStarTool {v} — The Saboteur modding tool

  wildstartool unpack <file> [folder]       unpack a game file into one .lua file or a folder (see: formats)
  wildstartool pack   <folder|file.lua> [file]   build the game file back
  wildstartool info   <file>                what the file is
  wildstartool list   <file>                what is inside
  wildstartool hash   <name>                hash of a name, as the game computes it
  wildstartool name   <hash>                look up the name behind a hash
  wildstartool new    <kind> <file> [value] create a file:
      new sku sku.bin [0..5]                game edition, next to the game exe (1 = window title \"Saboteur\")
      new preorder data01.bin               unlocks the pre-order knife (Documents\\My Games\\The Saboteur™\\SaveGames)
      new videoconfig config.ini [WxH]      all video settings the game reads, e.g. 1920x1080 (same folder)
  wildstartool formats                      all supported files

  Drop a file on the program to unpack it, drop a folder or .lua file to pack it.

Options
  --be     pack for PS3 / Xbox 360 (big-endian)
  --raw    keep textures as game records instead of .dds";

struct Args {
    pos: Vec<String>,
    opts: Opts,
}

fn args(a: Vec<String>) -> Result<Args, String> {
    let mut r = Args { pos: Vec::new(), opts: Opts::default() };
    for x in a {
        match x.as_str() {
            "--be" => r.opts.be = true,
            "--raw" => r.opts.raw = true,
            "-h" | "--help" | "/?" => r.pos.insert(0, "help".into()),
            "-v" | "--version" => r.pos.insert(0, "version".into()),
            s if s.starts_with("--") => return Err(format!("unknown option {s} (see: wildstartool help)")),
            _ => r.pos.push(x),
        }
    }
    Ok(r)
}

fn need<'a>(p: &'a [String], i: usize, what: &str) -> Res<&'a str> {
    p.get(i).map(String::as_str).ok_or_else(|| Error::Msg(format!("missing {what} (see: wildstartool help)")))
}

fn size(n: u64) -> String {
    let s = n.to_string();
    let mut o = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            o.push(',');
        }
        o.push(c);
    }
    format!("{o} bytes")
}

fn shown(p: &Path) -> String {
    let c = std::env::current_dir().unwrap_or_default();
    p.strip_prefix(&c).unwrap_or(p).display().to_string()
}

fn open(p: &str) -> Res<(Input, &'static dyn fmt::Format)> {
    let path = Path::new(p);
    if path.is_dir() {
        return Err(Error::Msg(format!("{p} is a folder — to build it back use: wildstartool pack {p}")));
    }
    let mut i = Input::open(path)?;
    if let Some(f) = fmt::detect(&mut i) {
        return Ok((i, f));
    }
    let ext = i.ext();
    let fs: Vec<_> = fmt::all().iter().filter(|f| ext != "bin" && f.exts().contains(&ext.as_str())).collect();
    let name = i.name.clone();
    let mut why = |f: &&&dyn fmt::Format| f.info(&mut i).err().map_or("unknown layout".into(), |e| e.to_string());
    match fs.as_slice() {
        [] => Err(Error::Msg(format!("{} is not a file WildStarTool knows (see: wildstartool formats)", name))),
        [f] => Err(Error::Msg(format!("{} looks like {} but cannot be read: {}", name, f.about(), why(f)))),
        _ => {
            let tried: Vec<String> = fs.iter().map(|f| format!("\n  {}: {}", f.id(), why(f))).collect();
            Err(Error::Msg(format!("{} is not a .{ext} layout WildStarTool knows:{}", name, tried.concat())))
        }
    }
}

fn unpack(a: &Args) -> Res<()> {
    let src = need(&a.pos, 1, "file to unpack")?;
    let (mut i, f) = open(src)?;
    let base = i.path.parent().map(Path::to_path_buf).unwrap_or_default();
    let dir = match (a.pos.get(2), f.folder()) {
        (Some(d), _) => PathBuf::from(d),
        (None, true) => base.join(i.name.replace('.', "_")),
        (None, false) => base,
    };
    let t = Instant::now();
    let mut o = Out::new(&dir, a.opts);
    let made = f.unpack(&mut i, &mut o)?;
    match f.folder() {
        true => println!("{}  →  folder {}", i.name, shown(&dir)),
        false => println!("{}  →  file {}", i.name, shown(&made)),
    }
    let n = if f.folder() { format!(", {} files", o.files) } else { String::new() };
    println!("   {}{n}, {:.1}s", f.about(), t.elapsed().as_secs_f32());
    for w in &o.warn {
        println!("   note: {w}");
    }
    Ok(())
}

fn pack(a: &Args) -> Res<()> {
    let src = PathBuf::from(need(&a.pos, 1, "folder or .lua file to pack")?);
    let (src, idx) = fmt::source(&src).ok_or_else(|| {
        Error::Msg(if src.is_dir() { format!("{} has no {INDEX} — is it a folder made by wildstartool unpack?", src.display()) } else { format!("{} is not a folder or .lua file made by wildstartool unpack", src.display()) })
    })?;
    let meta = fmt::read_text(&idx)?;
    let file = meta.key("File").and_then(|v| v.str()).ok_or_else(|| Error::Msg(format!("{} has no File = \"...\" line", idx.display())))?.to_string();
    let out = match a.pos.get(2) {
        Some(o) if Path::new(o).is_dir() => Path::new(o).join(&file),
        Some(o) => PathBuf::from(o),
        None => src.parent().unwrap_or(Path::new("")).join(&file),
    };
    let t = Instant::now();
    let bak = ac_core::fs::backup(&out)?;
    let tmp = out.with_file_name(format!("{file}.wst-tmp"));
    let mut sink = fmt::Sink::create(&tmp)?;
    let r = fmt::pack_into(&src, &idx, a.opts, &mut sink);
    let n = sink.len;
    let r = r.and_then(|f| sink.done().map(|_| f));
    let f = r.inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })?;
    std::fs::rename(&tmp, &out)?;
    names::save();
    println!("{}  →  {}", shown(&src), shown(&out));
    println!("   {}, {}, {:.1}s", f.about(), size(n), t.elapsed().as_secs_f32());
    if let Some(b) = bak {
        println!("   original saved as {}", shown(&b));
    }
    Ok(())
}

fn info(a: &Args) -> Res<()> {
    let (mut i, f) = open(need(&a.pos, 1, "file")?)?;
    println!("{}", i.name);
    let mut rows = vec![("format", format!("{} — {}", f.id(), f.about()))];
    rows.extend(f.info(&mut i)?);
    rows.push(("size", size(i.len)));
    for (k, v) in rows {
        println!("  {k:<11} {v}");
    }
    Ok(())
}

fn list(a: &Args) -> Res<()> {
    let (mut i, f) = open(need(&a.pos, 1, "file")?)?;
    let items = f.list(&mut i)?;
    let w = items.iter().map(|x| x.name.chars().count()).max().unwrap_or(0).min(80);
    for x in &items {
        println!("{:<w$}  {:>14}  {}", x.name, size(x.size), x.note);
    }
    println!("{} items in {}", items.len(), i.name);
    Ok(())
}

fn hash(a: &Args) -> Res<()> {
    let s = need(&a.pos, 1, "name")?;
    for s in &a.pos[1..] {
        println!("0x{:08x}  {s}", names::hash(s));
    }
    let _ = s;
    Ok(())
}

fn name(a: &Args) -> Res<()> {
    need(&a.pos, 1, "hash")?;
    for s in &a.pos[1..] {
        let h = u32::from_str_radix(s.trim_start_matches("0x").trim_start_matches("0X"), 16).map_err(|_| Error::Msg(format!("{s} is not a hex hash like 0x1a2b3c4d")))?;
        match names::name(h) {
            Some(n) => println!("0x{h:08x}  {n}"),
            None => println!("0x{h:08x}  (unknown)"),
        }
    }
    Ok(())
}

fn formats() -> Res<()> {
    for (folder, title) in [(false, "Unpacked into one .lua file (edit it, then pack it):"), (true, "Unpacked into a folder with index.lua (files inside are edited or replaced):")] {
        println!("{title}");
        for f in fmt::all().iter().filter(|f| f.folder() == folder) {
            let e: Vec<String> = f.exts().iter().map(|e| format!(".{e}")).collect();
            println!("  {:<24} {}", e.join(" "), f.about());
        }
        println!();
    }
    Ok(())
}

fn new(a: &Args) -> Res<()> {
    let kind = need(&a.pos, 1, "kind: sku, preorder or videoconfig")?;
    let out = PathBuf::from(need(&a.pos, 2, "output file")?);
    let d = crate::fmt::make(kind, a.pos.get(3).map(String::as_str), a.opts)?;
    ac_core::fs::write(&out, &d)?;
    println!("{kind}  →  {}  ({})", shown(&out), size(d.len() as u64));
    Ok(())
}

#[cfg(windows)]
fn own_console() -> bool {
    extern "system" {
        fn GetConsoleProcessList(l: *mut u32, n: u32) -> u32;
    }
    let mut l = [0u32; 4];
    unsafe { GetConsoleProcessList(l.as_mut_ptr(), 4) == 1 }
}

#[cfg(not(windows))]
fn own_console() -> bool {
    false
}

pub fn run(raw: Vec<String>) -> ExitCode {
    let a = match args(raw) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    let mut cmd = a.pos.first().cloned().unwrap_or_default();
    let mut a = a;
    if !matches!(cmd.as_str(), "unpack" | "pack" | "info" | "list" | "hash" | "name" | "new" | "formats" | "help" | "version" | "") {
        let p = Path::new(&cmd);
        if p.exists() {
            cmd = if p.is_dir() || cmd.to_ascii_lowercase().ends_with(".lua") && std::fs::read_to_string(p).is_ok_and(|t| t.contains("Format = ")) { "pack" } else { "unpack" }.into();
            a.pos.insert(0, cmd.clone());
        }
    }
    let r = match cmd.as_str() {
        "unpack" => unpack(&a),
        "pack" => pack(&a),
        "info" => info(&a),
        "list" => list(&a),
        "hash" => hash(&a),
        "name" => name(&a),
        "new" => new(&a),
        "formats" => formats(),
        "version" => {
            println!("WildStarTool {VERSION}");
            Ok(())
        }
        "" | "help" => {
            println!("{}", HELP.replace("{v}", VERSION));
            Ok(())
        }
        c => Err(Error::Msg(format!("unknown command or missing file: {c} (see: wildstartool help)"))),
    };
    let code = match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    };
    if own_console() {
        eprintln!("\npress Enter to close");
        let _ = std::io::stdin().read_line(&mut String::new());
    }
    code
}
