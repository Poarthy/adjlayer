//! Clean, NodeVideo-independent description of an Adjustment Layer.
//! See docs/FORMAT.md for the full specification.

use serde::{Deserialize, Serialize};

pub const FORMAT: &str = "adjustment-layer";
pub const VERSION: u32 = 1;

fn yes() -> bool {
    true
}

/// Curve control points `[x, y]`, both in 0..1.
pub type Curve = Vec<[f32; 2]>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdjustmentLayer {
    pub format: String,
    pub version: u32,
    pub name: String,
    /// "all_layers_below" | "next_layer" (informational: a video has one layer)
    pub apply_to: String,
    /// Applied top to bottom, in this order.
    pub effects: Vec<Effect>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Effect {
    Glow(Glow),
    ColorGrading(ColorGrading),
    ShiftChannels(ShiftChannels),
    ColorCurve(ColorCurve),
    UnsharpMask(UnsharpMask),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Glow {
    #[serde(default = "yes")]
    pub enabled: bool,
    /// 1..10
    pub scattering: f32,
    /// [low, high] brightness range where the glow fades in (0..1)
    pub threshold: [f32; 2],
    pub intensity: f32,
    pub color: [f32; 3],
    pub preserve_alpha: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColorGrading {
    #[serde(default = "yes")]
    pub enabled: bool,
    /// "none" | "neutral" | "aces"
    pub tone_mapping: String,
    pub temperature: f32,
    pub tint: f32,
    pub exposure: f32,
    pub color_filter: [f32; 3],
    pub hue_shift: f32,
    /// -100..100, 0 = unchanged
    pub saturation: f32,
    /// -100..100, 0 = unchanged
    pub contrast: f32,
    /// RGBA colour-wheel values; neutral is [1,1,1,0.5]
    pub lift: [f32; 4],
    pub gamma: [f32; 4],
    pub gain: [f32; 4],
    /// rows = output R,G,B; columns = input R,G,B; percent (100 = identity)
    pub channel_mixer: [[f32; 3]; 3],
    pub curves: GradingCurves,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GradingCurves {
    #[serde(default)]
    pub rgb: Curve,
    #[serde(default)]
    pub r: Curve,
    #[serde(default)]
    pub g: Curve,
    #[serde(default)]
    pub b: Curve,
    #[serde(default)]
    pub hue_vs_hue: Curve,
    #[serde(default)]
    pub hue_vs_sat: Curve,
    #[serde(default)]
    pub sat_vs_sat: Curve,
    #[serde(default)]
    pub lum_vs_sat: Curve,
    #[serde(default)]
    pub hue_vs_lum: Curve,
    #[serde(default)]
    pub sat_vs_lum: Curve,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShiftChannels {
    #[serde(default = "yes")]
    pub enabled: bool,
    /// red|green|blue|alpha|luminance|hue|lightness|saturation|full_on|full_off
    pub r: String,
    pub g: String,
    pub b: String,
    pub a: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColorCurve {
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub master: Curve,
    #[serde(default)]
    pub r: Curve,
    #[serde(default)]
    pub g: Curve,
    #[serde(default)]
    pub b: Curve,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnsharpMask {
    #[serde(default = "yes")]
    pub enabled: bool,
    /// "sharpen" | "soft_edge" | "clean" (recorded, not used by the renderer)
    pub preset: String,
    pub amount: f32,
    pub radius: f32,
    pub threshold: f32,
}
