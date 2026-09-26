//! Visual baselines: PNG captures compared with a tolerance.
//!
//! Baselines live in `<crate>/tests/visual/<backend>/<image>/`. Native
//! rendering differs between OS versions and display scales, so baselines
//! belong to the machine image that produced them: CI's pinned runners, or
//! a developer's OS (see `snapshot::image`).
//!
//! Same rules as text snapshots: missing baselines are created (not on CI),
//! mismatches fail and write `<name>.new.png` plus `<name>.diff.png` (on CI
//! too, which uploads them), `MITSUAMI_UPDATE_SNAPSHOTS=1` accepts changes,
//! and `MITSUAMI_SKIP_MACHINE_SNAPSHOTS=1` skips them all.

use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};

use mitsuami_core::backend::Image;

use crate::app::TestContext;

/// A channel may differ this much before a pixel counts as changed
/// (absorbs anti-aliasing and subpixel noise).
const CHANNEL_TOLERANCE: u8 = 24;
/// Fraction of changed pixels allowed.
const MAX_CHANGED_RATIO: f64 = 0.001;

fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).expect("create baseline directory");
    let file = BufWriter::new(File::create(path).expect("create png"));
    let mut encoder = png::Encoder::new(file, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().and_then(|mut w| w.write_image_data(rgba)).expect("write png");
}

fn read_png(path: &Path) -> Option<(u32, u32, Vec<u8>)> {
    let file = BufReader::new(File::open(path).ok()?);
    let mut decoder = png::Decoder::new(file);
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    (info.color_type == png::ColorType::Rgba).then_some((info.width, info.height, buf))
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let stem = path.file_stem().unwrap().to_string_lossy();
    path.with_file_name(format!("{stem}.{suffix}.png"))
}

#[track_caller]
pub(crate) fn assert(context: &TestContext, machine: &Path, name: &str, image: &Image) {
    if crate::snapshot::skip_machine_snapshots() {
        return;
    }
    let file = PathBuf::from(context.manifest_dir).join("tests").join("visual").join(machine).join(format!(
        "{}@{}.png",
        context.file_prefix,
        crate::snapshot::sanitize(name)
    ));
    let pending = sibling(&file, "new");
    let diff_path = sibling(&file, "diff");
    let update = crate::snapshot::updating();
    let ci = crate::snapshot::on_ci();

    let Some((width, height, expected)) = read_png(&file) else {
        if ci && !update {
            write_png(&pending, image.width, image.height, &image.rgba);
            context.fail_snapshot(format!(
                "visual baseline {} is missing (not created on CI); the capture is in {}",
                file.display(),
                pending.display()
            ));
            return;
        }
        write_png(&file, image.width, image.height, &image.rgba);
        eprintln!("mitsuami-test: created visual baseline {}", file.display());
        return;
    };

    let problem = if (width, height) != (image.width, image.height) {
        Some(format!("size changed from {width}×{height} to {}×{}", image.width, image.height))
    } else {
        let mut diff = Vec::with_capacity(expected.len());
        let mut changed = 0usize;
        for (a, b) in expected.chunks_exact(4).zip(image.rgba.chunks_exact(4)) {
            let delta = a.iter().zip(b).map(|(x, y)| x.abs_diff(*y)).max().unwrap_or(0);
            if delta > CHANNEL_TOLERANCE {
                changed += 1;
                diff.extend_from_slice(&[255, 0, 0, 255]);
            } else {
                // Faded original, so the red changes stand out.
                diff.extend(b[..3].iter().map(|c| 255 - (255 - c) / 4));
                diff.push(255);
            }
        }
        let ratio = changed as f64 / (width as f64 * height as f64);
        (ratio > MAX_CHANGED_RATIO).then(|| {
            write_png(&diff_path, width, height, &diff);
            format!("{changed} pixels changed ({:.3}%)", ratio * 100.0)
        })
    };

    match problem {
        None => {
            let _ = std::fs::remove_file(&pending);
            let _ = std::fs::remove_file(&diff_path);
        }
        Some(_) if update => {
            write_png(&file, image.width, image.height, &image.rgba);
            let _ = std::fs::remove_file(&pending);
            let _ = std::fs::remove_file(&diff_path);
        }
        Some(problem) => {
            write_png(&pending, image.width, image.height, &image.rgba);
            context.fail_snapshot(format!(
                "visual baseline {} does not match: {problem}\nnew: {}\ndiff: {}",
                file.display(),
                pending.display(),
                diff_path.display()
            ));
        }
    }
}
