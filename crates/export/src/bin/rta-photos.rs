//! ```text
//! rta-photos --out DIR [--game DIR]
//! ```
//!
//! Dump every venue's photo from the game paks as JPEGs: `<venue>.jpg` at full size (1920x1080,
//! the site's banner) and `<venue>-card.jpg` 640 wide (the track cards, twice their CSS width for
//! high-DPI screens). Venue keys are layout keys without the layout (`kanto_c` -> `kanto`).
//!
//! The photos only change with the game, so this runs once: the site keeps the output as static
//! files (`site/public/tracks/`), and `rta-export` and the uploads never touch them.

use ride4_data::{tracks, Game};
use std::path::{Path, PathBuf};

const USAGE: &str = "rta-photos --out DIR [--game DIR]";
const CARD_WIDTH: u32 = 640;

type Res<T> = Result<T, Box<dyn std::error::Error>>;

fn write_jpeg(p: &Path, img: &image::RgbImage) -> Res<()> {
    let w = std::io::BufWriter::new(std::fs::File::create(p)?);
    image::codecs::jpeg::JpegEncoder::new_with_quality(w, 82).encode_image(img)?;
    Ok(())
}

fn main() -> Res<()> {
    let (mut out, mut game) = (None, export::Options::new(PathBuf::new()).game);
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match (arg.as_str(), it.next()) {
            ("--out", Some(v)) => out = Some(PathBuf::from(v)),
            ("--game", Some(v)) => game = PathBuf::from(v),
            _ => out = None,
        }
    }
    let Some(out) = out else {
        eprintln!("{USAGE}");
        std::process::exit(2)
    };
    std::fs::create_dir_all(&out)?;
    let photos = tracks::backgrounds(&Game::open(&game)?)?;
    let mut bytes = 0;
    for (venue, img) in &photos {
        let rgba = image::RgbaImage::from_raw(img.width, img.height, img.rgba.clone()).ok_or("bad photo size")?;
        let full = image::DynamicImage::ImageRgba8(rgba).into_rgb8();
        let h = (img.height * CARD_WIDTH).div_ceil(img.width);
        let card = image::imageops::resize(&full, CARD_WIDTH, h, image::imageops::FilterType::Lanczos3);
        for (name, im) in [(format!("{venue}.jpg"), &full), (format!("{venue}-card.jpg"), &card)] {
            let p = out.join(name);
            write_jpeg(&p, im)?;
            bytes += std::fs::metadata(&p)?.len();
        }
    }
    println!("{} venue photos, {} KB -> {}", photos.len(), bytes / 1024, out.display());
    Ok(())
}
