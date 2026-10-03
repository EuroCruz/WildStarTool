use ac_lua::Val;
use std::sync::{LazyLock, RwLock};

pub fn hash(s: &str) -> u32 {
    if s.is_empty() {
        return 0;
    }
    let h = s.bytes().fold(0x811c_9dc5u32, |h, c| ((c | 0x20) as u32 ^ h).wrapping_mul(0x0100_0193));
    (h ^ 0x2a).wrapping_mul(0x0100_0193)
}

struct Dict {
    text: String,
    idx: Vec<(u32, u32, u32)>,
    extra: Vec<(u32, String)>,
}

impl Dict {
    fn load(packed: &[u8]) -> Dict {
        let text = String::from_utf8(ac_pack::zstd::decode(packed).unwrap_or_default()).unwrap_or_default();
        let mut idx = Vec::new();
        let mut at = 0;
        for l in text.split('\n') {
            match l.split_once('\t').and_then(|(h, n)| Some((u32::from_str_radix(h, 16).ok()?, n))) {
                Some((h, n)) => idx.push((h, (at + l.len() - n.len()) as u32, n.len() as u32)),
                None if !l.is_empty() => idx.push((hash(l), at as u32, l.len() as u32)),
                None => {}
            }
            at += l.len() + 1;
        }
        idx.sort_by_key(|x| x.0);
        idx.dedup_by_key(|x| x.0);
        idx.shrink_to_fit();
        Dict { text, idx, extra: Vec::new() }
    }

    fn get(&self, h: u32) -> Option<String> {
        match self.idx.binary_search_by_key(&h, |x| x.0) {
            Ok(i) => Some(self.text[self.idx[i].1 as usize..][..self.idx[i].2 as usize].to_string()),
            Err(_) => self.extra.iter().find(|x| x.0 == h).map(|x| x.1.clone()),
        }
    }
}

static DICT: LazyLock<RwLock<Dict>> = LazyLock::new(|| {
    let mut d = Dict::load(include_bytes!(concat!(env!("OUT_DIR"), "/names.zst")));
    for n in user_names() {
        let h = hash(&n);
        if d.get(h).is_none() {
            d.extra.push((h, n));
        }
    }
    RwLock::new(d)
});
static ADDED: RwLock<Vec<String>> = RwLock::new(Vec::new());

const USER: &str = "my_names.lua";

fn user_path() -> Option<std::path::PathBuf> {
    Some(std::env::current_exe().ok()?.parent()?.join(USER))
}

fn user_names() -> Vec<String> {
    let Some(t) = user_path().and_then(|p| std::fs::read_to_string(p).ok()) else { return Vec::new() };
    let Ok(v) = ac_lua::data::parse_chunk(&t) else { return Vec::new() };
    v.key("Names").map(Val::items).unwrap_or(&[]).iter().filter_map(|(_, x)| x.str().map(str::to_string)).collect()
}

pub fn key(s: &str) -> u32 {
    let h = hash(s);
    if h != 0 && name(h).is_none() {
        learn(s);
        if let Ok(mut a) = ADDED.write() {
            a.push(s.to_string());
        }
    }
    h
}

pub fn save() {
    let added = ADDED.write().map(|mut a| std::mem::take(&mut *a)).unwrap_or_default();
    if added.is_empty() {
        return;
    }
    let Some(p) = user_path() else { return };
    let mut all = user_names();
    all.extend(added);
    all.sort();
    all.dedup();
    let v = Val::Tbl(vec![
        (None, Val::Note("Your own names (new files, objects, ids) remembered by wildstartool pack,
so that unpack shows them instead of 0x... hashes. Add or remove names freely.".into())),
        (Some("Names".into()), Val::Tbl(all.into_iter().map(|n| (None, Val::Str(n))).collect())),
    ]);
    let _ = std::fs::write(p, ac_lua::data::chunk(&v, &ac_lua::data::Style::default()));
}
static SCRIPTS: LazyLock<Dict> = LazyLock::new(|| Dict::load(include_bytes!(concat!(env!("OUT_DIR"), "/scripts.zst"))));

pub fn name(h: u32) -> Option<String> {
    DICT.read().ok()?.get(h)
}

pub fn script(h: u32) -> Option<String> {
    SCRIPTS.get(h)
}

pub fn learn(s: &str) {
    let h = hash(s);
    if name(h).is_none() {
        if let Ok(mut d) = DICT.write() {
            d.extra.push((h, s.to_string()));
        }
    }
}

pub fn count() -> usize {
    DICT.read().map(|d| d.idx.len() + d.extra.len()).unwrap_or(0)
}

pub fn show(h: u32) -> Val {
    match name(h) {
        Some(n) if h != 0 && hash(&n) == h => Val::Str(n),
        _ => Val::Raw(format!("0x{h:08x}")),
    }
}

pub fn label(h: u32) -> String {
    name(h).filter(|_| h != 0).unwrap_or_else(|| format!("0x{h:08x}"))
}

#[cfg(test)]
mod t {
    use super::*;

    #[test]
    fn hashes() {
        assert_eq!(hash("ANY"), 0xed05_7225);
        assert_eq!(hash(r"d:\Scripts\Includes\WRAPPER_Util.luac"), 0xda8b_14f5);
        assert_eq!(hash("AttackAction"), 0x7a9f_0a17);
        assert_eq!(hash(""), 0);
    }

    #[test]
    fn dictionary() {
        assert!(count() > 1_000_000, "{}", count());
        assert_eq!(name(hash("AttackAction")).as_deref(), Some("AttackAction"));
        learn("WildStarTool_Test_Name");
        assert_eq!(label(hash("wildstartool_test_name")), "WildStarTool_Test_Name");
        assert_eq!(label(1), "0x00000001");
        assert_eq!(script(0x003e_5fef).as_deref(), Some("Missions/NOTE_P_Qualifier.lua"));
    }
}
