use super::*;
use ac_lua::data::{chunk, Style};
use std::collections::BTreeMap;

const META: [&str; 3] = ["Format", "File", "Endian"];

fn items(v: &Val) -> Vec<&Val> {
    v.items().iter().filter(|(_, x)| !matches!(x, Val::Note(_))).map(|(_, x)| x).collect()
}

fn num_eq(a: &Val, b: &Val) -> bool {
    match (a, b) {
        (Val::Raw(x), Val::Raw(y)) if x.contains('.') || y.contains('.') => x.parse::<f32>().ok() == y.parse::<f32>().ok(),
        (Val::Tbl(x), Val::Tbl(y)) => {
            let f = |t: &Vec<(Option<String>, Val)>| t.iter().filter(|(_, v)| !matches!(v, Val::Note(_))).cloned().collect::<Vec<_>>();
            let (x, y) = (f(x), f(y));
            x.len() == y.len() && x.iter().zip(&y).all(|(p, q)| p.0 == q.0 && num_eq(&p.1, &q.1))
        }
        (Val::Call(n, x), Val::Call(m, y)) => n == m && x.len() == y.len() && x.iter().zip(y).all(|(p, q)| num_eq(p, q)),
        _ => a == b,
    }
}

fn bump(v: &mut Val) -> Option<String> {
    match v {
        Val::Raw(r) if r.contains('.') && !r.starts_with("0x") => {
            let f: f32 = r.parse().ok()?;
            *v = crate::io::float(f + 1.0);
            Some("float".into())
        }
        Val::Tbl(t) => {
            for (k, x) in t.iter_mut() {
                if matches!(k.as_deref(), Some("Kind" | "Level" | "Type" | "Version" | "Mode" | "Grid")) {
                    continue;
                }
                if let Some(s) = bump(x) {
                    return Some(format!("{}.{s}", k.as_deref().unwrap_or("[]")));
                }
            }
            None
        }
        _ => None,
    }
}

fn edit_text(v: &mut Val) -> Option<(String, String)> {
    let Val::Tbl(t) = v else { return None };
    let key = t.iter().find(|(k, x)| k.as_deref().is_some_and(|k| !META.contains(&k)) && {
        let it = items(x);
        it.len() >= 2 && it.iter().all(|i| matches!(i, Val::Tbl(_)))
    })?.0.clone()?;
    let list = t.iter_mut().find(|(k, _)| k.as_deref() == Some(&key))?;
    let Val::Tbl(l) = &mut list.1 else { return None };
    l.retain(|(_, x)| !matches!(x, Val::Note(_)));
    l.pop();
    let mut first = l[0].clone();
    let what = bump(&mut first.1).unwrap_or_else(|| "nothing to bump".into());
    l[0] = first.clone();
    let max = l.iter().filter_map(|(_, r)| r.key("Id").and_then(Val::int)).max();
    if let (Some(m), Val::Tbl(ft)) = (max, &mut first.1) {
        if let Some(id) = ft.iter_mut().find(|(k, _)| k.as_deref() == Some("Id")) {
            id.1 = Val::Int(m + 1);
        }
    }
    l.push(first);
    Some((key, what))
}

fn write_val(p: &Path, v: &Val) {
    std::fs::write(p, chunk(v, &Style::default())).unwrap();
}

struct Case {
    unit: PathBuf,
    format: String,
    check: Check,
}

enum Check {
    List(String, Val),
    Files(String, Vec<String>),
    Dir(String, Vec<String>),
}

fn entry_name(v: &Val) -> String {
    super::arch::entry_path(v).unwrap_or_default()
}

fn edit_folder(dir: &Path, idx: &mut Val, fmt: &str) -> Option<Check> {
    if fmt == "pck" {
        let sub = ["Banks", "Streams"].into_iter().find(|d| super::arch::new_units(&dir.join(d), &[]).is_ok_and(|u| u.len() >= 2))?;
        let banks = dir.join(sub);
        let mut units = super::arch::new_units(&banks, &[]).ok()?;
        units.sort();
        let last = units.pop()?;
        let p = banks.join(&last);
        if p.is_dir() { std::fs::remove_dir_all(&p).ok()? } else { std::fs::remove_file(&p).ok()? }
        let first = banks.join(&units[0]);
        let new = format!("0xffff0001{}", units[0].rsplit_once('.').map_or(String::new(), |x| format!(".{}", x.1)));
        if first.is_dir() {
            copy_dir(&first, &banks.join(&new));
        } else {
            std::fs::copy(&first, banks.join(&new)).ok()?;
        }
        units.push(new);
        units.sort();
        return Some(Check::Dir(sub.into(), units));
    }
    let key = match fmt {
        "bnk" => "Sounds",
        "megapack" => "Packs",
        _ => "Files",
    };
    let Val::Tbl(t) = idx else { return None };
    let list = t.iter_mut().find(|(k, _)| k.as_deref() == Some(key))?;
    let Val::Tbl(l) = &mut list.1 else { return None };
    l.retain(|(_, x)| !matches!(x, Val::Note(_)));
    if l.len() < 2 {
        return None;
    }
    let last = l.pop()?;
    let lp = dir.join(entry_name(&last.1));
    if lp.is_dir() { std::fs::remove_dir_all(&lp).ok()? } else { let _ = std::fs::remove_file(&lp); }
    let _ = std::fs::remove_file(format!("{}.lua", lp.display()));
    let src_name = entry_name(&l[0].1);
    let (stem, ext) = src_name.rsplit_once('.').map_or((src_name.clone(), String::new()), |(s, e)| (s.to_string(), format!(".{e}")));
    let new = if fmt == "bnk" { "0xffff0002.wem".to_string() } else { format!("{stem}_mod{ext}") };
    let sp = dir.join(&src_name);
    let lua = PathBuf::from(format!("{}.lua", sp.display()));
    if !sp.exists() && lua.is_file() {
        std::fs::copy(&lua, format!("{}.lua", dir.join(&new).display())).ok()?;
    } else if sp.is_dir() {
        copy_dir(&sp, &dir.join(&new));
    } else {
        std::fs::copy(&sp, dir.join(&new)).ok()?;
    }
    let mut e = l[0].1.clone();
    if let Val::Tbl(et) = &mut e {
        et.retain(|(k, _)| !matches!(k.as_deref(), Some("Hash" | "Key" | "Name" | "Offset" | "Crc")));
        et[0].1 = Val::Str(new.clone());
        if et.len() == 1 {
            e = Val::Str(new.clone());
        }
    } else {
        e = Val::Str(new.clone());
    }
    l.push((None, e));
    let names: Vec<String> = l.iter().map(|(_, x)| entry_name(x)).collect();
    Some(Check::Files(key.into(), names))
}

fn copy_dir(a: &Path, b: &Path) {
    for p in ac_core::fs::walk(a).unwrap() {
        let t = b.join(p.strip_prefix(a).unwrap());
        ac_core::fs::ensure_parent(&t).unwrap();
        std::fs::copy(&p, &t).unwrap();
    }
}

fn units(root: &Path) -> Vec<(PathBuf, PathBuf, String)> {
    let mut v = Vec::new();
    for p in ac_core::fs::walk(root).unwrap() {
        let n = p.file_name().unwrap().to_string_lossy().to_string();
        if !n.ends_with(".lua") {
            continue;
        }
        let Ok(val) = read_text(&p) else { continue };
        let Some(f) = val.key("Format").and_then(Val::str).map(str::to_string) else { continue };
        let unit = if n == INDEX { p.parent().unwrap().to_path_buf() } else { p.clone() };
        v.push((unit, p, f));
    }
    v.sort_by_key(|x| std::cmp::Reverse(x.0.components().count()));
    v
}

fn verify(c: &Case, root2: &Path, root: &Path) -> Result<(), String> {
    let rel = c.unit.strip_prefix(root).unwrap();
    let unit = root2.join(rel);
    let idx = if unit.is_dir() { unit.join(INDEX) } else { unit.clone() };
    let v = read_text(&idx).map_err(|e| format!("re-unpacked {} missing: {e}", rel.display()))?;
    match &c.check {
        Check::List(k, want) => {
            let got = v.key(k).ok_or("list missing")?;
            if !num_eq(got, want) {
                let (a, b) = (items(got), items(want));
                let i = a.iter().zip(&b).position(|(x, y)| !num_eq(x, y));
                return Err(format!("{k}: got {} items, want {}; first diff at {:?}: got {} want {}", a.len(), b.len(), i, i.map(|i| chunk(a[i], &Style::default())).unwrap_or_default().chars().take(300).collect::<String>(), i.map(|i| chunk(b[i], &Style::default())).unwrap_or_default().chars().take(300).collect::<String>()));
            }
        }
        Check::Files(k, want) => {
            let got: Vec<String> = v.key(k).map(items).unwrap_or_default().iter().map(|x| entry_name(x)).collect();
            let sorted = |v: &Vec<String>| {
                let mut v = v.clone();
                v.sort();
                v
            };
            if &got != want && !(c.format == "albs" && sorted(&got) == sorted(want)) {
                let i = got.iter().zip(want).position(|(a, b)| a != b);
                return Err(format!("{k}: got {} files, want {}; first diff at {i:?}: {:?} vs {:?} (last got {:?})", got.len(), want.len(), i.and_then(|i| got.get(i)), i.and_then(|i| want.get(i)), got.last()));
            }
        }
        Check::Dir(sub, want) => {
            let mut got = super::arch::new_units(&unit.join(sub), &[]).map_err(|e| e.to_string())?;
            got.sort();
            if &got != want {
                return Err(format!("Banks: got {} want {}", got.len(), want.len()));
            }
        }
    }
    Ok(())
}

fn run(p: &Path, tmp: &Path, done: &mut BTreeMap<String, String>) -> Res<Vec<String>> {
    let s = p.to_string_lossy().to_uppercase();
    let opts = Opts { raw: s.contains("SUBXE") || s.contains("SUBPS3"), be: false };
    let mut i = Input::open(p)?;
    let Some(f) = detect(&mut i) else { return Ok(Vec::new()) };
    let root = tmp.join("a");
    let _ = std::fs::remove_dir_all(tmp);
    let mut o = Out::new(&root, opts);
    let made = f.unpack(&mut i, &mut o)?;
    let mut cases = Vec::new();
    let mut taken: Vec<PathBuf> = Vec::new();
    for (unit, idx, fmt) in units(&root) {
        if done.contains_key(&fmt) || taken.iter().any(|t| t.starts_with(&unit) || unit.starts_with(t)) {
            continue;
        }
        let mut v = read_text(&idx)?;
        let check = if unit.is_dir() {
            edit_folder(&unit, &mut v, &fmt).map(|c| {
                write_val(&idx, &v);
                c
            })
        } else {
            edit_text(&mut v).map(|(k, what)| {
                write_val(&idx, &v);
                let _ = what;
                Check::List(k.clone(), v.key(&k).unwrap().clone())
            })
        };
        if let Some(check) = check {
            taken.push(unit.clone());
            cases.push(Case { unit, format: fmt, check });
        }
    }
    if cases.is_empty() {
        return Ok(Vec::new());
    }
    let src = if f.folder() { root.clone() } else { made };
    let idx = if f.folder() { src.join(INDEX) } else { src.clone() };
    let mut out = Sink::mem();
    let mut results = Vec::new();
    if let Err(e) = pack_into(&src, &idx, opts, &mut out) {
        for c in &cases {
            done.insert(c.format.clone(), format!("FAIL pack: {e}"));
            results.push(format!("{} [{}]: pack failed: {e}", p.display(), c.format));
        }
        return Ok(results);
    }
    let d = out.done()?;
    let mut i2 = Input::from(&i.name, d);
    let root2 = tmp.join("b");
    let mut o2 = Out::new(&root2, opts);
    f.unpack(&mut i2, &mut o2)?;
    for c in &cases {
        let r = verify(c, &root2, &root);
        let line = match &r {
            Ok(()) => format!("ok   {:14} {}", c.format, c.unit.strip_prefix(&root).unwrap().display()),
            Err(e) => format!("FAIL {:14} {}: {e}", c.format, c.unit.strip_prefix(&root).unwrap().display()),
        };
        done.insert(c.format.clone(), line.clone());
        results.push(line);
    }
    Ok(results)
}

#[test]
#[ignore]
fn modding() {
    let max: u64 = 64 << 20;
    let tmp = std::env::temp_dir().join("wst_modtest");
    let mut done = BTreeMap::new();
    let game = PathBuf::from(r"C:\Users\Andrew\Desktop\Saboteur\GAMEFILES");
    let only = std::env::var("WST_PLAT").unwrap_or_else(|_| "SUBPC".into());
    for _ in 0..3 {
        let before = done.len();
        for p in ac_core::fs::walk(&game.join(&only)).unwrap() {
            if std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0) > max {
                continue;
            }
            match run(&p, &tmp, &mut done) {
                Ok(r) => r.iter().for_each(|l| eprintln!("{l}")),
                Err(e) => eprintln!("ERR  {}: {e}", p.display()),
            }
        }
        if done.len() == before {
            break;
        }
    }
    eprintln!("---- summary");
    for (k, v) in &done {
        eprintln!("{k:14} {v}");
    }
    assert!(done.values().all(|v| v.starts_with("ok")));
}
