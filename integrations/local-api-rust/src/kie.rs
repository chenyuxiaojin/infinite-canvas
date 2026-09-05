use crate::{db::ApiResult, direct::*};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    sync::OnceLock,
};

pub const IMAGES: &[&str] = &[
    "image",
    "images",
    "image_url",
    "image_urls",
    "input_url",
    "input_urls",
    "input_reference",
    "input_reference[]",
    "image_input",
    "reference_image",
    "reference_images",
    "reference_image_url",
    "reference_image_urls",
    "first_frame_url",
    "last_frame_url",
    "end_image_url",
    "tail_image_url",
];
pub const VIDEOS: &[&str] = &[
    "video",
    "videos",
    "video_url",
    "video_urls",
    "input_video_url",
    "input_video_urls",
    "video_reference",
    "video_reference[]",
    "first_clip_url",
    "reference_video",
    "reference_videos",
    "reference_video_url",
    "reference_video_urls",
];
pub const AUDIOS: &[&str] = &[
    "audio",
    "audios",
    "audio_url",
    "audio_urls",
    "input_audio_url",
    "input_audio_urls",
    "reference_audio",
    "reference_audios",
    "reference_audio_url",
    "reference_audio_urls",
    "audio_reference",
    "audio_reference[]",
    "driving_audio_url",
    "reference_voice",
    "audio_ids",
];
const FRONTEND: &[&str] = &[
    "actual_image_count",
    "moderation",
    "response_format",
    "stream",
    "partial_images",
    "output_compression",
];
fn aliases() -> impl Iterator<Item = &'static str> {
    IMAGES.iter().chain(VIDEOS).chain(AUDIOS).copied()
}

#[derive(Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Config {
    aspect_field: String,
    aspect_kind: String,
    duration_kind: String,
    duration_min: i64,
    duration_max: i64,
    has_resolution: bool,
    resolution_field: String,
    resolution_kind: String,
    max_resolution: String,
    count_field: String,
    count_kind: String,
    has_quality: bool,
    has_output_format: bool,
    preset_field: String,
    image_ref_field: String,
    image_ref_kind: String,
    video_ref_field: String,
    video_ref_kind: String,
    audio_ref_field: String,
    audio_ref_kind: String,
}
impl Config {
    fn resolution_field(&self) -> &str {
        if !self.resolution_field.is_empty() {
            &self.resolution_field
        } else if self.has_resolution {
            "resolution"
        } else {
            ""
        }
    }
}
fn configuration(model: &str) -> Config {
    static CONFIGS: OnceLock<HashMap<String, Config>> = OnceLock::new();
    CONFIGS
        .get_or_init(|| serde_json::from_str(include_str!("kie-model-configs.json")).unwrap())
        .get(model)
        .cloned()
        .unwrap_or_default()
}
fn resolve_model(model: &str, payload: &Object) -> String {
    static ALIASES: OnceLock<HashMap<String, String>> = OnceLock::new();
    let lower = model.trim().to_lowercase();
    if let Some(alias) = ALIASES
        .get_or_init(|| serde_json::from_str(include_str!("kie-model-aliases.json")).unwrap())
        .get(&lower)
    {
        return alias.clone();
    }
    if model.contains('/') {
        return model.trim().into();
    }
    match lower.as_str() {
        "grok-imagine" | "grok-imagine-video" => {
            let keys = [
                "image_url",
                "image_urls",
                "first_frame_url",
                "last_frame_url",
                "first_frame_image",
                "last_frame_image",
                "image",
                "images",
            ];
            let has_image = keys.iter().any(|k| {
                !empty(get(payload, k))
                    || get(payload, "input")
                        .as_object()
                        .is_some_and(|i| !empty(get(i, k)))
            });
            if has_image {
                "grok-imagine/image-to-video"
            } else {
                "grok-imagine/text-to-video"
            }
            .into()
        }
        "grok-imagine-1.5-video"
        | "grok-imagine-1.5-preview"
        | "grok-imagine-video-1-5-preview" => "grok-imagine-video-1-5-preview".into(),
        _ => model.trim().into(),
    }
}
fn motion(model: &str) -> bool {
    matches!(
        model,
        "kling-2.6/motion-control" | "kling-3.0/motion-control"
    )
}
pub fn normalize(model: &str, body: &Value) -> ApiResult<Value> {
    let payload = body.as_object().cloned().unwrap_or_default();
    let model = resolve_model(model, &payload);
    let lower = model.to_lowercase();
    let cfg = configuration(&lower);
    let mut input = get(&payload, "input")
        .as_object()
        .cloned()
        .unwrap_or_default();
    for (key, value) in &payload {
        if [
            "model",
            "input",
            "metadata",
            "callBackUrl",
            "callbackUrl",
            "callback_url",
        ]
        .contains(&key.as_str())
            || FRONTEND.contains(&key.as_str())
            || explicit(key)
        {
            continue;
        }
        input.entry(key.clone()).or_insert(value.clone());
    }
    if !empty(get(&payload, "prompt")) {
        insert(&mut input, "prompt", get(&payload, "prompt").clone());
    }
    for key in ["size", "image_size", "ratio", "aspect_ratio"] {
        let value = get(&payload, key);
        if !empty(value) {
            set_aspect(&mut input, &cfg, value);
            if matches!(key, "size" | "image_size")
                && cfg.has_quality
                && cfg.resolution_field().is_empty()
            {
                let quality = text(get(&payload, "quality"));
                if empty(get(&payload, "quality")) || quality.trim().eq_ignore_ascii_case("auto") {
                    let resolution = size_resolution(&text(value));
                    if !resolution.is_empty() {
                        insert(&mut input, "quality", quality_value(&lower, &resolution));
                    }
                }
            }
        }
    }
    for key in aliases() {
        let value = get(&payload, key);
        if !empty(value) {
            let field = set_reference(&mut input, &cfg, &lower, key, value);
            if field != key {
                input.remove(key);
            }
        }
    }
    if motion(&lower) {
        remove(&mut input, &["duration", "seconds"]);
    } else {
        for key in ["duration", "seconds"] {
            if !empty(get(&payload, key)) {
                insert(&mut input, "duration", duration(get(&payload, key), &cfg));
            }
        }
    }
    for key in ["resolution", "image_resolution"] {
        if !empty(get(&payload, key)) {
            set_resolution(&mut input, &cfg, get(&payload, key));
        }
    }
    if !empty(get(&payload, "quality")) {
        set_quality_resolution(&mut input, &cfg, get(&payload, "quality"));
    }
    for key in ["n", "num_images", "max_images", "actual_image_count"] {
        if !empty(get(&payload, key)) {
            set_count(&mut input, &cfg, get(&payload, key));
        }
    }
    if let Some(meta) = get(&payload, "metadata").as_object() {
        if !empty(get(meta, "resolution_name")) {
            if motion(&lower) {
                insert(
                    &mut input,
                    "mode",
                    motion_mode(get(meta, "resolution_name")),
                );
            } else {
                set_resolution(&mut input, &cfg, get(meta, "resolution_name"));
            }
        }
        if !cfg.preset_field.is_empty() && !empty(get(meta, "preset")) {
            insert(
                &mut input,
                &cfg.preset_field,
                text(get(meta, "preset")).trim(),
            );
        }
    }
    normalize_input(&mut input, &cfg, &lower);
    validate_required(&input, &lower)?;
    let mut result = json!({"model":model,"input":input});
    let callback = first(&payload, &["callBackUrl", "callbackUrl", "callback_url"]);
    if !callback.is_empty() {
        result["callBackUrl"] = json!(callback);
    }
    Ok(result)
}
fn explicit(key: &str) -> bool {
    [
        "prompt",
        "size",
        "ratio",
        "aspect_ratio",
        "image_size",
        "seconds",
        "duration",
        "resolution",
        "image_resolution",
        "n",
        "num_images",
        "max_images",
        "mask_url",
    ]
    .contains(&key)
        || (IMAGES.contains(&key) || VIDEOS.contains(&key) || AUDIOS.contains(&key))
            && !matches!(key, "video_reference" | "video_reference[]")
}
fn normalize_input(input: &mut Object, cfg: &Config, model: &str) {
    let layer_size = get(input, "size").clone();
    let layer_quality = get(input, "quality").clone();
    if let Some(value) = input.remove("resolution_name") {
        if !empty(&value) {
            if motion(model) {
                insert(input, "mode", motion_mode(&value));
            } else {
                set_resolution(input, cfg, &value);
            }
        }
    }
    if let Some(value) = input.remove("preset") {
        if !empty(&value) && !cfg.preset_field.is_empty() {
            insert(input, &cfg.preset_field, text(&value).trim());
        }
    }
    for key in ["size", "image_size", "ratio", "aspect_ratio"] {
        if let Some(value) = input.get(key).cloned() {
            set_aspect(input, cfg, &value);
            if (key == "image_size" && cfg.aspect_field != "image_size")
                || (key == "ratio" && cfg.aspect_field != "ratio")
                || (key == "aspect_ratio" && cfg.aspect_field == "ratio")
            {
                input.remove(key);
            }
        }
    }
    for key in aliases() {
        if let Some(value) = input.get(key).cloned() {
            let field = set_reference(input, cfg, model, key, &value);
            if field != key {
                input.remove(key);
            }
        }
    }
    if model == "seedream/5-pro-layer-decomposition" {
        for key in aliases() {
            if key != "image_url" {
                input.remove(key);
            }
        }
        let size = text(&layer_size).trim().to_lowercase();
        let quality = text(&layer_quality).trim().to_lowercase();
        let size = match size.as_str() {
            "1k" | "1.5k" | "2k" => size.to_uppercase(),
            "auto" => "auto".into(),
            _ => match quality.as_str() {
                "low" => "1K",
                "medium" => "1.5K",
                "high" => "2K",
                _ => "auto",
            }
            .into(),
        };
        insert(input, "size", size);
        insert(input, "output_format", "png");
        remove(
            input,
            &[
                "quality",
                "ratio",
                "aspect_ratio",
                "image_size",
                "resolution",
                "image_resolution",
                "n",
                "num_images",
                "max_images",
                "actual_image_count",
            ],
        );
        remove(input, FRONTEND);
        return;
    }
    for key in ["resolution", "image_resolution"] {
        if let Some(value) = input.get(key).cloned() {
            set_resolution(input, cfg, &value);
            if cfg.resolution_field() != key {
                input.remove(key);
            }
        }
    }
    if let Some(value) = input.get("quality").cloned() {
        if cfg.has_quality {
            insert(input, "quality", quality_value(model, &text(&value)));
        } else {
            set_quality_resolution(input, cfg, &value);
            input.remove("quality");
        }
    }
    for key in ["n", "num_images", "max_images", "actual_image_count"] {
        if let Some(value) = input.get(key).cloned() {
            set_count(input, cfg, &value);
            if cfg.count_field != key {
                input.remove(key);
            }
        }
    }
    if motion(model) {
        remove(input, &["duration", "seconds"]);
    } else {
        if let Some(value) = input.remove("seconds") {
            if !empty(&value) {
                insert(input, "duration", duration(&value, cfg));
            }
        }
        if let Some(value) = input.get("duration").cloned() {
            insert(input, "duration", duration(&value, cfg));
        }
    }
    crate::kie_advanced::v3(input, model);
    if let Some(value) = input.remove("video_generate_audio") {
        let field = if [
            "kling-2.6/text-to-video",
            "kling-2.6/image-to-video",
            "kling-3.0/video",
        ]
        .contains(&model)
        {
            "sound"
        } else if model.starts_with("kling-3.0-omni/") || model.starts_with("wan/2-6-flash-") {
            "audio"
        } else if [
            "bytedance/seedance-2",
            "bytedance/seedance-2-fast",
            "bytedance/seedance-2-mini",
            "bytedance/seedance-1.5-pro",
            "bytedance/seedance-1-5-pro",
            "bytedance/seedance-2-5",
        ]
        .contains(&model)
        {
            "generate_audio"
        } else {
            ""
        };
        if !field.is_empty() {
            insert(input, field, bool_like(&value));
        }
    }
    crate::kie_advanced::omni(input, model);
    if let Some(value) = input.get("output_format").cloned() {
        if cfg.has_output_format {
            let value = text(&value);
            insert(
                input,
                "output_format",
                match value.trim().to_lowercase().as_str() {
                    "jpeg" | "jpg" => "jpg",
                    "webp" | "png" => "png",
                    _ => &value,
                },
            );
        } else {
            input.remove("output_format");
        }
    }
    match model {
        "minimax-h3/text-to-video" => {
            input.entry("aspect_ratio").or_insert(json!("16:9"));
        }
        "kling-2.6/text-to-video" | "kling-2.6/image-to-video" => {
            input.entry("sound").or_insert(json!(false));
        }
        "kling-2.6/motion-control" | "kling-3.0/motion-control" => {
            input.entry("mode").or_insert(json!("720p"));
        }
        "bytedance/seedance-2" | "bytedance/seedance-2-fast" => {
            input.entry("return_last_frame").or_insert(json!(false));
        }
        "wan/2-6-flash-image-to-video" | "wan/2-6-flash-video-to-video" => {
            input.entry("audio").or_insert(json!(false));
            input.entry("multi_shots").or_insert(json!(false));
        }
        "topaz/image-upscale" => {
            input.entry("upscale_factor").or_insert(json!("2"));
        }
        _ => {}
    }
    remove(input, FRONTEND);
}
fn duration(value: &Value, cfg: &Config) -> Value {
    let normalized = if value.is_string() {
        let s = duration_text(value);
        if cfg.duration_kind == "number" {
            s.parse::<i64>()
                .map(|n| json!(n))
                .or_else(|_| s.parse::<f64>().map(|n| json!(n)))
                .unwrap_or(json!(s))
        } else {
            json!(s)
        }
    } else if value.is_number() && cfg.duration_kind == "string" {
        json!(text(value).trim_end_matches(".0").trim_end_matches('.'))
    } else {
        value.clone()
    };
    if cfg.duration_min <= 0 && cfg.duration_max <= 0 {
        return normalized;
    }
    let parsed = if normalized.is_number() {
        Some(number(&normalized))
    } else {
        duration_text(&normalized).parse::<i64>().ok()
    };
    if let Some(mut n) = parsed {
        if cfg.duration_min > 0 {
            n = n.max(cfg.duration_min);
        }
        if cfg.duration_max > 0 {
            n = n.min(cfg.duration_max);
        }
        if cfg.duration_kind == "number" {
            json!(n)
        } else {
            json!(n.to_string())
        }
    } else {
        normalized
    }
}
fn set_count(input: &mut Object, cfg: &Config, value: &Value) {
    remove(input, &["n", "num_images", "max_images"]);
    if cfg.count_field.is_empty() {
        return;
    }
    let normalized = if cfg.count_kind == "string" {
        json!(text(value).trim_end_matches(".0").trim_end_matches('.'))
    } else if value.is_number() {
        json!(number(value))
    } else if let Some(s) = value.as_str() {
        s.trim()
            .parse::<i64>()
            .map(|n| json!(n))
            .unwrap_or(value.clone())
    } else {
        value.clone()
    };
    insert(input, &cfg.count_field, normalized);
}
fn set_aspect(input: &mut Object, cfg: &Config, value: &Value) {
    if cfg.aspect_field.is_empty() {
        remove(input, &["size", "ratio", "aspect_ratio", "image_size"]);
        return;
    }
    if cfg.resolution_kind == "image"
        && !cfg.resolution_field().is_empty()
        && !input.contains_key("resolution")
        && !input.contains_key("image_resolution")
    {
        let resolution = size_resolution(&text(value));
        if !resolution.is_empty() {
            set_resolution(input, cfg, &json!(resolution));
        }
    }
    let ratio = aspect(&text(value));
    let normalized = if cfg.aspect_kind == "image_size_named" {
        match ratio.as_str() {
            "1:1" => "square_hd",
            "16:9" => "landscape_16_9",
            "9:16" => "portrait_16_9",
            "4:3" => "landscape_4_3",
            "3:4" => "portrait_4_3",
            _ => &ratio,
        }
        .into()
    } else {
        ratio
    };
    remove(input, &["size", "ratio", "aspect_ratio", "image_size"]);
    insert(input, &cfg.aspect_field, normalized);
}
pub fn aspect(value: &str) -> String {
    let value = value.trim().to_lowercase().replace(' ', "");
    match value.as_str() {
        "landscape" | "landscape_16_9" => return "16:9".into(),
        "portrait" | "portrait_16_9" => return "9:16".into(),
        "square" | "square_hd" => return "1:1".into(),
        "landscape_4_3" => return "4:3".into(),
        "portrait_4_3" => return "3:4".into(),
        _ => {}
    }
    let pair = value.split(['x', '*', ':']).collect::<Vec<_>>();
    if pair.len() != 2 {
        return value;
    }
    let (Ok(width), Ok(height)) = (pair[0].parse::<u64>(), pair[1].parse::<u64>()) else {
        return value;
    };
    if width == 0 || height == 0 {
        return value;
    }
    let options = [
        (1, 1),
        (1, 4),
        (16, 9),
        (1, 8),
        (21, 9),
        (2, 3),
        (3, 2),
        (3, 4),
        (4, 1),
        (4, 3),
        (4, 5),
        (5, 4),
        (8, 1),
        (9, 16),
    ];
    let ratio = width as f64 / height as f64;
    let mut best = (f64::MAX, 1, 1);
    for (w, h) in options {
        let r = w as f64 / h as f64;
        let diff = (ratio - r).abs() / r;
        if diff < best.0 {
            best = (diff, w, h);
        }
    }
    if best.0 <= 0.04 {
        return format!("{}:{}", best.1, best.2);
    }
    let (mut a, mut b) = (width, height);
    while b != 0 {
        (a, b) = (b, a % b);
    }
    format!("{}:{}", width / a, height / a)
}
pub fn size_resolution(value: &str) -> String {
    let value = value.trim().to_lowercase().replace(' ', "");
    let pair = value.split('x').collect::<Vec<_>>();
    if pair.len() != 2 {
        return String::new();
    }
    let (Ok(w), Ok(h)) = (pair[0].parse::<u64>(), pair[1].parse::<u64>()) else {
        return String::new();
    };
    if w == 0 || h == 0 {
        return String::new();
    }
    match w.max(h) {
        3500.. => "4K",
        1700.. => "2K",
        900.. => "1K",
        _ => "",
    }
    .into()
}
pub fn basic_resolution(value: &str) -> String {
    let s = value.trim().replace(' ', "");
    match s.to_lowercase().as_str() {
        "480" => "480p",
        "720" => "720p",
        "1080" => "1080p",
        _ => &s,
    }
    .into()
}
pub fn image_resolution(value: &str) -> String {
    let s = value.trim().replace(' ', "");
    let lower = s.to_lowercase();
    match lower.as_str() {
        "1" | "1k" | "1024" | "1024p" => "1K".into(),
        "2" | "2k" | "2048" | "2048p" => "2K".into(),
        "4" | "4k" | "4096" | "4096p" => "4K".into(),
        _ => {
            if lower.ends_with('k') {
                lower.to_uppercase()
            } else {
                s
            }
        }
    }
}
fn set_resolution(input: &mut Object, cfg: &Config, value: &Value) {
    let field = cfg.resolution_field();
    if field.is_empty() {
        remove(input, &["resolution", "image_resolution"]);
        return;
    }
    let raw = text(value).trim().replace(' ', "");
    let lower = raw.to_lowercase();
    let mut normalized = match cfg.resolution_kind.as_str() {
        "image" => image_resolution(&raw),
        "hailuo_video" => match lower.as_str() {
            "480" | "480p" | "512" | "512p" => "512P",
            "720" | "720p" | "768" | "768p" => "768P",
            _ => &raw,
        }
        .into(),
        "minimax_h3_video" => match lower.as_str() {
            "480" | "480p" | "720" | "720p" | "768" | "768p" => "768P",
            "1080" | "1080p" | "2k" | "2048" | "2048p" => "2K",
            _ => &raw,
        }
        .into(),
        "seedance_2_5_video" => {
            if matches!(lower.as_str(), "1080" | "1080p" | "2k" | "4k") {
                "720p".into()
            } else {
                basic_resolution(&raw)
            }
        }
        _ => basic_resolution(&raw),
    };
    if cfg.max_resolution == "2K" && normalized == "4K" {
        normalized = "2K".into();
    }
    remove(input, &["resolution", "image_resolution"]);
    insert(input, field, normalized);
}
pub fn quality_resolution(value: &str) -> &str {
    match value.trim().to_lowercase().as_str() {
        "low" | "standard" | "1k" => "1K",
        "medium" | "hd" | "2k" => "2K",
        "high" | "4k" => "4K",
        _ => "",
    }
}
fn set_quality_resolution(input: &mut Object, cfg: &Config, value: &Value) {
    if cfg.has_quality
        || cfg.resolution_kind != "image"
        || cfg.resolution_field().is_empty()
        || input.contains_key("resolution")
        || input.contains_key("image_resolution")
    {
        return;
    }
    let raw = text(value);
    let resolution = quality_resolution(&raw);
    if !resolution.is_empty() {
        set_resolution(input, cfg, &json!(resolution));
    }
}
fn quality_value(model: &str, value: &str) -> String {
    let quality = value.trim().to_lowercase();
    if model.starts_with("gpt-image/1.5") {
        if quality == "high" { "high" } else { "medium" }.into()
    } else if model.starts_with("seedream/4.5") || model.starts_with("seedream/5-lite") {
        if matches!(quality.as_str(), "high" | "4k") {
            "high"
        } else {
            "basic"
        }
        .into()
    } else if model.starts_with("seedream/5-pro") {
        if matches!(quality.as_str(), "high" | "2k") {
            "high"
        } else {
            "basic"
        }
        .into()
    } else {
        quality
    }
}
fn motion_mode(value: &Value) -> String {
    if basic_resolution(&text(value)).to_lowercase() == "1080p" {
        "1080p"
    } else {
        "720p"
    }
    .into()
}

fn inferred(key: &str, kind: &str) -> &'static str {
    match kind {
        "image" => match key {
            "image_urls" | "images" => "image_urls",
            "input_urls" | "input_reference" | "input_reference[]" => "input_urls",
            "image_input" => "image_input",
            "reference_image" | "reference_images" => "reference_image",
            "reference_image_url" | "reference_image_urls" => "reference_image_urls",
            "first_frame_url" => "first_frame_url",
            "last_frame_url" => "last_frame_url",
            "end_image_url" => "end_image_url",
            "tail_image_url" => "tail_image_url",
            "image" | "image_url" | "input_url" => "image_url",
            _ => "",
        },
        "video" => match key {
            "video_urls" | "videos" | "input_video_urls" | "video_reference"
            | "video_reference[]" | "input_reference" | "input_reference[]" => "video_urls",
            "reference_video" | "reference_videos" => "reference_video",
            "reference_video_urls" => "reference_video_urls",
            "first_clip_url" => "first_clip_url",
            "video" | "video_url" | "input_video_url" | "reference_video_url" => "video_url",
            _ => "",
        },
        _ => match key {
            "audio_urls" | "audios" | "input_audio_urls" => "audio_urls",
            "reference_audio" | "reference_audios" => "reference_audio",
            "reference_audio_urls" | "audio_reference" | "audio_reference[]" => {
                "reference_audio_urls"
            }
            "driving_audio_url" => "driving_audio_url",
            "reference_voice" => "reference_voice",
            "audio_ids" => "audio_ids",
            "audio" | "audio_url" | "input_audio_url" | "reference_audio_url" => "audio_url",
            _ => "",
        },
    }
}
fn direct_field(key: &str, kind: &str) -> bool {
    match kind {
        "image" => {
            IMAGES.contains(&key)
                && !["image", "images", "input_reference", "input_reference[]"].contains(&key)
        }
        "video" => {
            VIDEOS.contains(&key)
                && !["video", "videos", "video_reference", "video_reference[]"].contains(&key)
        }
        _ => AUDIOS.contains(&key) && !["audio", "audios"].contains(&key),
    }
}
fn source_kind(key: &str, values: &[String], kind: &str) -> bool {
    let (keys, suffixes) = match kind {
        "image" => (IMAGES, &[".png", ".jpg", ".jpeg", ".webp"][..]),
        "video" => (VIDEOS, &[".mp4", ".mov", ".webm"][..]),
        _ => (AUDIOS, &[".mp3", ".wav", ".m4a"][..]),
    };
    keys.contains(&key)
        || values.iter().any(|v| {
            let lower = v.trim().to_lowercase();
            lower.starts_with(&format!("data:{kind}/"))
                || suffixes.iter().any(|s| lower.ends_with(s))
        })
}
fn set_reference(
    input: &mut Object,
    cfg: &Config,
    model: &str,
    key: &str,
    value: &Value,
) -> String {
    if cfg.video_ref_kind == "gemini_video_list" {
        if let Some(items) = value.as_array().filter(|items| {
            !items.is_empty()
                && items.iter().all(|v| {
                    v.as_object().is_some_and(|o| {
                        get(o, "url").as_str().is_some_and(|s| !s.trim().is_empty())
                    })
                })
        }) {
            // A second normalization pass must retain structured references, not stringify them into a URL.
            let field = if direct_field(key, "video") {
                inferred(key, "video")
            } else {
                &cfg.video_ref_field
            };
            if !field.is_empty() {
                insert(input, field, json!(items));
                return field.into();
            }
        }
    }
    let values = strings(value);
    if values.is_empty() {
        return key.into();
    }
    let kind = if source_kind(key, &values, "audio")
        && (!cfg.audio_ref_field.is_empty() || direct_field(key, "audio"))
    {
        "audio"
    } else if source_kind(key, &values, "video")
        && (!cfg.video_ref_field.is_empty() || direct_field(key, "video"))
    {
        "video"
    } else if source_kind(key, &values, "image")
        && (!cfg.image_ref_field.is_empty() || direct_field(key, "image"))
    {
        "image"
    } else if !cfg.video_ref_field.is_empty() {
        "video"
    } else if !cfg.audio_ref_field.is_empty() {
        "audio"
    } else {
        return key.into();
    };
    let (configured, format) = match kind {
        "image" => (&cfg.image_ref_field, &cfg.image_ref_kind),
        "video" => (&cfg.video_ref_field, &cfg.video_ref_kind),
        _ => (&cfg.audio_ref_field, &cfg.audio_ref_kind),
    };
    let mut field = configured.clone();
    if kind == "image" && matches!(key, "first_frame_url" | "last_frame_url") {
        if [
            "bytedance/seedance-2",
            "bytedance/seedance-2-fast",
            "bytedance/seedance-2-mini",
            "bytedance/seedance-2-5",
            "wan/2-7-image-to-video",
        ]
        .contains(&model)
            || configured == "first_frame_url"
        {
            field = key.into();
        } else if [
            "bytedance/v1-lite-image-to-video",
            "hailuo/02-image-to-video-standard",
            "hailuo/02-image-to-video-pro",
            "kling/v2-1-pro",
            "kling/v2-5-turbo-image-to-video-pro",
        ]
        .contains(&model)
            && configured == "image_url"
        {
            field = if key == "first_frame_url" {
                "image_url"
            } else {
                tail_field(model)
            }
            .into();
        } else {
            return key.into();
        }
    } else if direct_field(key, kind) {
        let inferred = inferred(key, kind);
        let prefer = kind == "image"
            && !configured.is_empty()
            && configured != inferred
            && [
                "image",
                "images",
                "image_url",
                "input_url",
                "input_reference",
                "input_reference[]",
                "reference_image",
                "reference_image_url",
            ]
            .contains(&key);
        if !prefer && !inferred.is_empty() {
            field = inferred.into();
        }
    }
    if field.is_empty() {
        field = inferred(key, kind).into();
    }
    if field.is_empty() {
        return key.into();
    }
    if kind == "image" {
        if format == "single_array" {
            insert(input, &field, json!([values[0]]));
        } else if format == "array"
            || [
                "image_urls",
                "input_urls",
                "image_input",
                "reference_image_urls",
            ]
            .contains(&field.as_str())
        {
            let mut seen = HashSet::new();
            let values = strings(get(input, &field))
                .into_iter()
                .chain(values)
                .filter(|v| seen.insert(v.clone()))
                .collect::<Vec<_>>();
            insert(input, &field, json!(values));
        } else {
            insert(input, &field, values[0].clone());
            if values.len() > 1 {
                if field == "image_url" {
                    insert(input, tail_field(model), values[1].clone());
                }
                if field == "first_frame_url" {
                    insert(input, "last_frame_url", values[1].clone());
                }
            }
        }
    } else if kind == "video" && format == "gemini_video_list" {
        insert(
            input,
            &field,
            json!(values
                .iter()
                .map(|v| json!({"url":v,"start":0,"ends":10}))
                .collect::<Vec<_>>()),
        );
    } else if kind == "audio" && field == "audio_ids" {
        let ids = values
            .into_iter()
            .filter(|v| v.starts_with("audio_"))
            .collect::<Vec<_>>();
        if ids.is_empty() {
            return key.into();
        }
        insert(input, &field, json!(ids));
    } else if format == "array"
        || (kind == "video"
            && [
                "video_urls",
                "input_video_urls",
                "reference_video",
                "reference_videos",
                "reference_video_urls",
            ]
            .contains(&field.as_str()))
        || (kind == "audio"
            && [
                "audio_urls",
                "input_audio_urls",
                "reference_audio",
                "reference_audios",
                "reference_audio_urls",
                "audio_ids",
            ]
            .contains(&field.as_str()))
    {
        insert(input, &field, json!(values));
    } else {
        insert(input, &field, values[0].clone());
    }
    field
}
fn tail_field(model: &str) -> &str {
    if model.starts_with("kling/") {
        "tail_image_url"
    } else {
        "end_image_url"
    }
}
fn validate_required(input: &Object, model: &str) -> ApiResult<()> {
    static REQUIRED: OnceLock<HashMap<String, Vec<Vec<String>>>> = OnceLock::new();
    if let Some(groups) = REQUIRED
        .get_or_init(|| serde_json::from_str(include_str!("kie-required-inputs.json")).unwrap())
        .get(model)
    {
        for fields in groups {
            require(
                input,
                fields
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .as_slice(),
            )?;
        }
        return Ok(());
    }
    if model.contains("image-to-video") || model.contains("image_to_video") {
        return require(
            input,
            &[
                "image_url",
                "image_urls",
                "input_urls",
                "first_frame_url",
                "image_input",
            ],
        );
    }
    if [
        "video-to-video",
        "video_to_video",
        "videoedit",
        "video-edit",
        "motion-control",
    ]
    .iter()
    .any(|s| model.contains(s))
    {
        return require(
            input,
            &[
                "video_url",
                "video_urls",
                "input_video_urls",
                "first_clip_url",
                "reference_video",
                "reference_video_urls",
            ],
        );
    }
    Ok(())
}
fn require(input: &Object, fields: &[&str]) -> ApiResult<()> {
    if fields.iter().any(|field| !empty(get(input, field))) {
        Ok(())
    } else {
        Err(format!(
            "KIE required input missing: {}",
            fields.join(" or ")
        ))
    }
}
