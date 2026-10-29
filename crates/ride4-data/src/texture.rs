//! Cooked UE 4.25 `Texture2D`s (the menu's track pictures) decoded to RGBA.
//!
//! After the tagged properties, a cooked texture's `.uexp` holds its platform data:
//! `SizeX i32, SizeY i32, PackedData u32, PixelFormat FString, FirstMip i32, NumMips i32`, then
//! each mip as a cooked flag (u32) and inline bulk data (`flags u32, count i32, size i32, offset
//! i64, bytes`) followed by its `SizeX, SizeY, SizeZ`. The UI textures keep every mip inline (no
//! `.ubulk`), so we find the pixel format string rather than walk the properties before it, and
//! read the first mip.

use crate::reader::{err, Reader, Result};

/// 8-bit RGBA pixels, row by row.
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

const FORMATS: &[&str] = &["PF_B8G8R8A8", "PF_DXT1", "PF_DXT5"];
/// Bulk data stored somewhere other than right here.
const BULK_UNUSED: u32 = 0x20;
const BULK_SEPARATE_FILE: u32 = 0x100;

pub fn decode(uexp: &[u8]) -> Result<Image> {
    let Some((at, format)) = FORMATS.iter().find_map(|f| {
        let mut needle = ((f.len() + 1) as i32).to_le_bytes().to_vec();
        needle.extend_from_slice(f.as_bytes());
        needle.push(0);
        uexp.windows(needle.len()).position(|w| w == needle).map(|p| (p, *f))
    }) else {
        return err("no supported pixel format");
    };
    let mut r = Reader::new(uexp);
    r.p = at.checked_sub(12).ok_or_else(|| crate::Error("texture header cut off".into()))?;
    let (w, h) = (r.i32()?, r.i32()?);
    r.u32()?; // packed: slice count and cubemap flag
    r.fstring()?;
    r.i32()?; // first mip to serialize
    let mips = r.i32()?;
    if mips < 1 {
        return err("texture has no mips");
    }
    r.u32()?; // mip cooked flag
    let flags = r.u32()?;
    if flags & (BULK_UNUSED | BULK_SEPARATE_FILE) != 0 {
        return err(format!("first mip isn't inline (bulk flags {flags:#x})"));
    }
    r.i32()?; // element count
    let size = r.i32()?;
    r.i64()?; // offset in file
    let data = r.take(size.max(0) as usize)?;
    let (mw, mh) = (r.i32()?, r.i32()?);
    if (mw, mh) != (w, h) || w <= 0 || h <= 0 {
        return err(format!("first mip is {mw}x{mh}, texture is {w}x{h}"));
    }
    let (width, height) = (w as u32, h as u32);
    let rgba = match format {
        "PF_B8G8R8A8" => bgra(data, width, height)?,
        "PF_DXT1" => blocks(data, width, height, 8, dxt1)?,
        _ => blocks(data, width, height, 16, dxt5)?,
    };
    Ok(Image { width, height, rgba })
}

fn bgra(data: &[u8], w: u32, h: u32) -> Result<Vec<u8>> {
    let n = (w * h * 4) as usize;
    if data.len() < n {
        return err(format!("{} bytes of BGRA for {w}x{h}", data.len()));
    }
    Ok(data[..n].chunks_exact(4).flat_map(|p| [p[2], p[1], p[0], p[3]]).collect())
}

/// Decode 4x4 blocks of `size` bytes each into RGBA, clipping blocks past the right and bottom edges.
fn blocks(data: &[u8], w: u32, h: u32, size: usize, block: fn(&[u8]) -> [[u8; 4]; 16]) -> Result<Vec<u8>> {
    let (bw, bh) = (w.div_ceil(4) as usize, h.div_ceil(4) as usize);
    if data.len() < bw * bh * size {
        return err(format!("{} bytes of blocks for {w}x{h}", data.len()));
    }
    let (w, h) = (w as usize, h as usize);
    let mut out = vec![0; w * h * 4];
    for by in 0..bh {
        for bx in 0..bw {
            let px = block(&data[(by * bw + bx) * size..][..size]);
            for (i, p) in px.iter().enumerate() {
                let (x, y) = (bx * 4 + i % 4, by * 4 + i / 4);
                if x < w && y < h {
                    out[(y * w + x) * 4..][..4].copy_from_slice(p);
                }
            }
        }
    }
    Ok(out)
}

fn rgb565(c: u16) -> [u8; 3] {
    let (r, g, b) = ((c >> 11) & 31, (c >> 5) & 63, c & 31);
    [(r << 3 | r >> 2) as u8, (g << 2 | g >> 4) as u8, (b << 3 | b >> 2) as u8]
}

/// The colour part of a DXT block: two RGB565 endpoints and 2-bit indices. `opaque` forces the
/// four-colour mode (DXT5 colour blocks never use DXT1's punch-through alpha).
fn colours(b: &[u8], opaque: bool) -> [[u8; 4]; 16] {
    let (c0, c1) = (u16::from_le_bytes([b[0], b[1]]), u16::from_le_bytes([b[2], b[3]]));
    let (a, z) = (rgb565(c0), rgb565(c1));
    let mix = |p: u16, q: u16, d: u16| -> [u8; 4] {
        let f = |i: usize| ((a[i] as u16 * p + z[i] as u16 * q) / d) as u8;
        [f(0), f(1), f(2), 255]
    };
    let palette = if c0 > c1 || opaque {
        [mix(1, 0, 1), mix(0, 1, 1), mix(2, 1, 3), mix(1, 2, 3)]
    } else {
        [mix(1, 0, 1), mix(0, 1, 1), mix(1, 1, 2), [0, 0, 0, 0]]
    };
    let bits = u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
    std::array::from_fn(|i| palette[(bits >> (2 * i) & 3) as usize])
}

fn dxt1(b: &[u8]) -> [[u8; 4]; 16] {
    colours(b, false)
}

/// DXT5: an interpolated alpha block (two endpoints, 3-bit indices) then a DXT1 colour block.
fn dxt5(b: &[u8]) -> [[u8; 4]; 16] {
    let (a0, a1) = (b[0] as u32, b[1] as u32);
    let alpha: [u8; 8] = std::array::from_fn(|i| {
        let i = i as u32;
        match i {
            0 => a0 as u8,
            1 => a1 as u8,
            _ if a0 > a1 => (((8 - i) * a0 + (i - 1) * a1) / 7) as u8,
            2..=5 => (((6 - i) * a0 + (i - 1) * a1) / 5) as u8,
            6 => 0,
            _ => 255,
        }
    });
    let bits = b[2..8].iter().rev().fold(0u64, |acc, &x| acc << 8 | x as u64);
    let mut px = colours(&b[8..], true);
    for (i, p) in px.iter_mut().enumerate() {
        p[3] = alpha[(bits >> (3 * i) & 7) as usize];
    }
    px
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A cooked-texture `.uexp` tail around one mip.
    fn uexp(format: &str, w: i32, h: i32, mip: &[u8]) -> Vec<u8> {
        let mut b = vec![0xAB; 20]; // stands in for the tagged properties
        for v in [w, h, 1] {
            b.extend(v.to_le_bytes());
        }
        b.extend(((format.len() + 1) as i32).to_le_bytes());
        b.extend(format.as_bytes());
        b.push(0);
        for v in [0, 1, 1, 0x48, mip.len() as i32, mip.len() as i32] {
            b.extend(v.to_le_bytes());
        }
        b.extend(0i64.to_le_bytes());
        b.extend(mip);
        for v in [w, h, 1] {
            b.extend(v.to_le_bytes());
        }
        b
    }

    #[test]
    fn bgra_swaps_to_rgba() {
        let img = decode(&uexp("PF_B8G8R8A8", 2, 1, &[1, 2, 3, 4, 5, 6, 7, 8])).unwrap();
        assert_eq!((img.width, img.height), (2, 1));
        assert_eq!(img.rgba, [3, 2, 1, 4, 7, 6, 5, 8]);
    }

    #[test]
    fn dxt1_palette_and_clipping() {
        // red and blue endpoints; first row picks colours 0, 1, 2, 3, the rest colour 0
        let block = [0x00, 0xF8, 0x1F, 0x00, 0b11_10_01_00, 0, 0, 0];
        let img = decode(&uexp("PF_DXT1", 3, 2, &block)).unwrap();
        let px: Vec<&[u8]> = img.rgba.chunks(4).collect();
        assert_eq!(px.len(), 6);
        assert_eq!(px[0], [255, 0, 0, 255]);
        assert_eq!(px[1], [0, 0, 255, 255]);
        assert_eq!(px[2], [170, 0, 85, 255]);
        assert_eq!(px[3], [255, 0, 0, 255]); // second row, clipped to 3 wide
    }

    #[test]
    fn dxt1_punch_through_alpha() {
        // c0 <= c1 selects the three-colour mode, where index 3 is transparent
        let block = [0x1F, 0x00, 0x00, 0xF8, 0xFF, 0xFF, 0xFF, 0xFF];
        let img = decode(&uexp("PF_DXT1", 4, 4, &block)).unwrap();
        assert!(img.rgba.chunks(4).all(|p| p == [0, 0, 0, 0]));
    }

    #[test]
    fn dxt5_alpha_ramp() {
        // alpha 255..0 over eight steps; pixel i uses alpha index i % 8
        let idx: u64 = (0..16).map(|i| ((i % 8) as u64) << (3 * i)).sum();
        let mut block = vec![255, 0];
        block.extend(&idx.to_le_bytes()[..6]);
        block.extend([0xFF, 0xFF, 0xFF, 0xFF, 0, 0, 0, 0]);
        let img = decode(&uexp("PF_DXT5", 4, 4, &block)).unwrap();
        let a: Vec<u8> = img.rgba.chunks(4).map(|p| p[3]).collect();
        assert_eq!(a[..8], [255, 0, 218, 182, 145, 109, 72, 36]);
        assert_eq!(img.rgba[..3], [255, 255, 255]);
    }

    #[test]
    fn rejects_unknown_formats() {
        assert!(decode(&uexp("PF_BC5", 4, 4, &[0; 16])).is_err());
    }
}
