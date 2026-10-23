//! `bridge-testbed compare --base DIR1 --new DIR2`: per-pixel diff of two shot runs.
//!
//! A pixel differs when any colour channel differs by more than `--tolerance` (of 255). An item
//! regresses when the fraction of differing pixels is above `--threshold`, when its geometry numbers
//! in `manifest.json` changed, or when it went missing or began to fail.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::shots::{Entry, Manifest};

pub struct Args {
    pub base: PathBuf,
    pub new: PathBuf,
    /// Allowed fraction of differing pixels (0.01 = 1 %)
    pub threshold: f32,
    /// Largest channel difference (of 255) that still counts as the same
    pub tolerance: u8,
    pub diff_out: PathBuf,
}

pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

pub fn read_png(path: &Path) -> Result<Rgba> {
    let file = std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info()?;
    let mut buf = vec![0; reader.output_buffer_size().context("png too large")?];
    let info = reader.next_frame(&mut buf)?;
    buf.truncate(info.buffer_size());
    let data = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => buf
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|g| [*g, *g, *g, 255]).collect(),
        other => bail!("{}: unsupported colour type {other:?}", path.display()),
    };
    Ok(Rgba {
        width: info.width,
        height: info.height,
        data,
    })
}

pub fn write_png(path: &Path, img: &Rgba) -> Result<()> {
    let file =
        std::fs::File::create(path).with_context(|| format!("creating {}", path.display()))?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), img.width, img.height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(&img.data)?;
    Ok(())
}

#[derive(Debug, PartialEq)]
pub struct Diff {
    pub fraction: f32,
    pub max: u8,
    pub mean: f32,
}

/// Compares two images of the same size; the second result is the diff picture
pub fn diff(a: &Rgba, b: &Rgba, tolerance: u8) -> (Diff, Rgba) {
    let pixels = (a.width * a.height) as usize;
    let mut out = Vec::with_capacity(pixels * 4);
    let (mut differing, mut max, mut sum) = (0usize, 0u8, 0u64);
    for (pa, pb) in a
        .data
        .as_chunks::<4>()
        .0
        .iter()
        .zip(b.data.as_chunks::<4>().0.iter())
    {
        let d = (0..3).map(|k| pa[k].abs_diff(pb[k])).max().unwrap_or(0);
        sum += (0..3)
            .map(|k| u64::from(pa[k].abs_diff(pb[k])))
            .sum::<u64>();
        max = max.max(d);
        if d > tolerance {
            differing += 1;
        }
        // The base, dimmed to grey, with the difference in red
        let grey = ((u32::from(pa[0]) + u32::from(pa[1]) + u32::from(pa[2])) / 3 * 3 / 10) as u8;
        out.extend_from_slice(&[grey.saturating_add(d.saturating_mul(4)), grey, grey, 255]);
    }
    (
        Diff {
            fraction: differing as f32 / pixels.max(1) as f32,
            max,
            mean: sum as f32 / (pixels.max(1) * 3) as f32,
        },
        Rgba {
            width: a.width,
            height: a.height,
            data: out,
        },
    )
}

fn read_manifest(dir: &Path) -> Result<Manifest> {
    let p = dir.join("manifest.json");
    let text = std::fs::read_to_string(&p).with_context(|| format!("reading {}", p.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parsing {}", p.display()))
}

/// What differs in the numbers of two entries
pub fn geometry_changes(a: &Entry, b: &Entry) -> Vec<String> {
    let mut out = Vec::new();
    for (name, x, y) in [
        ("vertices", a.vertices, b.vertices),
        ("triangles", a.triangles, b.triangles),
        ("instances", a.instances, b.instances),
        ("clips", a.clips, b.clips),
    ] {
        if x != y {
            out.push(format!("{name} {x} -> {y}"));
        }
    }
    if let (Some(x), Some(y)) = (a.aabb_size, b.aabb_size) {
        if (0..3).any(|k| (x[k] - y[k]).abs() > 1e-4) {
            out.push(format!("AABB size {x:?} -> {y:?}"));
        }
    }
    out
}

/// Runs the comparison, prints the table and returns the number of regressions
pub fn run(args: &Args) -> Result<usize> {
    let base = read_manifest(&args.base)?;
    let new = read_manifest(&args.new)?;
    std::fs::create_dir_all(&args.diff_out)?;
    let mut labels: Vec<&str> = base
        .items
        .iter()
        .chain(&new.items)
        .map(|e| e.label.as_str())
        .collect();
    labels.sort();
    labels.dedup();
    println!("item                         differing   max    mean  result");
    let mut regressions = 0;
    for label in labels {
        let a = base.items.iter().find(|e| e.label == label);
        let b = new.items.iter().find(|e| e.label == label);
        let (verdict, regression, row) = match (a, b) {
            (Some(_), None) => ("MISSING from the new run".to_string(), true, None),
            (None, Some(_)) => ("new item".to_string(), false, None),
            (Some(a), Some(b)) => compare_entry(args, a, b),
            (None, None) => continue,
        };
        let (frac, max, mean) = row.map_or(("-".into(), "-".into(), "-".into()), |d: Diff| {
            (
                format!("{:.3}%", d.fraction * 100.0),
                d.max.to_string(),
                format!("{:.3}", d.mean),
            )
        });
        println!(
            "{label:<28} {frac:>9} {max:>5} {mean:>7}  {verdict}{}",
            if regression { "  <-- REGRESSION" } else { "" }
        );
        if regression {
            regressions += 1;
        }
    }
    println!("{regressions} regressions");
    Ok(regressions)
}

fn compare_entry(args: &Args, a: &Entry, b: &Entry) -> (String, bool, Option<Diff>) {
    match (a.status == "ok", b.status == "ok") {
        (false, false) => return ("fails in both".into(), false, None),
        (false, true) => return ("loads now".into(), false, None),
        (true, false) => {
            return (
                format!("now fails: {}", b.error.clone().unwrap_or_default()),
                true,
                None,
            )
        }
        (true, true) => {}
    }
    let changes = geometry_changes(a, b);
    let load = |dir: &Path, e: &Entry| read_png(&dir.join(e.png.as_deref().unwrap_or("")));
    let (ia, ib) = match (load(&args.base, a), load(&args.new, b)) {
        (Ok(x), Ok(y)) => (x, y),
        (Err(e), _) | (_, Err(e)) => return (format!("cannot read: {e:#}"), true, None),
    };
    if (ia.width, ia.height) != (ib.width, ib.height) {
        return (
            format!(
                "size {}x{} -> {}x{}",
                ia.width, ia.height, ib.width, ib.height
            ),
            true,
            None,
        );
    }
    let (d, picture) = diff(&ia, &ib, args.tolerance);
    if d.max > 0 {
        let _ = write_png(&args.diff_out.join(format!("{}.png", a.label)), &picture);
    }
    let pixel_regression = d.fraction > args.threshold;
    let mut notes = Vec::new();
    if pixel_regression {
        notes.push(format!(
            "{:.2}% of pixels differ, over {:.2}%",
            d.fraction * 100.0,
            args.threshold * 100.0
        ));
    }
    notes.extend(changes.iter().map(|c| format!("geometry: {c}")));
    let regression = pixel_regression || !changes.is_empty();
    (
        if notes.is_empty() {
            "same".into()
        } else {
            notes.join("; ")
        },
        regression,
        Some(d),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(v: u8) -> Rgba {
        Rgba {
            width: 4,
            height: 4,
            data: [v, v, v, 255].repeat(16),
        }
    }

    #[test]
    fn identical_images_do_not_differ() {
        let (d, _) = diff(&flat(10), &flat(10), 0);
        assert_eq!(
            d,
            Diff {
                fraction: 0.0,
                max: 0,
                mean: 0.0
            }
        );
    }

    #[test]
    fn tolerance_and_fraction() {
        let a = flat(100);
        let mut b = flat(100);
        b.data[0] = 110; // one pixel, 10 off
        let (d, pic) = diff(&a, &b, 4);
        assert_eq!(d.max, 10);
        assert!((d.fraction - 1.0 / 16.0).abs() < 1e-6);
        assert_eq!(pic.data.len(), a.data.len());
        let (d, _) = diff(&a, &b, 10);
        assert_eq!(d.fraction, 0.0);
    }

    #[test]
    fn png_round_trip() {
        let dir = std::env::temp_dir().join(format!("bt-png-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let img = flat(77);
        write_png(&dir.join("a.png"), &img).unwrap();
        let back = read_png(&dir.join("a.png")).unwrap();
        assert_eq!(back.data, img.data);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn geometry_changes_are_named() {
        let a = Entry {
            vertices: 10,
            aabb_size: Some([1.0, 2.0, 3.0]),
            ..Entry::default()
        };
        let b = Entry {
            vertices: 12,
            aabb_size: Some([1.0, 2.5, 3.0]),
            ..Entry::default()
        };
        let c = geometry_changes(&a, &b);
        assert_eq!(c.len(), 2);
        assert!(c[0].contains("vertices 10 -> 12"));
    }
}
