//! CPU renderer for the effect stack.  All maths in f32, RGB in 0..1.
//!
//! IMPORTANT: NodeVideo's shaders are proprietary.  The algorithms below are
//! standard ones; every place where NodeVideo's exact behaviour is a guess is
//! marked `CALIBRATE:`.  Use `adjlayer compare` against frames exported from
//! the app to tune those spots (see README.md).

use crate::model::*;
use rayon::prelude::*;

const LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722]; // CALIBRATE: Rec.709 weights
const LUT_SIZE: usize = 1024;
const MAX_GLOW_LEVELS: usize = 6;

// ------------------------------------------------------------------ frame

pub struct Frame {
    pub w: usize,
    pub h: usize,
    pub px: Vec<f32>, // RGB interleaved
}

impl Frame {
    pub fn new(w: usize, h: usize) -> Frame {
        Frame { w, h, px: vec![0.0; w * h * 3] }
    }

    pub fn load_rgb8(&mut self, bytes: &[u8]) {
        self.px
            .par_iter_mut()
            .zip(bytes.par_iter())
            .for_each(|(d, &s)| *d = s as f32 / 255.0);
    }

    pub fn store_rgb8(&self, out: &mut [u8]) {
        out.par_iter_mut()
            .zip(self.px.par_iter())
            .for_each(|(d, &s)| *d = (s.clamp(0.0, 1.0) * 255.0 + 0.5) as u8);
    }
}

// --------------------------------------------------------------- pipeline

#[derive(Clone, Copy, PartialEq)]
enum Source {
    Red,
    Green,
    Blue,
    Alpha,
    Luminance,
    Hue,
    Lightness,
    Saturation,
    FullOn,
    FullOff,
}

fn parse_source(s: &str) -> Option<Source> {
    Some(match s {
        "red" => Source::Red,
        "green" => Source::Green,
        "blue" => Source::Blue,
        "alpha" => Source::Alpha,
        "luminance" => Source::Luminance,
        "hue" => Source::Hue,
        "lightness" => Source::Lightness,
        "saturation" => Source::Saturation,
        "full_on" => Source::FullOn,
        "full_off" => Source::FullOff,
        _ => return None,
    })
}

struct Grade {
    gain: [f32; 3],
    mixer: Option<[[f32; 3]; 3]>,
    contrast: f32,
    saturation: f32,
    lut_rgb: Option<Vec<f32>>,
    lut_ch: Option<[Vec<f32>; 3]>,
}

enum Stage {
    Glow(Glow),
    Grade(Box<Grade>),
    Shift([Source; 3]),
    Curves(Box<[Vec<f32>; 3]>),
    Unsharp { amount: f32, sigma: f32, threshold: f32 },
}

pub struct Pipeline {
    stages: Vec<Stage>,
    /// Human-readable remarks (unsupported / ignored settings).
    pub notes: Vec<String>,
}

impl Pipeline {
    pub fn build(layer: &AdjustmentLayer) -> Pipeline {
        let mut stages = Vec::new();
        let mut notes = Vec::new();

        for e in &layer.effects {
            match e {
                Effect::Glow(g) if g.enabled => {
                    if g.intensity > 0.0 {
                        stages.push(Stage::Glow(g.clone()));
                    }
                }
                Effect::ColorGrading(c) if c.enabled => {
                    stages.push(Stage::Grade(Box::new(make_grade(c, &mut notes))));
                }
                Effect::ShiftChannels(s) if s.enabled => {
                    let src = [parse_source(&s.r), parse_source(&s.g), parse_source(&s.b)];
                    match src {
                        [Some(r), Some(g), Some(b)] => {
                            if !(r == Source::Red && g == Source::Green && b == Source::Blue) {
                                stages.push(Stage::Shift([r, g, b]));
                            }
                        }
                        _ => notes.push("shift_channels: unknown source name, effect skipped".into()),
                    }
                    if s.a != "alpha" {
                        notes.push("shift_channels: alpha remapping ignored (video has no alpha)".into());
                    }
                }
                Effect::ColorCurve(c) if c.enabled => {
                    let master = if is_identity(&c.master) { identity_lut() } else { pchip_lut(&c.master, LUT_SIZE) };
                    let chan = |k: &Curve| -> Vec<f32> {
                        if is_identity(k) { identity_lut() } else { pchip_lut(k, LUT_SIZE) }
                    };
                    // CALIBRATE: order is channel(master(x)); NodeVideo may differ.
                    let compose = |ch: Vec<f32>| -> Vec<f32> { master.iter().map(|&m| lut(&ch, m)).collect() };
                    let luts = [compose(chan(&c.r)), compose(chan(&c.g)), compose(chan(&c.b))];
                    stages.push(Stage::Curves(Box::new(luts)));
                }
                Effect::UnsharpMask(u) if u.enabled => {
                    if u.amount != 0.0 && u.radius > 0.0 {
                        stages.push(Stage::Unsharp {
                            amount: u.amount,
                            sigma: u.radius, // CALIBRATE: radius -> gaussian sigma (px)
                            threshold: u.threshold,
                        });
                    }
                }
                _ => {}
            }
        }
        Pipeline { stages, notes }
    }

    pub fn run(&self, f: &mut Frame) {
        for s in &self.stages {
            match s {
                Stage::Glow(g) => glow(g, f),
                Stage::Grade(g) => {
                    let w = f.w;
                    f.px.par_chunks_mut(w * 3).for_each(|row| {
                        for p in row.chunks_exact_mut(3) {
                            grade_px(g, p);
                        }
                    });
                }
                Stage::Shift(src) => {
                    let w = f.w;
                    f.px.par_chunks_mut(w * 3).for_each(|row| {
                        for p in row.chunks_exact_mut(3) {
                            let (r, g, b) = (p[0], p[1], p[2]);
                            let (h, s, l) = hsl(r, g, b);
                            let lum = LUMA[0] * r + LUMA[1] * g + LUMA[2] * b;
                            let pick = |s_: Source| match s_ {
                                Source::Red => r,
                                Source::Green => g,
                                Source::Blue => b,
                                Source::Alpha => 1.0,
                                Source::Luminance => lum,
                                Source::Hue => h,
                                Source::Lightness => l,
                                Source::Saturation => s,
                                Source::FullOn => 1.0,
                                Source::FullOff => 0.0,
                            };
                            p[0] = pick(src[0]);
                            p[1] = pick(src[1]);
                            p[2] = pick(src[2]);
                        }
                    });
                }
                Stage::Curves(luts) => {
                    let w = f.w;
                    f.px.par_chunks_mut(w * 3).for_each(|row| {
                        for p in row.chunks_exact_mut(3) {
                            for c in 0..3 {
                                p[c] = lut(&luts[c], p[c]);
                            }
                        }
                    });
                }
                Stage::Unsharp { amount, sigma, threshold } => {
                    let k = gaussian_kernel(*sigma);
                    let blurred = blur(&f.px, f.w, f.h, &k);
                    let (a, t) = (*amount, *threshold);
                    f.px.par_iter_mut().zip(blurred.par_iter()).for_each(|(v, b)| {
                        let d = *v - *b;
                        if d.abs() >= t {
                            *v = (*v + a * d).clamp(0.0, 1.0);
                        }
                    });
                }
            }
        }
    }
}

// ------------------------------------------------------------ colour grade

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

fn neutral_wheel(w: &[f32; 4]) -> bool {
    near(w[0], 1.0) && near(w[1], 1.0) && near(w[2], 1.0) && near(w[3], 0.5)
}

fn flat_half(c: &Curve) -> bool {
    c.iter().all(|p| near(p[1], 0.5))
}

fn is_identity(c: &Curve) -> bool {
    c.is_empty() || c.iter().all(|p| near(p[0], p[1]))
}

fn make_grade(c: &ColorGrading, notes: &mut Vec<String>) -> Grade {
    if !near(c.temperature, 0.0) || !near(c.tint, 0.0) {
        notes.push("color_grading: temperature/tint not implemented, ignored".into());
    }
    if !near(c.hue_shift, 0.0) {
        notes.push("color_grading: hue_shift not implemented, ignored".into());
    }
    if c.tone_mapping != "none" {
        notes.push(format!("color_grading: tone_mapping '{}' not implemented, ignored", c.tone_mapping));
    }
    if !(neutral_wheel(&c.lift) && neutral_wheel(&c.gamma) && neutral_wheel(&c.gain)) {
        notes.push("color_grading: lift/gamma/gain wheels not implemented, ignored".into());
    }
    let hue_curves = [
        &c.curves.hue_vs_hue, &c.curves.hue_vs_sat, &c.curves.sat_vs_sat,
        &c.curves.lum_vs_sat, &c.curves.hue_vs_lum, &c.curves.sat_vs_lum,
    ];
    if !hue_curves.iter().all(|k| flat_half(k)) {
        notes.push("color_grading: hue/sat/lum curves not implemented, ignored".into());
    }

    let expo = 2f32.powf(c.exposure);
    let gain = [expo * c.color_filter[0], expo * c.color_filter[1], expo * c.color_filter[2]];

    let mut m = [[0.0f32; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            m[i][j] = c.channel_mixer[i][j] / 100.0;
        }
    }
    let ident = (0..3).all(|i| (0..3).all(|j| near(m[i][j], if i == j { 1.0 } else { 0.0 })));

    let lut_rgb = if is_identity(&c.curves.rgb) { None } else { Some(pchip_lut(&c.curves.rgb, LUT_SIZE)) };
    let chs = [&c.curves.r, &c.curves.g, &c.curves.b];
    let lut_ch = if chs.iter().all(|k| is_identity(k)) {
        None
    } else {
        Some([
            pchip_lut(chs[0], LUT_SIZE),
            pchip_lut(chs[1], LUT_SIZE),
            pchip_lut(chs[2], LUT_SIZE),
        ])
    };

    Grade {
        gain,
        mixer: if ident { None } else { Some(m) },
        // CALIBRATE: contrast pivot (0.5 in encoded space) and scale (1 + v/100)
        contrast: (1.0 + c.contrast / 100.0).max(0.0),
        // CALIBRATE: saturation in encoded space with Rec.709 luma, scale (1 + v/100)
        saturation: (1.0 + c.saturation / 100.0).max(0.0),
        lut_rgb,
        lut_ch,
    }
}

#[inline]
fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

#[inline]
fn grade_px(g: &Grade, p: &mut [f32]) {
    let mut c = [p[0] * g.gain[0], p[1] * g.gain[1], p[2] * g.gain[2]];
    if let Some(m) = &g.mixer {
        c = [dot3(m[0], c), dot3(m[1], c), dot3(m[2], c)];
    }
    if g.contrast != 1.0 {
        for v in c.iter_mut() {
            *v = (*v - 0.5) * g.contrast + 0.5;
        }
    }
    if g.saturation != 1.0 {
        let l = dot3(LUMA, c);
        for v in c.iter_mut() {
            *v = l + (*v - l) * g.saturation;
        }
    }
    for v in c.iter_mut() {
        *v = v.clamp(0.0, 1.0);
    }
    if let Some(l) = &g.lut_rgb {
        for v in c.iter_mut() {
            *v = lut(l, *v);
        }
    }
    if let Some(ls) = &g.lut_ch {
        for i in 0..3 {
            c[i] = lut(&ls[i], c[i]);
        }
    }
    p[0] = c[0];
    p[1] = c[1];
    p[2] = c[2];
}

fn hsl(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let mx = r.max(g).max(b);
    let mn = r.min(g).min(b);
    let l = (mx + mn) * 0.5;
    let d = mx - mn;
    if d < 1e-6 {
        return (0.0, 0.0, l);
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs()).max(1e-6);
    let h = if mx == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if mx == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (h / 6.0, s.min(1.0), l)
}

// ------------------------------------------------------------------ curves

fn identity_lut() -> Vec<f32> {
    (0..LUT_SIZE).map(|i| i as f32 / (LUT_SIZE - 1) as f32).collect()
}

#[inline]
fn lut(l: &[f32], v: f32) -> f32 {
    let x = v.clamp(0.0, 1.0) * (l.len() - 1) as f32;
    let i = x as usize;
    let j = (i + 1).min(l.len() - 1);
    let t = x - i as f32;
    l[i] + (l[j] - l[i]) * t
}

/// Monotone cubic (Fritsch-Carlson / PCHIP) through the control points.
/// CALIBRATE: NodeVideo's interpolation between curve points is unknown.
fn pchip_lut(points: &[[f32; 2]], n: usize) -> Vec<f32> {
    let mut p: Vec<[f32; 2]> = points.to_vec();
    p.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap_or(std::cmp::Ordering::Equal));
    p.dedup_by(|a, b| (a[0] - b[0]).abs() < 1e-9);
    let m = p.len();
    if m == 0 {
        return identity_lut();
    }
    if m == 1 {
        return vec![p[0][1]; n];
    }
    let h: Vec<f32> = (0..m - 1).map(|i| p[i + 1][0] - p[i][0]).collect();
    let del: Vec<f32> = (0..m - 1).map(|i| (p[i + 1][1] - p[i][1]) / h[i]).collect();
    let mut d = vec![0.0f32; m];
    if m == 2 {
        d[0] = del[0];
        d[1] = del[0];
    } else {
        d[0] = del[0];
        d[m - 1] = del[m - 2];
        for i in 1..m - 1 {
            if del[i - 1] * del[i] > 0.0 {
                let w1 = 2.0 * h[i] + h[i - 1];
                let w2 = h[i] + 2.0 * h[i - 1];
                d[i] = (w1 + w2) / (w1 / del[i - 1] + w2 / del[i]);
            }
        }
    }
    (0..n)
        .map(|k| -> f32 {
            let x = k as f32 / (n - 1) as f32;
            if x <= p[0][0] {
                return p[0][1];
            }
            if x >= p[m - 1][0] {
                return p[m - 1][1];
            }
            let mut i = 0;
            while i + 2 < m && x > p[i + 1][0] {
                i += 1;
            }
            let t = (x - p[i][0]) / h[i];
            let (t2, t3) = (t * t, t * t * t);
            let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
            let h10 = t3 - 2.0 * t2 + t;
            let h01 = -2.0 * t3 + 3.0 * t2;
            let h11 = t3 - t2;
            h00 * p[i][1] + h10 * h[i] * d[i] + h01 * p[i + 1][1] + h11 * h[i] * d[i + 1]
        })
        .collect()
}

// -------------------------------------------------------------------- blur

fn gaussian_kernel(sigma: f32) -> Vec<f32> {
    let sigma = sigma.max(0.1);
    let r = ((sigma * 3.0).ceil() as usize).max(1);
    let mut k: Vec<f32> = (0..=2 * r)
        .map(|i| {
            let x = i as f32 - r as f32;
            (-(x * x) / (2.0 * sigma * sigma)).exp()
        })
        .collect();
    let s: f32 = k.iter().sum();
    for v in k.iter_mut() {
        *v /= s;
    }
    k
}

/// Separable convolution with clamp-to-edge borders. `k` must have odd length.
fn blur(src: &[f32], w: usize, h: usize, k: &[f32]) -> Vec<f32> {
    let r = k.len() / 2;
    let mut tmp = vec![0.0f32; src.len()];
    tmp.par_chunks_mut(w * 3).zip(src.par_chunks(w * 3)).for_each(|(dst, row)| {
        for x in 0..w {
            let mut acc = [0.0f32; 3];
            for (j, kv) in k.iter().enumerate() {
                let xs = (x as isize + j as isize - r as isize).clamp(0, w as isize - 1) as usize;
                acc[0] += kv * row[xs * 3];
                acc[1] += kv * row[xs * 3 + 1];
                acc[2] += kv * row[xs * 3 + 2];
            }
            dst[x * 3..x * 3 + 3].copy_from_slice(&acc);
        }
    });
    let mut out = vec![0.0f32; src.len()];
    out.par_chunks_mut(w * 3).enumerate().for_each(|(y, dst)| {
        for (j, kv) in k.iter().enumerate() {
            let ys = (y as isize + j as isize - r as isize).clamp(0, h as isize - 1) as usize;
            let row = &tmp[ys * w * 3..(ys + 1) * w * 3];
            for i in 0..w * 3 {
                dst[i] += kv * row[i];
            }
        }
    });
    out
}

fn downsample2(src: &[f32], w: usize, h: usize) -> (Vec<f32>, usize, usize) {
    let nw = (w / 2).max(1);
    let nh = (h / 2).max(1);
    let mut out = vec![0.0f32; nw * nh * 3];
    out.par_chunks_mut(nw * 3).enumerate().for_each(|(y, row)| {
        let y0 = (y * 2).min(h - 1);
        let y1 = (y * 2 + 1).min(h - 1);
        for x in 0..nw {
            let x0 = (x * 2).min(w - 1);
            let x1 = (x * 2 + 1).min(w - 1);
            for c in 0..3 {
                row[x * 3 + c] = 0.25
                    * (src[(y0 * w + x0) * 3 + c]
                        + src[(y0 * w + x1) * 3 + c]
                        + src[(y1 * w + x0) * 3 + c]
                        + src[(y1 * w + x1) * 3 + c]);
            }
        }
    });
    (out, nw, nh)
}

fn resize_bilinear(src: &[f32], sw: usize, sh: usize, dw: usize, dh: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; dw * dh * 3];
    let sx = sw as f32 / dw as f32;
    let sy = sh as f32 / dh as f32;
    out.par_chunks_mut(dw * 3).enumerate().for_each(|(y, row)| {
        let fy = ((y as f32 + 0.5) * sy - 0.5).clamp(0.0, (sh - 1) as f32);
        let y0 = fy.floor() as usize;
        let y1 = (y0 + 1).min(sh - 1);
        let ty = fy - y0 as f32;
        for x in 0..dw {
            let fx = ((x as f32 + 0.5) * sx - 0.5).clamp(0.0, (sw - 1) as f32);
            let x0 = fx.floor() as usize;
            let x1 = (x0 + 1).min(sw - 1);
            let tx = fx - x0 as f32;
            for c in 0..3 {
                let a = src[(y0 * sw + x0) * 3 + c];
                let b = src[(y0 * sw + x1) * 3 + c];
                let cc = src[(y1 * sw + x0) * 3 + c];
                let d = src[(y1 * sw + x1) * 3 + c];
                row[x * 3 + c] = (a + (b - a) * tx) * (1.0 - ty) + (cc + (d - cc) * tx) * ty;
            }
        }
    });
    out
}

// -------------------------------------------------------------------- glow

fn smooth(t0: f32, t1: f32, x: f32) -> f32 {
    if t1 <= t0 {
        return if x >= t0 { 1.0 } else { 0.0 };
    }
    let t = ((x - t0) / (t1 - t0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Bloom: bright-pass -> blurred mip pyramid -> scatter-weighted upsample -> add.
/// CALIBRATE: threshold interpretation (smoothstep over [low, high] of max(r,g,b)),
/// scattering mapping ((s-1)/9 as URP-style scatter), intensity scale, level count.
fn glow(g: &Glow, f: &mut Frame) {
    let (w, h) = (f.w, f.h);
    let t0 = g.threshold[0].min(g.threshold[1]);
    let t1 = g.threshold[0].max(g.threshold[1]);

    let mut pre = f.px.clone();
    pre.par_chunks_mut(3).for_each(|p| {
        let k = smooth(t0, t1, p[0].max(p[1]).max(p[2]));
        p[0] *= k;
        p[1] *= k;
        p[2] *= k;
    });

    let mut levels: Vec<(Vec<f32>, usize, usize)> = Vec::new();
    let mut cur = (pre, w, h);
    for _ in 0..MAX_GLOW_LEVELS {
        if cur.1 < 4 || cur.2 < 4 {
            break;
        }
        let (d, dw, dh) = downsample2(&cur.0, cur.1, cur.2);
        let b = blur(&d, dw, dh, &[0.25, 0.5, 0.25]);
        levels.push((b.clone(), dw, dh));
        cur = (b, dw, dh);
    }
    if levels.is_empty() {
        return;
    }

    let scatter = ((g.scattering - 1.0) / 9.0).clamp(0.0, 1.0);
    let n = levels.len();
    let mut acc = levels[n - 1].0.clone();
    let (mut aw, mut ah) = (levels[n - 1].1, levels[n - 1].2);
    for i in (0..n - 1).rev() {
        let lv = &levels[i].0;
        let (lw, lh) = (levels[i].1, levels[i].2);
        let up = resize_bilinear(&acc, aw, ah, lw, lh);
        acc = lv.iter().zip(up.iter()).map(|(a, b)| a + (b - a) * scatter).collect();
        aw = lw;
        ah = lh;
    }
    let full = resize_bilinear(&acc, aw, ah, w, h);

    let tint = [
        g.color[0] * g.intensity,
        g.color[1] * g.intensity,
        g.color[2] * g.intensity,
    ];
    f.px.par_chunks_mut(3).zip(full.par_chunks(3)).for_each(|(p, u)| {
        for c in 0..3 {
            p[c] = (p[c] + u[c] * tint[c]).min(1.0);
        }
    });
}

// ------------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pchip_hits_control_points() {
        let pts = vec![[0.0, 0.0], [0.25, 0.1], [0.6, 0.5], [1.0, 0.55]];
        let l = pchip_lut(&pts, LUT_SIZE);
        for p in &pts {
            assert!((lut(&l, p[0]) - p[1]).abs() < 2e-3, "point {:?}", p);
        }
        // monotone data must stay monotone
        assert!(l.windows(2).all(|w| w[1] >= w[0] - 1e-6));
    }

    #[test]
    fn kernel_is_normalised() {
        let k = gaussian_kernel(1.0);
        assert!((k.iter().sum::<f32>() - 1.0).abs() < 1e-5);
        assert_eq!(k.len() % 2, 1);
    }

    #[test]
    fn sample_layer_runs_and_stays_in_range() {
        let layer: AdjustmentLayer =
            serde_json::from_str(include_str!("../samples/Untitled.adjust.json")).unwrap();
        let pipe = Pipeline::build(&layer);
        let (w, h) = (64usize, 48usize);
        let mut f = Frame::new(w, h);
        for (i, v) in f.px.iter_mut().enumerate() {
            *v = ((i * 7919) % 256) as f32 / 255.0;
        }
        pipe.run(&mut f);
        assert!(f.px.iter().all(|v| v.is_finite() && *v >= 0.0 && *v <= 1.0));
    }

    #[test]
    fn empty_layer_is_identity() {
        let layer = AdjustmentLayer {
            format: FORMAT.into(),
            version: VERSION,
            name: "x".into(),
            apply_to: "all_layers_below".into(),
            effects: vec![],
        };
        let pipe = Pipeline::build(&layer);
        let mut f = Frame::new(8, 8);
        for (i, v) in f.px.iter_mut().enumerate() {
            *v = (i % 255) as f32 / 255.0;
        }
        let before = f.px.clone();
        pipe.run(&mut f);
        assert_eq!(before, f.px);
    }
}
