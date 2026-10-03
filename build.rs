use std::collections::BTreeMap;
use std::path::Path;

fn hash(s: &str) -> u32 {
    let h = s.bytes().fold(0x811c_9dc5u32, |h, c| ((c | 0x20) as u32 ^ h).wrapping_mul(0x0100_0193));
    (h ^ 0x2a).wrapping_mul(0x0100_0193)
}

fn main() {
    println!("cargo:rerun-if-changed=data");
    let mut files: Vec<_> = std::fs::read_dir("data").unwrap().filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "lua")).collect();
    files.sort();
    let mut out = BTreeMap::<String, Vec<String>>::new();
    for p in files {
        println!("cargo:rerun-if-changed={}", p.display());
        let v = ac_lua::data::parse_chunk(&std::fs::read_to_string(&p).unwrap()).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        for (k, t) in v.items() {
            let lines = out.entry(k.as_deref().unwrap_or("").to_lowercase()).or_default();
            for (h, n) in t.items() {
                match (h.as_deref().and_then(|h| h.parse::<u32>().ok()), n.str()) {
                    (Some(h), Some(n)) if hash(n) == h && !n.is_empty() => lines.push(n.to_string()),
                    (Some(h), Some(n)) => lines.push(format!("{h:08x}\t{n}")),
                    _ => panic!("{}: every line must be [0xHASH] = \"name\"", p.display()),
                }
            }
        }
    }
    let dir = std::env::var("OUT_DIR").unwrap();
    for (k, mut lines) in out {
        lines.sort_unstable_by(|a, b| a.rsplit('\t').next().cmp(&b.rsplit('\t').next()));
        std::fs::write(Path::new(&dir).join(format!("{k}.zst")), ac_pack::zstd::encode(lines.join("\n").as_bytes(), 9)).unwrap();
    }
}
