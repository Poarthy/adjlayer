mod fx;
mod model;
mod nv;
mod video;

use model::{AdjustmentLayer, Effect};
use std::path::Path;
use std::process::exit;

const USAGE: &str = "\
adjlayer - apply a NodeVideo-style Adjustment Layer to a video or image

USAGE
  adjlayer convert <project.nv> [-o out.adjust.json]
      Strip a raw NodeVideo project down to the clean adjustment-layer JSON.
  adjlayer info <layer.nv|layer.adjust.json>
      List the effects in a layer.
  adjlayer apply <layer.nv|layer.adjust.json> <input> <output> [--crf N] [--preset NAME]
      Render. Output .png/.jpg = single frame; anything else = H.264 video
      (audio is copied untouched).  Defaults: --crf 17 --preset medium.
  adjlayer compare <a.png> <b.png>
      Numeric difference between two images (first frame), for calibration.

Requires ffmpeg and ffprobe on PATH (Termux: pkg install ffmpeg).";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(|s| s.as_str()) {
        Some("convert") => cmd_convert(&args[1..]),
        Some("info") => cmd_info(&args[1..]),
        Some("apply") => cmd_apply(&args[1..]),
        Some("compare") => cmd_compare(&args[1..]),
        _ => {
            eprintln!("{USAGE}");
            exit(2);
        }
    };
    if let Err(e) = result {
        eprintln!("error: {e}");
        exit(1);
    }
}

/// Accepts either the clean format or a raw .nv; returns import warnings too.
fn load_layer(path: &str) -> Result<(AdjustmentLayer, Vec<String>), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))?;
    let text = text.trim_start_matches('\u{feff}');
    let v: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("{path} is not valid JSON: {e}"))?;
    if v.get("format").and_then(|f| f.as_str()) == Some(model::FORMAT) {
        let layer: AdjustmentLayer =
            serde_json::from_value(v).map_err(|e| format!("bad adjustment-layer file: {e}"))?;
        if layer.version > model::VERSION {
            return Err(format!(
                "file is format version {}, this build understands up to {}",
                layer.version,
                model::VERSION
            ));
        }
        Ok((layer, Vec::new()))
    } else {
        nv::import(&v)
    }
}

fn cmd_convert(args: &[String]) -> Result<(), String> {
    let mut input: Option<&str> = None;
    let mut output: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-o" => {
                i += 1;
                output = Some(args.get(i).ok_or("-o needs a path")?.clone());
            }
            a => input = Some(a),
        }
        i += 1;
    }
    let input = input.ok_or("convert: missing input file")?;
    let (layer, warnings) = load_layer(input)?;
    for w in &warnings {
        eprintln!("warning: {w}");
    }
    let out = output.unwrap_or_else(|| {
        let stem = Path::new(input).file_stem().and_then(|s| s.to_str()).unwrap_or("layer");
        let dir = Path::new(input).parent().map(|p| p.to_path_buf()).unwrap_or_default();
        dir.join(format!("{stem}.adjust.json")).to_string_lossy().to_string()
    });
    let json = serde_json::to_string_pretty(&layer).map_err(|e| e.to_string())?;
    std::fs::write(&out, json).map_err(|e| format!("cannot write {out}: {e}"))?;
    println!("wrote {out}: {} effects", layer.effects.len());
    Ok(())
}

fn cmd_info(args: &[String]) -> Result<(), String> {
    let path = args.first().ok_or("info: missing layer file")?;
    let (layer, warnings) = load_layer(path)?;
    println!("{}  (apply_to: {})", layer.name, layer.apply_to);
    for (i, e) in layer.effects.iter().enumerate() {
        let (name, on) = match e {
            Effect::Glow(x) => ("glow", x.enabled),
            Effect::ColorGrading(x) => ("color_grading", x.enabled),
            Effect::ShiftChannels(x) => ("shift_channels", x.enabled),
            Effect::ColorCurve(x) => ("color_curve", x.enabled),
            Effect::UnsharpMask(x) => ("unsharp_mask", x.enabled),
        };
        println!("  {}. {:<15} {}", i + 1, name, if on { "on" } else { "off" });
    }
    for w in &warnings {
        println!("warning: {w}");
    }
    let pipe = fx::Pipeline::build(&layer);
    for n in &pipe.notes {
        println!("note: {n}");
    }
    Ok(())
}

fn cmd_apply(args: &[String]) -> Result<(), String> {
    let mut pos: Vec<&str> = Vec::new();
    let mut crf = 17u32;
    let mut preset = "medium".to_string();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--crf" => {
                i += 1;
                crf = args
                    .get(i)
                    .and_then(|v| v.parse().ok())
                    .ok_or("--crf needs a number (0-51)")?;
            }
            "--preset" => {
                i += 1;
                preset = args.get(i).ok_or("--preset needs a name")?.clone();
            }
            a => pos.push(a),
        }
        i += 1;
    }
    if pos.len() != 3 {
        return Err("apply: expected <layer> <input> <output>".into());
    }
    let (layer, warnings) = load_layer(pos[0])?;
    for w in &warnings {
        eprintln!("warning: {w}");
    }
    let pipe = fx::Pipeline::build(&layer);
    for n in &pipe.notes {
        eprintln!("note: {n}");
    }
    video::apply(&pipe, pos[1], pos[2], crf, &preset)
}

fn cmd_compare(args: &[String]) -> Result<(), String> {
    if args.len() != 2 {
        return Err("compare: expected two image paths".into());
    }
    let (w1, h1, a) = video::decode_first_frame(&args[0])?;
    let (w2, h2, b) = video::decode_first_frame(&args[1])?;
    if (w1, h1) != (w2, h2) {
        return Err(format!("size mismatch: {w1}x{h1} vs {w2}x{h2}"));
    }
    let mut sum = [0f64; 3];
    let mut sq = [0f64; 3];
    let mut max = [0u8; 3];
    let mut worst = (0usize, 0usize, 0u8);
    for (i, (pa, pb)) in a.chunks_exact(3).zip(b.chunks_exact(3)).enumerate() {
        for c in 0..3 {
            let d = (pa[c] as i32 - pb[c] as i32).unsigned_abs() as u8;
            sum[c] += d as f64;
            sq[c] += (d as f64) * (d as f64);
            if d > max[c] {
                max[c] = d;
            }
            if d > worst.2 {
                worst = (i % w1, i / w1, d);
            }
        }
    }
    let n = (w1 * h1) as f64;
    let names = ["R", "G", "B"];
    for c in 0..3 {
        println!(
            "{}: mean abs diff {:.3}   rmse {:.3}   max {}   (0-255 scale)",
            names[c],
            sum[c] / n,
            (sq[c] / n).sqrt(),
            max[c]
        );
    }
    println!("largest single difference {} at x={}, y={}", worst.2, worst.0, worst.1);
    Ok(())
}
