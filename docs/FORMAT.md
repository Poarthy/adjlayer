# ساختار فایل: `.nv` اصلی و فرمت تمیز

## ۱. فایل `.nv` اصلی چیست؟

یک فایل **JSON** است که کتابخانه‌ی Json.NET از اشیای C# ساخته (با `PreserveReferencesHandling`).
پس هر چیزی که در کد برنامه یک کلاس بوده، همان‌طور که هست در فایل ریخته شده، با همه‌ی وضعیت رابط کاربری.
بیشتر حجم ۴٫۹ مگابایتی فایل همین زائده‌هاست، نه اطلاعات افکت.

### نشانه‌های Json.NET
| کلید | معنی | تعداد در این فایل |
|---|---|---|
| `$id` | شناسه‌ی شیء (هر شیء یک بار تعریف می‌شود) | ≈ ۸٬۴۰۰ |
| `$ref` | ارجاع به یک `$id` قبلی (اغلب پارامتر ← پدرش) | ≈ ۸٬۲۰۰ |
| `$type` | نام کلاس C# (`NodeVideo.Data.PFloat, Assembly-CSharp`) | ≈ ۸٬۳۰۰ |

حداکثر عمق تو در تویی ۲۰ است.

### درخت پروژه
```
Root                       Type=2   (پروژه، مدت ۵ ثانیه، TimeLine ۰ تا ۱۰)
├─ Props: DataTransform, DataProjectSetting, Data3DSettings
└─ ChildList
   └─ Adjustment Layer     Type=0   $type=NodeVideo.Logic.NodeAdjustment
      ├─ Data.Params._apply_to   = _all_layers_below | _next_layer
      └─ Props (به ترتیب اجرا)
         1. DataTransform                (جابه‌جایی لایه؛ برای Adjustment Layer بی‌اثر)
         2. {"$ref":"2"}                 (ارجاع به همان Transform)
         3. DataEffect   _Glow
         4. DataColorGrading
         5. DataEffect   _ShiftChannels
         6. DataColorCurve
         7. DataEffect   _UnsharpMask
```
`DataProjectSetting` تنظیمات خود پروژه را دارد: ۳۰ فریم بر ثانیه، `_export_crf = 17` و پروفایل H.264.

### انواع پارامتر (`NodeVideo.Data.*`)
| نوع | تعداد | مقدار واقعی کجاست |
|---|---|---|
| `PFloat` | ۵٬۹۲۸ | `Value` (محدوده‌ی `Min`/`Max`، مقدار پیش‌فرض `OriValue`) |
| `PBool` | ۱٬۰۴۶ | `Value` (۰ یا ۱) |
| `PEnum` | ۵۷۱ | `Value` = اندیس در `Enums` |
| `PList` | ۵۷۱ | `List` (آرایه‌ای از پارامترها، اغلب با `Name`) |
| `PVector2/3` | ۶۶ / ۲۰ | `Params` = ۲ یا ۳ `PFloat` |
| `PColor` | ۳۴ | `Color` = `{r,g,b,a}` |
| `…Const` | ۲۰ | ثابت‌ها (رزولوشن، فهرست‌ها) |

### هر پارامتر چه فیلدهایی دارد و کدام لازم است؟
مثلاً `PFloat` حدود ۳۰ فیلد دارد، ولی فقط این‌ها «افکت» هستند:

* **لازم:** `Name`, `Value`
* **انیمیشن (در این فایل همه خالی/خاموش‌اند):** `KeyFrames`, `WiggleEnabled`, `BandEnabled`, `ExpressionEnabled`, `_Expression`
* **فقط رابط کاربری، حذف می‌شود:** `Style`, `Nested`, `NestedParams`, `Clamp`, `HideInUI`, `Percentage`, `Cycle`, `UISensitivity`, `UIBrightness`, `MINIFlags`, `_MINIGroup`, `MINIGroup`, `MINIOrder`, `SnapToInt`, `MultipleOf2`, `Expanded`, `Selected`, `Prop($ref)`, `$id`…
* **محدوده و پیش‌فرض (برای ابزار ویرایش لازم، برای رندر نه):** `Min`, `Max`, `OriValue`

`NestedParams` برای هر پارامتر زیرپارامترهایی دارد (ظاهراً برای کانال‌های Audio Band، مثل `_band_frequency_channel`) که خودشان همان ۳۰ فیلد را تکرار می‌کنند؛ بخش بزرگی از حجم همین‌هاست.

### فیلدهای سطح Prop افکت
`$type`, `DataPath` (مسیر افکت در فروشگاه، مثل `_Effect/_UnsharpMask`), `Params`, `Hided` (خاموش بودن افکت)،
`UseMask`/`ShowMask` (ماسک؛ اینجا خاموش)، `Version`, `OriVersion`, `Expanded`, `AllowDuplicate`, `Node`($ref).

## ۲. چه چیزی دور ریخته شد، چه چیزی ماند

| ماند (مقدار افکت) | دور ریخته شد |
|---|---|
| ترتیب افکت‌ها، روشن/خاموش بودن (`Hided`) | همه‌ی `$id`/`$ref`/`$type` |
| مقدار هر پارامتر مؤثر | همه‌ی فیلدهای رابط کاربری، `Min`/`Max`/`OriValue` |
| نقاط منحنی‌ها، رنگ‌ها، اندیس enumها (به‌صورت نام) | Transform لایه، رنگ و UID و UIIndex نود، BookMarks، تنظیمات پروژه |
| `apply_to` | `_editor` (تب فعال در UI)، `output_channel` ماتریس رنگ (وضعیت UI) |

**هیچ مقداری از خود افکت‌ها تغییر نکرده.** نتیجه‌ی تبدیل: ۴٬۹۴۳٬۷۵۲ بایت ← ۴٬۸۶۷ بایت.

> `backup` یک عکس فوری قدیمی‌تر از همین پروژه است: Glow با intensity ۱٫۰ و رنگ سفید، یک Color Grading،
> و Basic Correction، به‌علاوه‌ی نود `Image`. نسخه‌ی معتبر `Untitled.nv` است (طبق لاگ، Image حذف و
> Basic Correction/Tint/CAS بعداً عوض شدند).

## ۳. فرمت تمیز (`*.adjust.json`، نسخه‌ی ۱)

```jsonc
{
  "format": "adjustment-layer",
  "version": 1,
  "name": "Adjustment Layer",
  "apply_to": "all_layers_below",      // یا "next_layer"
  "effects": [ /* به ترتیب اجرا، از بالا به پایین */ ]
}
```
هر افکت یک شیء با `"type"` و `"enabled"` است. نوع‌های نسخه‌ی ۱:

### `glow`
| فیلد | معنی |
|---|---|
| `scattering` | ۱ تا ۱۰ |
| `threshold` | `[پایین, بالا]` ۰ تا ۱: بازه‌ای که در آن درخشش شروع به ظاهر شدن می‌کند |
| `intensity` | شدت |
| `color` | `[r,g,b]` رنگ درخشش |
| `preserve_alpha` | ثبت می‌شود؛ ویدیو آلفا ندارد |

### `color_grading`
`tone_mapping` (`none|neutral|aces`)، `temperature`, `tint`, `exposure`, `color_filter [r,g,b]`, `hue_shift`,
`saturation` (‎−۱۰۰ تا ۱۰۰)، `contrast` (‎−۱۰۰ تا ۱۰۰)، `lift`/`gamma`/`gain` (`[r,g,b,a]`، حالت خنثی `[1,1,1,0.5]`)،
`channel_mixer` (۳×۳ به درصد؛ سطر = کانال خروجی، ستون = کانال ورودی؛ ۱۰۰ روی قطر یعنی خنثی)،
`curves` با کلیدهای `rgb r g b hue_vs_hue hue_vs_sat sat_vs_sat lum_vs_sat hue_vs_lum sat_vs_lum`
(هرکدام لیست `[x,y]`).

### `shift_channels`
`r g b a` ← یکی از: `red green blue alpha luminance hue lightness saturation full_on full_off`.

### `color_curve`
`master r g b` ← لیست نقاط `[x,y]`.

### `unsharp_mask`
`preset` (`sharpen|soft_edge|clean`، فقط ثبت می‌شود)، `amount`, `radius`, `threshold`.

## ۴. مقدارهای واقعی این پروژه

| افکت | مقادیر غیر پیش‌فرض |
|---|---|
| glow | scattering ۵٫۰۲۱۹، threshold [۰٫۵۰۴، ۰٫۷۳۸]، intensity ۰٫۵، رنگ (۱، ۰٫۹۶، ۰٫۸۸) |
| color_grading | saturation = ‎−۵۰ (بقیه خنثی؛ `contrast ‎−0.5` مقدار پیش‌فرض خود اپ است) |
| shift_channels | همانی (R←R، G←G، B←B، A←A)؛ بی‌اثر |
| color_curve | منحنی ۱۰ نقطه‌ای برای هر کانال R/G/B؛ سفید به ≈ ۰٫۵۵–۰٫۵۷ می‌رسد، سیاهی‌ها بالا می‌آیند |
| unsharp_mask | amount ۱٫۳، radius ۱٫۰، threshold ۰ |

## ۵. محدودیت‌های نسخه‌ی ۱
* انیمیشن (keyframe، wiggle، expression، Audio Band) ذخیره نمی‌شود؛ مقدار ثابت استفاده می‌شود.
  ابزار هنگام ورود از `.nv` هشدار می‌دهد.
* ماسک (`UseMask`) پشتیبانی نمی‌شود.
* `Hided` به `enabled=false` تبدیل می‌شود.
* افکت‌های ناشناخته (مثلاً Tint، Basic Correction، Contrast Adaptive Sharpening که در لاگ بودند) هنگام ورود رد و هشدار داده می‌شوند.
