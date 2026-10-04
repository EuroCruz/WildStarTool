use super::Data;
use crate::io::{err, Io, Len, Prim, Show, Str};
use ac_core::{Endian, Reader, Res, Writer};
use ac_lua::Val;

const NAME: usize = 56;

#[derive(Clone, Copy, Default)]
struct Ch(u8);

impl Prim for Ch {
    fn get(r: &mut Reader) -> Res<Self> {
        r.u8().map(Ch)
    }
    fn put(self, w: &mut Writer) {
        w.u8(self.0);
    }
    fn dump(self, _: Show) -> Val {
        match self.0 {
            0x21..=0x7e | 0xa1..=0xff => Val::Str((self.0 as char).to_string()),
            c => Val::Int(c as i64),
        }
    }
    fn load(v: &Val) -> Option<Self> {
        match v {
            Val::Str(s) => {
                let mut c = s.chars();
                let ch = c.next().filter(|_| c.next().is_none())?;
                u8::try_from(ch as u32).ok().map(Ch)
            }
            v => v.int().and_then(|i| u8::try_from(i).ok()).map(Ch),
        }
    }
    fn same(self, o: Self) -> bool {
        self.0 == o.0
    }
}

#[derive(Default)]
struct Glyph {
    ch: Ch,
    advance: u8,
    rect: [i8; 4],
    uv: [f32; 4],
}

#[derive(Default)]
pub struct Font {
    count: u8,
    height: u8,
    ascent: u8,
    descent: u8,
    texture: String,
    unused: Vec<u8>,
    glyphs: Vec<Glyph>,
}

impl Font {
    fn walk<S: Io>(&mut self, s: &mut S) -> Res<()> {
        s.magic(b"FNT2")?;
        s.hide(&mut self.count)?;
        s.note("Height: line height in pixels (added on a new line). Ascent, Descent, Texture, Unused: not read by the game
(the texture is the .dds next to the font)");
        s.u8("Height", &mut self.height)?;
        s.u8("Ascent", &mut self.ascent)?;
        s.u8("Descent", &mut self.descent)?;
        s.text("Texture", &mut self.texture, Str::Z)?;
        let n = (NAME - 1).saturating_sub(self.texture.len());
        if !s.skip("Unused", self.unused.iter().all(|&b| b == 0)) {
            s.raw("Unused", &mut self.unused, Len::N(n))?;
        }
        s.note("Glyphs: Char (one symbol or its code), Advance (pen step in pixels), Box = { Left, Top, Right, Bottom } in pixels\nfrom the pen, UV = { U0, V0, U1, V1 } on the texture");
        let n = s.n(self.count as u32);
        s.list("Glyphs", &mut self.glyphs, n, |s, g| {
            s.val("Char", &mut g.ch, Show::Dec)?;
            s.u8("Advance", &mut g.advance)?;
            s.val("Box", &mut g.rect, Show::Dec)?;
            s.vec("UV", &mut g.uv)
        })
    }

    fn fixup(&mut self) -> Res<()> {
        if self.texture.len() >= NAME {
            return err(format!("Texture is longer than {} characters", NAME - 1));
        }
        self.count = u8::try_from(self.glyphs.len()).or_else(|_| err("a font has at most 255 glyphs"))?;
        self.unused.resize(NAME - 1 - self.texture.len(), 0);
        Ok(())
    }
}

impl Data for Font {
    const ID: &'static str = "font";
    const ABOUT: &'static str = "bitmap font (glyph boxes on a .dds texture)";
    const EXT: &'static [&'static str] = &["fnt"];
    fn io<S: Io>(&mut self, s: &mut S) -> Res<()> {
        self.walk(s)
    }
    fn fix(&mut self) -> Res<()> {
        self.fixup()
    }
    fn size(&self) -> String {
        format!("{} glyphs", self.glyphs.len())
    }
    fn endian(d: &[u8]) -> Option<Endian> {
        d.starts_with(b"FNT2").then_some(Endian::Le)
    }
}
