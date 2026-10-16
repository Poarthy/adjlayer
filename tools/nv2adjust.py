#!/usr/bin/env python3
"""Reference converter: NodeVideo .nv  ->  clean adjustment-layer JSON.
The Rust importer (src/nv.rs) mirrors this logic exactly."""
import json, sys, re


def snake(s):
    s = s.lstrip("_")
    s = re.sub(r"(?<=[a-z])(?=[A-Z])", "_", s)
    return s.lower()


def find_adj(o):
    if isinstance(o, dict):
        if str(o.get("$type", "")).startswith("NodeVideo.Logic.NodeAdjustment"):
            return o
        for v in o.values():
            r = find_adj(v)
            if r is not None:
                return r
    elif isinstance(o, list):
        for v in o:
            r = find_adj(v)
            if r is not None:
                return r
    return None


def r6(x):
    return round(float(x), 6)


def val(p, n, d=0.0):
    x = p["Params"].get(n)
    return r6(x["Value"]) if isinstance(x, dict) and "Value" in x else d


def col(c, a=False):
    k = "rgba" if a else "rgb"
    return [r6(c[i]) for i in k]


def enum_name(x):
    return snake(x["Enums"][int(round(x["Value"]))])


def item(lst, name):
    for it in lst["List"]:
        if it.get("Name") == name:
            return it
    raise KeyError(name)


def points(lst):
    return [[r6(q["Value"]) for q in it["Params"][:2]] for it in lst["List"]]


def has_animation(o):
    if isinstance(o, dict):
        if o.get("KeyFrames"): return True
        if any(o.get(k) is True for k in ("WiggleEnabled", "BandEnabled", "ExpressionEnabled")): return True
        return any(has_animation(v) for v in o.values())
    if isinstance(o, list): return any(has_animation(v) for v in o)
    return False


def conv(p, warnings):
    t = p["$type"].split(",")[0].split(".")[-1]
    en = not p.get("Hided", False)
    if t == "DataEffect":
        path = p["DataPath"]
        P = p["Params"]
        if path.endswith("_Glow"):
            th = P["_BloomThreshold"]["Params"]
            return {"type": "glow", "enabled": en, "scattering": val(p, "_BloomScattering"),
                    "threshold": [r6(th[0]["Value"]), r6(th[1]["Value"])],
                    "intensity": val(p, "_BloomIntensity"),
                    "color": col(P["_glow_color"]["Color"]),
                    "preserve_alpha": bool(val(p, "_preserve_alpha"))}
        if path.endswith("_ShiftChannels"):
            return {"type": "shift_channels", "enabled": en,
                    "r": enum_name(P["_r_source"]), "g": enum_name(P["_g_source"]),
                    "b": enum_name(P["_b_source"]), "a": enum_name(P["_a_source"])}
        if path.endswith("_UnsharpMask"):
            return {"type": "unsharp_mask", "enabled": en, "preset": enum_name(P["_preset"]),
                    "amount": val(p, "_amount"), "radius": val(p, "_radius"),
                    "threshold": val(p, "_threshold")}
        warnings.append("unsupported effect skipped: " + path)
        return None
    if t == "DataColorGrading":
        P = p["Params"]
        wb, tone, mx = P["_whitebalance"], P["_tone"], P["_channel_mixer"]

        def m(n):
            return r6(item(mx, n)["Value"])

        curves = {"rgb": "_curve_points_rgb", "r": "_curve_points_r", "g": "_curve_points_g",
                  "b": "_curve_points_b", "hue_vs_hue": "_curve_points_hh",
                  "hue_vs_sat": "_curve_points_hs", "sat_vs_sat": "_curve_points_ss",
                  "lum_vs_sat": "_curve_points_ls", "hue_vs_lum": "_curve_points_hl",
                  "sat_vs_lum": "_curve_points_sl"}
        return {"type": "color_grading", "enabled": en, "tone_mapping": enum_name(P["_tone_mapping"]),
                "temperature": r6(item(wb, "_temperature")["Value"]), "tint": r6(item(wb, "_tint")["Value"]),
                "exposure": r6(item(tone, "_exposure")["Value"]),
                "color_filter": col(item(tone, "_color_filter")["Color"]),
                "hue_shift": r6(item(tone, "_hue_shift")["Value"]),
                "saturation": r6(item(tone, "_saturation")["Value"]),
                "contrast": r6(item(tone, "_contrast")["Value"]),
                "lift": col(P["_lift"]["Color"], True), "gamma": col(P["_gamma"]["Color"], True),
                "gain": col(P["_gain"]["Color"], True),
                "channel_mixer": [[m("_r_red"), m("_r_green"), m("_r_blue")],
                                  [m("_g_red"), m("_g_green"), m("_g_blue")],
                                  [m("_b_red"), m("_b_green"), m("_b_blue")]],
                "curves": {k: points(P[v]) for k, v in curves.items()}}
    if t == "DataColorCurve":
        P = p["Params"]
        return {"type": "color_curve", "enabled": en,
                "master": points(P["_colorcurve_points"]), "r": points(P["_colorcurve_points_r"]),
                "g": points(P["_colorcurve_points_g"]), "b": points(P["_colorcurve_points_b"])}
    if t != "DataTransform":
        warnings.append("unsupported prop skipped: " + t)
    return None


def main(src, dst):
    adj = find_adj(json.load(open(src)))
    if adj is None:
        sys.exit("no NodeAdjustment layer found")
    warnings, effects = [], []
    if has_animation(adj):
        warnings.append("layer contains keyframes/wiggle/expression/audio-band animation; ignored (static values used)")
    for p in adj["Props"]:
        if "$type" not in p:
            continue  # {"$ref": ...} back-reference
        e = conv(p, warnings)
        if e:
            effects.append(e)
    ap = adj["Data"]["Params"]["_apply_to"]
    apply_to = snake(ap["Enums"][int(ap["Value"])])
    out = {"format": "adjustment-layer", "version": 1, "name": adj.get("Name", "Adjustment Layer"),
           "apply_to": apply_to, "effects": effects}
    json.dump(out, open(dst, "w"), indent=2)
    for w in warnings:
        print("warning:", w, file=sys.stderr)
    print(f"wrote {dst}: {len(effects)} effects")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
