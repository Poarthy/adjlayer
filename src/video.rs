//! Video I/O through the `ffmpeg` / `ffprobe` command-line tools
//! (Termux: `pkg install ffmpeg`).  Frames travel as raw rgb24 over pipes.

use crate::fx::{Frame, Pipeline};
use serde_json::Value;
use std::io::{Read, Write};
use std::process::{Child, Command, Stdio};
use std::time::Instant;

pub struct Probe {
    pub w: usize,
    pub h: usize,
    pub fps: String,
    /// "bt709" | "bt601" | "bt2020"
    pub matrix: String,
    /// "limited" | "full"
    pub range: String,
    pub has_audio: bool,
}

fn run_ffprobe(args: &[&str]) -> Result<String, String> {
    let out = Command::new("ffprobe")
        .args(args)
        .output()
        .map_err(|e| format!("cannot run ffprobe ({e}). On Termux: pkg install ffmpeg"))?;
    if !out.status.success() {
        return Err(format!("ffprobe failed: {}", String::from_utf8_lossy(&out.stderr)));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

pub fn probe(path: &str) -> Result<Probe, String> {
    let j = run_ffprobe(&["-v", "error", "-select_streams", "v:0", "-show_streams", "-of", "json", path])?;
    let v: Value = serde_json::from_str(&j).map_err(|e| format!("bad ffprobe output: {e}"))?;
    let s = v
        .get("streams")
        .and_then(|a| a.get(0))
        .ok_or("no video stream found in input")?;

    let mut w = s.get("width").and_then(|x| x.as_u64()).ok_or("no width")? as usize;
    let mut h = s.get("height").and_then(|x| x.as_u64()).ok_or("no height")? as usize;

    // ffmpeg auto-rotates phone videos, so decoded frames have swapped size.
    let mut rot = 0i64;
    if let Some(sd) = s.get("side_data_list").and_then(|a| a.as_array()) {
        for d in sd {
            if let Some(r) = d.get("rotation").and_then(|x| x.as_i64()) {
                rot = r;
            }
        }
    }
    if let Some(t) = s.get("tags").and_then(|t| t.get("rotate")).and_then(|x| x.as_str()) {
        if let Ok(r) = t.parse::<i64>() {
            rot = r;
        }
    }
    if rot.rem_euclid(180) == 90 {
        std::mem::swap(&mut w, &mut h);
    }

    let text = |k: &str| s.get(k).and_then(|x| x.as_str()).map(|x| x.to_string());
    let fps = text("avg_frame_rate")
        .filter(|x| x != "0/0")
        .or_else(|| text("r_frame_rate").filter(|x| x != "0/0"))
        .unwrap_or_else(|| "30/1".to_string());

    let matrix = match text("color_space").as_deref() {
        Some("bt709") => "bt709",
        Some("bt470bg") | Some("smpte170m") | Some("bt601") => "bt601",
        Some("bt2020nc") | Some("bt2020c") => "bt2020",
        _ => {
            if w.max(h) >= 1280 { "bt709" } else { "bt601" }
        }
    }
    .to_string();
    let range = match text("color_range").as_deref() {
        Some("pc") | Some("jpeg") => "full",
        _ => "limited",
    }
    .to_string();

    let a = run_ffprobe(&["-v", "error", "-select_streams", "a", "-show_entries", "stream=index", "-of", "csv=p=0", path])?;
    Ok(Probe { w, h, fps, matrix, range, has_audio: !a.trim().is_empty() })
}

fn ffmpeg_err(e: std::io::Error) -> String {
    format!("cannot run ffmpeg ({e}). On Termux: pkg install ffmpeg")
}

fn spawn_decoder(input: &str, p: &Probe) -> Result<Child, String> {
    let vf = format!("scale=in_color_matrix={}:in_range={},format=rgb24", p.matrix, p.range);
    let args: Vec<&str> = vec![
        "-v", "error", "-nostdin", "-i", input, "-map", "0:v:0", "-vf", vf.as_str(),
        "-f", "rawvideo", "-pix_fmt", "rgb24", "-",
    ];
    Command::new("ffmpeg")
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(ffmpeg_err)
}

fn colour_tags(matrix: &str) -> (&'static str, &'static str, &'static str) {
    match matrix {
        "bt601" => ("smpte170m", "smpte170m", "smpte170m"),
        "bt2020" => ("bt2020nc", "bt2020", "bt709"),
        _ => ("bt709", "bt709", "bt709"),
    }
}

fn is_still_image(output: &str) -> bool {
    let ext = output.rsplit('.').next().unwrap_or("").to_lowercase();
    matches!(ext.as_str(), "png" | "bmp" | "tif" | "tiff" | "webp" | "jpg" | "jpeg")
}

fn spawn_encoder(input: &str, output: &str, p: &Probe, crf: u32, preset: &str) -> Result<Child, String> {
    let mut a: Vec<String> = Vec::new();
    let mut push = |xs: &[&str]| {
        for x in xs {
            a.push(x.to_string());
        }
    };
    push(&["-v", "error", "-nostdin", "-y", "-f", "rawvideo", "-pix_fmt", "rgb24"]);
    push(&["-s", &format!("{}x{}", p.w, p.h), "-framerate", &p.fps, "-i", "-"]);

    if is_still_image(output) {
        push(&["-frames:v", "1", output]);
    } else {
        let (cs, cp, ct) = colour_tags(&p.matrix);
        if p.has_audio {
            push(&["-i", input]);
        }
        push(&["-map", "0:v:0"]);
        if p.has_audio {
            push(&["-map", "1:a:0", "-c:a", "copy"]);
        }
        push(&["-vf", &format!("scale=out_color_matrix={}:out_range=limited,format=yuv420p", p.matrix)]);
        push(&["-c:v", "libx264", "-crf", &crf.to_string(), "-preset", preset]);
        push(&["-colorspace", cs, "-color_primaries", cp, "-color_trc", ct, "-color_range", "tv"]);
        push(&["-movflags", "+faststart", output]);
    }

    Command::new("ffmpeg")
        .args(&a)
        .stdin(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(ffmpeg_err)
}

/// Returns Ok(false) on clean EOF before the first byte of a frame.
fn read_frame(r: &mut impl Read, buf: &mut [u8]) -> Result<bool, String> {
    let mut got = 0;
    while got < buf.len() {
        let n = r.read(&mut buf[got..]).map_err(|e| e.to_string())?;
        if n == 0 {
            return if got == 0 { Ok(false) } else { Err("truncated frame from decoder".into()) };
        }
        got += n;
    }
    Ok(true)
}

fn pump(
    rd: &mut impl Read,
    wr: &mut impl Write,
    pipeline: &Pipeline,
    p: &Probe,
) -> Result<u64, String> {
    let mut buf = vec![0u8; p.w * p.h * 3];
    let mut frame = Frame::new(p.w, p.h);
    let t0 = Instant::now();
    let mut count = 0u64;
    while read_frame(rd, &mut buf)? {
        frame.load_rgb8(&buf);
        pipeline.run(&mut frame);
        frame.store_rgb8(&mut buf);
        wr.write_all(&buf).map_err(|e| format!("encoder closed early: {e}"))?;
        count += 1;
        if count % 15 == 0 {
            eprint!("\r{count} frames  ({:.1} fps)   ", count as f64 / t0.elapsed().as_secs_f64());
        }
    }
    eprintln!("\r{count} frames done in {:.1}s        ", t0.elapsed().as_secs_f64());
    Ok(count)
}

pub fn apply(pipeline: &Pipeline, input: &str, output: &str, crf: u32, preset: &str) -> Result<(), String> {
    let p = probe(input)?;
    eprintln!(
        "input: {}x{} @ {}  matrix={} range={} audio={}",
        p.w, p.h, p.fps, p.matrix, p.range, p.has_audio
    );
    if p.matrix == "bt2020" {
        eprintln!("warning: BT.2020/HDR input detected; this tool assumes SDR, colours will be off");
    }
    let mut dec = spawn_decoder(input, &p)?;
    let mut enc = spawn_encoder(input, output, &p, crf, preset)?;
    let mut rd = dec.stdout.take().ok_or("decoder has no stdout")?;
    let mut wr = enc.stdin.take().ok_or("encoder has no stdin")?;

    let res = pump(&mut rd, &mut wr, pipeline, &p);
    drop(wr); // closes the pipe so the encoder can finish
    if res.is_err() {
        let _ = dec.kill();
    }
    let es = enc.wait().map_err(|e| e.to_string())?;
    let ds = dec.wait().map_err(|e| e.to_string())?;
    let frames = res?;
    if !es.success() {
        return Err("ffmpeg encoder reported an error".into());
    }
    if !ds.success() {
        return Err("ffmpeg decoder reported an error".into());
    }
    if frames == 0 {
        return Err("no frames were decoded".into());
    }
    Ok(())
}

/// First frame as (width, height, rgb24 bytes).
pub fn decode_first_frame(path: &str) -> Result<(usize, usize, Vec<u8>), String> {
    let p = probe(path)?;
    let mut dec = spawn_decoder(path, &p)?;
    let mut rd = dec.stdout.take().ok_or("decoder has no stdout")?;
    let mut buf = vec![0u8; p.w * p.h * 3];
    let ok = read_frame(&mut rd, &mut buf);
    drop(rd);
    let _ = dec.kill();
    let _ = dec.wait();
    if !ok? {
        return Err(format!("no frame in {path}"));
    }
    Ok((p.w, p.h, buf))
}
