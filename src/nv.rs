//! Importer: raw NodeVideo `.nv` (Json.NET dump) -> clean `AdjustmentLayer`.
//! Mirrors tools/nv2adjust.py.  Everything that is UI state ($id, $ref,
//! Expanded, Selected, UISensitivity, ...) is dropped; only effect values stay.

use crate::model::*;
use serde_json::Value;

type R<T> = Result<T, String>;

pub fn import(root: &Value) -> R<(AdjustmentLayer, Vec<String>)> {
    let node = find_adjustment(root).ok_or("no NodeAdjustment layer found in this file")?;
    let props = node
        .get("Props")
        .and_then(|p| p.as_array())
        .ok_or("adjustment layer has no Props")?;

    let mut warnings = Vec::new();
    if has_animation(node) {
        warnings.push(
            "layer contains keyframes/wiggle/expression/audio-band animation; \
             this tool ignores animation and uses the static values"
                .to_string(),
        );
    }
    let mut effects = Vec::new();
    for p in props {
        // `{"$ref": "2"}` entries are back-references to the Transform prop.
        if p.get("$type").is_none() {
            continue;
        }
        if let Some(e) = convert_prop(p, &mut warnings)? {
            effects.push(e);
        }
    }

    let apply_to = node
        .get("Data")
        .and_then(|d| d.get("Params"))
        .and_then(|p| p.get("_apply_to"))
        .and_then(|x| enum_name(x).ok())
        .unwrap_or_else(|| "all_layers_below".to_string());
    let name = node
        .get("Name")
        .and_then(|n| n.as_str())
        .unwrap_or("Adjustment Layer")
        .to_string();

    Ok((
        AdjustmentLayer {
            format: FORMAT.to_string(),
            version: VERSION,
            name,
            apply_to,
            effects,
        },
        warnings,
    ))
}

fn has_animation(v: &Value) -> bool {
    match v {
        Value::Object(m) => {
            if m.get("KeyFrames").and_then(|k| k.as_array()).map_or(false, |a| !a.is_empty()) {
                return true;
            }
            for k in ["WiggleEnabled", "BandEnabled", "ExpressionEnabled"] {
                if m.get(k).and_then(|x| x.as_bool()) == Some(true) {
                    return true;
                }
            }
            m.values().any(has_animation)
        }
        Value::Array(a) => a.iter().any(has_animation),
        _ => false,
    }
}

fn find_adjustment(v: &Value) -> Option<&Value> {
    match v {
        Value::Object(m) => {
            let is_adj = m
                .get("$type")
                .and_then(|t| t.as_str())
                .map_or(false, |t| t.starts_with("NodeVideo.Logic.NodeAdjustment"));
            if is_adj {
                return Some(v);
            }
            m.values().find_map(find_adjustment)
        }
        Value::Array(a) => a.iter().find_map(find_adjustment),
        _ => None,
    }
}

// ---------------------------------------------------------------- helpers

fn num(v: Option<&Value>) -> Option<f32> {
    v.and_then(|x| x.as_f64()).map(|x| x as f32)
}

fn snake(s: &str) -> String {
    let mut out = String::new();
    let mut prev_lower = false;
    for ch in s.trim_start_matches('_').chars() {
        if ch.is_uppercase() && prev_lower {
            out.push('_');
        }
        prev_lower = ch.is_lowercase();
        out.extend(ch.to_lowercase());
    }
    out
}

fn param<'a>(p: &'a Value, name: &str) -> R<&'a Value> {
    p.get("Params")
        .and_then(|m| m.get(name))
        .ok_or_else(|| format!("missing parameter {name}"))
}

fn value(p: &Value, name: &str) -> R<f32> {
    num(param(p, name)?.get("Value")).ok_or_else(|| format!("parameter {name} has no numeric Value"))
}

fn color(x: &Value) -> R<[f32; 4]> {
    let c = x.get("Color").ok_or("colour parameter without Color")?;
    let g = |k: &str| num(c.get(k)).ok_or_else(|| format!("bad colour channel {k}"));
    Ok([g("r")?, g("g")?, g("b")?, g("a")?])
}

fn enum_name(x: &Value) -> R<String> {
    let idx = num(x.get("Value")).ok_or("enum without Value")?.round() as usize;
    let names = x
        .get("Enums")
        .and_then(|e| e.as_array())
        .ok_or("enum without Enums")?;
    let n = names
        .get(idx)
        .and_then(|s| s.as_str())
        .ok_or("enum index out of range")?;
    Ok(snake(n))
}

fn item<'a>(list: &'a Value, name: &str) -> R<&'a Value> {
    list.get("List")
        .and_then(|l| l.as_array())
        .and_then(|a| {
            a.iter()
                .find(|it| it.get("Name").and_then(|n| n.as_str()) == Some(name))
        })
        .ok_or_else(|| format!("missing list item {name}"))
}

fn item_value(list: &Value, name: &str) -> R<f32> {
    num(item(list, name)?.get("Value")).ok_or_else(|| format!("list item {name} has no Value"))
}

fn points(list: &Value) -> R<Curve> {
    let arr = list
        .get("List")
        .and_then(|l| l.as_array())
        .ok_or("curve without List")?;
    arr.iter()
        .map(|it| -> R<[f32; 2]> {
            let ps = it
                .get("Params")
                .and_then(|p| p.as_array())
                .ok_or("curve point without Params")?;
            let x = num(ps.get(0).and_then(|q| q.get("Value"))).ok_or("bad curve x")?;
            let y = num(ps.get(1).and_then(|q| q.get("Value"))).ok_or("bad curve y")?;
            Ok([x, y])
        })
        .collect()
}

// ---------------------------------------------------------------- props

fn convert_prop(p: &Value, warnings: &mut Vec<String>) -> R<Option<Effect>> {
    let full = p.get("$type").and_then(|t| t.as_str()).unwrap_or("");
    let ty = full.split(',').next().unwrap_or("").rsplit('.').next().unwrap_or("");
    let enabled = !p.get("Hided").and_then(|h| h.as_bool()).unwrap_or(false);

    match ty {
        "DataEffect" => {
            let path = p.get("DataPath").and_then(|x| x.as_str()).unwrap_or("");
            if path.ends_with("_Glow") {
                let th = param(p, "_BloomThreshold")?
                    .get("Params")
                    .and_then(|x| x.as_array())
                    .ok_or("glow threshold missing")?;
                let t0 = num(th.get(0).and_then(|q| q.get("Value"))).ok_or("bad glow threshold")?;
                let t1 = num(th.get(1).and_then(|q| q.get("Value"))).ok_or("bad glow threshold")?;
                let c = color(param(p, "_glow_color")?)?;
                Ok(Some(Effect::Glow(Glow {
                    enabled,
                    scattering: value(p, "_BloomScattering")?,
                    threshold: [t0, t1],
                    intensity: value(p, "_BloomIntensity")?,
                    color: [c[0], c[1], c[2]],
                    preserve_alpha: value(p, "_preserve_alpha")? > 0.5,
                })))
            } else if path.ends_with("_ShiftChannels") {
                Ok(Some(Effect::ShiftChannels(ShiftChannels {
                    enabled,
                    r: enum_name(param(p, "_r_source")?)?,
                    g: enum_name(param(p, "_g_source")?)?,
                    b: enum_name(param(p, "_b_source")?)?,
                    a: enum_name(param(p, "_a_source")?)?,
                })))
            } else if path.ends_with("_UnsharpMask") {
                Ok(Some(Effect::UnsharpMask(UnsharpMask {
                    enabled,
                    preset: enum_name(param(p, "_preset")?)?,
                    amount: value(p, "_amount")?,
                    radius: value(p, "_radius")?,
                    threshold: value(p, "_threshold")?,
                })))
            } else {
                warnings.push(format!("unsupported effect skipped: {path}"));
                Ok(None)
            }
        }
        "DataColorGrading" => {
            let wb = param(p, "_whitebalance")?;
            let tone = param(p, "_tone")?;
            let mx = param(p, "_channel_mixer")?;
            let cf = color(item(tone, "_color_filter")?)?;
            let m = |n: &str| item_value(mx, n);
            let curves = GradingCurves {
                rgb: points(param(p, "_curve_points_rgb")?)?,
                r: points(param(p, "_curve_points_r")?)?,
                g: points(param(p, "_curve_points_g")?)?,
                b: points(param(p, "_curve_points_b")?)?,
                hue_vs_hue: points(param(p, "_curve_points_hh")?)?,
                hue_vs_sat: points(param(p, "_curve_points_hs")?)?,
                sat_vs_sat: points(param(p, "_curve_points_ss")?)?,
                lum_vs_sat: points(param(p, "_curve_points_ls")?)?,
                hue_vs_lum: points(param(p, "_curve_points_hl")?)?,
                sat_vs_lum: points(param(p, "_curve_points_sl")?)?,
            };
            let w = |n: &str| -> R<[f32; 4]> { color(param(p, n)?) };
            Ok(Some(Effect::ColorGrading(ColorGrading {
                enabled,
                tone_mapping: enum_name(param(p, "_tone_mapping")?)?,
                temperature: item_value(wb, "_temperature")?,
                tint: item_value(wb, "_tint")?,
                exposure: item_value(tone, "_exposure")?,
                color_filter: [cf[0], cf[1], cf[2]],
                hue_shift: item_value(tone, "_hue_shift")?,
                saturation: item_value(tone, "_saturation")?,
                contrast: item_value(tone, "_contrast")?,
                lift: w("_lift")?,
                gamma: w("_gamma")?,
                gain: w("_gain")?,
                channel_mixer: [
                    [m("_r_red")?, m("_r_green")?, m("_r_blue")?],
                    [m("_g_red")?, m("_g_green")?, m("_g_blue")?],
                    [m("_b_red")?, m("_b_green")?, m("_b_blue")?],
                ],
                curves,
            })))
        }
        "DataColorCurve" => Ok(Some(Effect::ColorCurve(ColorCurve {
            enabled,
            master: points(param(p, "_colorcurve_points")?)?,
            r: points(param(p, "_colorcurve_points_r")?)?,
            g: points(param(p, "_colorcurve_points_g")?)?,
            b: points(param(p, "_colorcurve_points_b")?)?,
        }))),
        "DataTransform" => Ok(None),
        other => {
            warnings.push(format!("unsupported prop skipped: {other}"));
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snake_case_names() {
        assert_eq!(snake("_FullOn"), "full_on");
        assert_eq!(snake("_ACES"), "aces");
        assert_eq!(snake("_soft_edge"), "soft_edge");
        assert_eq!(snake("_all_layers_below"), "all_layers_below");
    }
}
