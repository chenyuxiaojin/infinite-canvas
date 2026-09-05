use crate::{db::ApiResult, direct::*};
use serde_json::{json, Value};

#[derive(Clone, Default)]
pub struct Config {
    pub aspect_field: &'static str,
    pub duration_field: &'static str,
    pub has_resolution: bool,
    pub resolution_case: &'static str,
    pub max_resolution: &'static str,
    pub min_resolution: &'static str,
    pub has_count: bool,
    pub has_quality: bool,
    pub max_image_refs: usize,
    pub has_output: bool,
    pub mode_from_res: bool,
    pub drop_aspect_with_image: bool,
    pub image_ref_field: &'static str,
    pub image_ref_kind: &'static str,
    pub video_ref_field: &'static str,
    pub video_ref_kind: &'static str,
    pub audio_ref_field: &'static str,
    pub audio_ref_kind: &'static str,
}
pub fn model_name(model: &str) -> String {
    model.trim().to_lowercase().replace(['_', '.', '/'], "-")
}
pub fn motion(model: &str) -> bool {
    matches!(
        model,
        "kling-v2-6-motion-control" | "kling-v3-motion-control"
    )
}
fn video_config(model: &str) -> Config {
    let mut c = Config {
        aspect_field: "aspect_ratio",
        duration_field: "duration",
        has_resolution: true,
        resolution_case: "video",
        image_ref_field: "image_urls",
        image_ref_kind: "array",
        ..Config::default()
    };
    if model.contains("doubao-seedance-2") {
        c.aspect_field = "size";
        c.image_ref_kind = "seedance2";
        c.video_ref_field = "video_urls";
        c.video_ref_kind = "array";
        c.audio_ref_field = "audio_urls";
        c.audio_ref_kind = "array";
    } else if model.contains("doubao-seedance-1-0")
        || model.contains("doubao-seedance-1-5")
        || model.contains("seedance-1")
    {
        c.image_ref_field = "image_with_roles";
        c.image_ref_kind = "roles";
    } else if model.contains("sora-2-pro") {
        c.drop_aspect_with_image = true;
        c.max_image_refs = 1;
    } else if model.contains("sora-2") {
        c.max_resolution = "720p";
        c.drop_aspect_with_image = true;
        c.max_image_refs = 1;
    } else if model.contains("veo") && model.contains("official") {
        c.image_ref_field = "first_frame_image";
        c.image_ref_kind = "first_last";
    } else if model.contains("veo") {
    } else if model == "minimax-h3" {
        c.image_ref_kind = "minimax_h3";
        c.video_ref_field = "video_urls";
        c.video_ref_kind = "array";
        c.audio_ref_field = "audio_urls";
        c.audio_ref_kind = "array";
    } else if model.contains("minimax-hailuo-2-3") {
        c.aspect_field = "";
        c.image_ref_field = "first_frame_image";
        c.image_ref_kind = "first_only";
    } else if model.contains("minimax") || model.contains("hailuo") {
        c.aspect_field = "";
        c.image_ref_field = "first_frame_image";
        c.image_ref_kind = "first_last";
    } else if model.contains("skyreels") {
        c.image_ref_field = "first_frame_image";
        c.image_ref_kind = "skyreels";
        c.video_ref_field = "ref_videos";
        c.video_ref_kind = "skyreels";
        c.audio_ref_kind = "skyreels_ref_images";
    } else if model == "kling-3-0-turbo" {
        c.image_ref_field = "first_frame_image";
        c.image_ref_kind = "first_only";
        c.drop_aspect_with_image = true;
    } else if model == "happyhorse-1-1" {
        c.aspect_field = "size";
        c.resolution_case = "upper_video";
        c.image_ref_kind = "happyhorse11";
    } else if model.contains("happyhorse") {
        c.aspect_field = "size";
        c.resolution_case = "upper_video";
        c.image_ref_kind = "happyhorse";
        c.video_ref_field = "video_url";
        c.video_ref_kind = "single";
    } else if model.contains("gemini-omni-flash-preview") {
        c.max_resolution = "720p";
        c.video_ref_field = "video_urls";
        c.video_ref_kind = "array";
    } else if model.contains("wan2-7-r2v") {
        c.aspect_field = "size";
        c.resolution_case = "upper_video";
        c.image_ref_field = "image_with_roles";
        c.image_ref_kind = "roles";
        c.video_ref_field = "video_urls";
        c.video_ref_kind = "array";
        c.audio_ref_kind = "wan_r2v_voice";
    } else if model.contains("wan2-7-videoedit") {
        c.aspect_field = "size";
        c.resolution_case = "upper_video";
        c.video_ref_field = "video_urls";
        c.video_ref_kind = "array";
    } else if model.contains("wan2-7") {
        c.aspect_field = "size";
        c.resolution_case = "upper_video";
        c.image_ref_field = "image_with_roles";
        c.image_ref_kind = "roles";
        c.video_ref_field = "video_urls";
        c.video_ref_kind = "array";
        c.audio_ref_field = "audio_url";
        c.audio_ref_kind = "single";
    } else if model.contains("wan2-6-i2v-flash") {
        c.aspect_field = "";
        c.audio_ref_field = "audio_url";
        c.audio_ref_kind = "single";
    } else if model.contains("wan2-5") {
        c.aspect_field = "size";
        c.drop_aspect_with_image = true;
        c.audio_ref_field = "audio_url";
        c.audio_ref_kind = "single";
    } else if model.contains("wan2-6") {
        c.drop_aspect_with_image = true;
        c.audio_ref_field = "audio_url";
        c.audio_ref_kind = "single";
    } else if model.contains("kling-v2-6-motion") || model.contains("motion-control") {
        c.aspect_field = "";
        c.has_resolution = false;
        c.image_ref_field = "image_url";
        c.image_ref_kind = "single";
        c.video_ref_field = "video_url";
        c.video_ref_kind = "single";
    } else if model.contains("kling-v2-6") || model.contains("kling-2-6") || model == "kling-v3" {
        c.has_resolution = false;
        c.image_ref_kind = "array_frames";
    } else if model.contains("kling-v3-omni") || model.contains("kling-video-o1") {
        c.has_resolution = false;
        c.mode_from_res = true;
        c.video_ref_field = "video_list";
        c.video_ref_kind = "kling_video_list";
    } else if model.contains("kling") {
        c.has_resolution = false;
        c.mode_from_res = true;
    } else if model.contains("vidu") {
        c.drop_aspect_with_image = model != "viduq3" && model != "viduq3-mix";
        c.image_ref_kind = "array_frames";
    } else if model.contains("grok-imagine") {
        c.aspect_field = "size";
        c.has_resolution = false;
        c.has_quality = true;
    } else if model.contains("pixverse") {
        c.aspect_field = "size";
        c.image_ref_kind = "pixverse";
    } else if model.contains("omni-flash") {
        c.video_ref_field = "video_urls";
        c.video_ref_kind = "array";
    } else if model.contains("flux-3-video") {
        c.max_image_refs = 10;
        c.video_ref_field = "video_url";
        c.video_ref_kind = "single";
    }
    if motion(model) {
        c.duration_field = "";
    }
    c
}
fn image_config(model: &str) -> Config {
    let mut c = Config {
        aspect_field: "size",
        has_resolution: true,
        resolution_case: "upper",
        has_count: true,
        image_ref_field: "image_urls",
        image_ref_kind: "array",
        ..Config::default()
    };
    if model.contains("gpt-image-2") && model.contains("official") {
        c.resolution_case = "lower";
        c.has_quality = true;
        c.has_output = true;
    } else if model.contains("gpt-image-2") {
        c.resolution_case = "lower";
        c.has_quality = true;
    } else if model.contains("gpt-4o-image") {
        c.has_resolution = false;
    } else if model.contains("gpt-image-1") {
        c.has_resolution = false;
        c.has_quality = true;
        c.has_output = true;
    } else if model.contains("gemini-3-1-flash-lite") {
        c.max_resolution = "1K";
    } else if model.contains("gemini-3-1")
        || model.contains("gemini-31")
        || model.contains("nano-banana2")
        || model.contains("gemini-3-pro")
        || model.contains("nano-banana-pro")
    {
        c.has_count = false;
    } else if model.contains("gemini-2-5") || model.contains("nano-banana") {
        c.max_resolution = "1K";
        c.has_count = false;
    } else if model.contains("imagen") {
        c.has_resolution = false;
        c.has_count = false;
        c.image_ref_field = "";
    } else if model.contains("seedream-5-0-pro") {
        c.max_resolution = "2K";
        c.has_count = false;
        c.max_image_refs = 10;
    } else if model.contains("seedream-5") {
        c.min_resolution = "2K";
        c.has_output = true;
    } else if model.contains("seedream-4-5") || model.contains("seedance-4-5") {
        c.min_resolution = "2K";
    } else if model.contains("seedream") || model.contains("seedance-4") {
    } else if model.contains("qwen") {
        c.max_resolution = "2K";
    } else if model.contains("z-image") {
        c.max_resolution = "2K";
        c.has_count = false;
        c.image_ref_field = "";
    } else if model.contains("grok-imagine") {
        c.has_resolution = false;
    } else if model.contains("wan2-7") {
    } else if model.contains("flux-2") {
        c.has_count = false;
    }
    c
}
pub fn normalize(model: &str, body: &Value, video: bool) -> ApiResult<Value> {
    let mut payload = body.as_object().cloned().unwrap_or_default();
    insert(&mut payload, "model", model.trim());
    let name = model_name(model);
    let cfg = if video {
        video_config(&name)
    } else {
        image_config(&name)
    };
    if video {
        aspect(&mut payload, &cfg);
        duration(&mut payload, &cfg);
        video_mode(&mut payload, &cfg);
        kling_advanced(&mut payload, &name);
        resolution(&mut payload, &cfg);
        if cfg.has_quality {
            let value = first(&payload, &["quality", "resolution", "resolution_name"]);
            if !value.is_empty() {
                insert(&mut payload, "quality", video_resolution(&value, &cfg));
            }
            remove(&mut payload, &["resolution", "resolution_name"]);
        }
        crate::apimart_refs::normalize(&mut payload, &cfg);
        generate_audio(&mut payload, &name);
        defaults(&mut payload, &name);
        conflicts(&mut payload, &name);
        validate_video(&payload, &name)?;
        if cfg.drop_aspect_with_image
            && [
                "image_urls",
                "image_with_roles",
                "first_frame_image",
                "last_frame_image",
                "end_frame_image",
                "img_references",
                "ref_images",
            ]
            .iter()
            .any(|key| !empty(get(&payload, key)))
        {
            remove(
                &mut payload,
                &["aspect_ratio", "size", "ratio", "image_size"],
            );
        }
        payload.remove("preset");
    } else {
        resolution(&mut payload, &cfg);
        aspect(&mut payload, &cfg);
        count(&mut payload, &cfg);
        if cfg.has_quality {
            let quality = text(get(&payload, "quality")).trim().to_lowercase();
            if !quality.is_empty() {
                insert(&mut payload, "quality", quality);
            }
        } else {
            payload.remove("quality");
        }
        if cfg.has_output {
            let format = first(&payload, &["output_format", "format"]);
            if !format.is_empty() {
                insert(
                    &mut payload,
                    "output_format",
                    if format.eq_ignore_ascii_case("jpg") {
                        "jpeg".into()
                    } else {
                        format.to_lowercase()
                    },
                );
            }
        } else {
            payload.remove("output_format");
        }
        payload.remove("format");
        if matches!(
            name.as_str(),
            "grok-imagine-1-5-apimart" | "imagen-4-0-apimart"
        ) {
            remove(&mut payload, crate::apimart_refs::IMAGE_KEYS);
        } else {
            crate::apimart_refs::normalize(&mut payload, &cfg);
        }
        if name.contains("grok-imagine") && name.contains("edit") {
            require(&payload, &["image_urls"])?;
        }
    }
    Ok(json!(payload))
}
fn aspect(payload: &mut Object, cfg: &Config) {
    if cfg.aspect_field.is_empty() {
        remove(payload, &["size", "ratio", "aspect_ratio"]);
        return;
    }
    let value = first(
        payload,
        &[
            cfg.aspect_field,
            "size",
            "aspect_ratio",
            "ratio",
            "image_size",
        ],
    );
    if !value.is_empty() {
        insert(payload, cfg.aspect_field, ratio(&value));
    }
    if cfg.aspect_field != "size" {
        payload.remove("size");
    }
    if cfg.aspect_field != "aspect_ratio" {
        payload.remove("aspect_ratio");
    }
    remove(payload, &["ratio", "image_size"]);
}
pub fn integer(value: &Value) -> i64 {
    if value.is_number() {
        number(value)
    } else {
        text(value)
            .trim()
            .to_lowercase()
            .trim_end_matches('s')
            .parse()
            .unwrap_or_default()
    }
}
fn duration(payload: &mut Object, cfg: &Config) {
    if cfg.duration_field.is_empty() {
        remove(payload, &["duration", "seconds"]);
        return;
    }
    let value = [cfg.duration_field, "duration", "seconds"]
        .iter()
        .map(|key| get(payload, key))
        .find(|v| !empty(v))
        .cloned();
    if let Some(value) = value {
        insert(payload, cfg.duration_field, integer(&value));
    }
    if cfg.duration_field != "duration" {
        payload.remove("duration");
    }
    payload.remove("seconds");
}
fn count(payload: &mut Object, cfg: &Config) {
    let fields = ["n", "num_images", "max_images", "actual_image_count"];
    if !cfg.has_count {
        remove(payload, &fields);
        return;
    }
    let value = fields
        .iter()
        .map(|key| get(payload, key))
        .find(|v| !empty(v))
        .cloned();
    if let Some(value) = value {
        insert(payload, "n", integer(&value));
    }
    remove(payload, &fields[1..]);
}
fn resolution(payload: &mut Object, cfg: &Config) {
    if !cfg.has_resolution {
        if !cfg.has_quality {
            remove(payload, &["resolution", "resolution_name"]);
        }
        payload.remove("image_resolution");
        return;
    }
    let mut value = first(
        payload,
        &["resolution", "resolution_name", "image_resolution"],
    );
    if !matches!(cfg.resolution_case, "video" | "upper_video") && value.is_empty() {
        value = size_resolution(&text(get(payload, "size")));
        if value.is_empty() {
            value = quality_resolution(&text(get(payload, "quality"))).into();
        }
    }
    if !value.is_empty() {
        let value = if cfg.resolution_case == "video" {
            video_resolution(&value, cfg)
        } else if cfg.resolution_case == "upper_video" {
            video_resolution(&value, cfg).to_uppercase()
        } else {
            let level = resolution_level(&value);
            let value = if level > 0 {
                let max = resolution_level(cfg.max_resolution);
                let min = resolution_level(cfg.min_resolution);
                let n = if max > 0 { level.min(max) } else { level };
                format!("{}K", if min > 0 { n.max(min) } else { n })
            } else {
                value
            };
            image_resolution(&value, cfg.resolution_case)
        };
        insert(payload, "resolution", value);
    }
    remove(payload, &["image_resolution", "resolution_name"]);
}
fn video_mode(payload: &mut Object, cfg: &Config) {
    if !cfg.mode_from_res {
        return;
    }
    let mut mode = text(get(payload, "mode")).trim().to_lowercase();
    if mode.is_empty() || mode == "normal" {
        let resolution = video_resolution(
            &first(payload, &["resolution", "resolution_name"]),
            &Config::default(),
        );
        mode = if matches!(resolution.as_str(), "1080p" | "4k") {
            "pro"
        } else {
            "std"
        }
        .into();
    }
    insert(payload, "mode", mode);
}
fn parse_size(value: &str) -> Option<(u64, u64)> {
    let v = value.trim().to_lowercase();
    let pair = v
        .split(if v.contains('*') { '*' } else { 'x' })
        .collect::<Vec<_>>();
    if pair.len() != 2 {
        return None;
    }
    let (w, h) = (
        pair[0].trim().parse::<u64>().ok()?,
        pair[1].trim().parse::<u64>().ok()?,
    );
    if w == 0 || h == 0 {
        None
    } else {
        Some((w, h))
    }
}
fn ratio(value: &str) -> String {
    let value = value.trim().to_lowercase();
    if value.is_empty() || value == "auto" {
        return "auto".into();
    }
    if let Some((w, h)) = parse_size(&value) {
        for (a, b) in [
            (1, 1),
            (2, 1),
            (1, 2),
            (3, 1),
            (1, 3),
            (5, 4),
            (4, 5),
            (16, 9),
            (9, 16),
            (4, 3),
            (3, 4),
            (3, 2),
            (2, 3),
            (21, 9),
            (9, 21),
        ] {
            let x = w as u128 * b;
            let y = h as u128 * a;
            if x.abs_diff(y) * 100 <= x * 4 {
                return format!("{a}:{b}");
            }
        }
    }
    value
}
fn size_resolution(value: &str) -> String {
    let Some((w, h)) = parse_size(value) else {
        return String::new();
    };
    match w.max(h) {
        3500.. => "4K",
        1700.. => "2K",
        900.. => "1K",
        _ => "",
    }
    .into()
}
fn quality_resolution(value: &str) -> &str {
    match value.trim().to_lowercase().as_str() {
        "low" | "standard" => "1K",
        "medium" | "hd" => "2K",
        "high" | "uhd" => "4K",
        _ => "",
    }
}
fn resolution_level(value: &str) -> u8 {
    match value.trim().to_lowercase().as_str() {
        "0.5" | "0.5k" | "512" | "512p" | "1" | "1k" | "1024" | "1024p" | "low" | "standard" => 1,
        "2" | "2k" | "2048" | "2048p" | "medium" | "hd" => 2,
        "3" | "3k" | "3072" => 3,
        "4" | "4k" | "4096" | "4096p" | "high" | "uhd" => 4,
        _ => 0,
    }
}
pub fn video_resolution(value: &str, cfg: &Config) -> String {
    let v = value.trim().to_lowercase();
    let result = match v.as_str() {
        "480" | "480p" | "sd" | "low" => "480p",
        "512" | "512p" => "512p",
        "540" | "540p" => "540p",
        "720" | "720p" | "hd" | "medium" | "standard" => "720p",
        "768" | "768p" => "768p",
        "1080" | "1080p" | "fhd" | "high" | "pro" => "1080p",
        "2160" | "2160p" | "4k" | "uhd" => "4k",
        "360" | "360p" => "360p",
        _ => &v,
    };
    if cfg.max_resolution == "720p" && matches!(result, "1080p" | "4k") {
        "720p".into()
    } else {
        result.into()
    }
}
fn image_resolution(value: &str, mode: &str) -> String {
    let value = value.trim().to_lowercase();
    let value = value.trim_end_matches("px");
    let result = match value {
        "0.5" | "0.5k" | "512" | "512p" => "0.5k",
        "1" | "1k" | "1024" | "1024p" | "low" | "standard" => "1k",
        "2" | "2k" | "2048" | "2048p" | "medium" | "hd" => "2k",
        "3" | "3k" | "3072" => "3k",
        "4" | "4k" | "4096" | "4096p" | "high" | "uhd" => "4k",
        _ => value,
    };
    if mode == "lower" {
        result.into()
    } else {
        result.to_uppercase()
    }
}
fn kling_advanced(payload: &mut Object, model: &str) {
    if model != "kling-v3" {
        return;
    }
    let elements=get(payload,"element_list").as_array().into_iter().flatten().filter_map(|v|v.as_object()).filter_map(|o|{let values=crate::apimart_refs::collect(get(o,"element_input_urls"),0).into_iter().take(4).collect::<Vec<_>>();if values.is_empty(){None}else{Some(json!({"name":text(get(o,"name")).trim(),"description":text(get(o,"description")).trim(),"element_input_urls":values}))}}).take(3).collect::<Vec<_>>();
    if elements.is_empty() {
        payload.remove("element_list");
    } else {
        insert(payload, "element_list", json!(elements));
    }
    if !bool_like(get(payload, "multi_shot")) {
        remove(payload, &["multi_shot", "shot_type", "multi_prompt"]);
        return;
    }
    insert(payload, "multi_shot", true);
    if !text(get(payload, "shot_type"))
        .trim()
        .eq_ignore_ascii_case("customize")
    {
        insert(payload, "shot_type", "intelligence");
        payload.remove("multi_prompt");
        return;
    }
    insert(payload, "shot_type", "customize");
    let items = get(payload, "multi_prompt")
        .as_array()
        .cloned()
        .filter(|xs| !xs.is_empty())
        .unwrap_or(vec![json!({"prompt":"","duration":1})]);
    insert(payload,"multi_prompt",json!(items.iter().enumerate().map(|(i,v)|json!({"index":i+1,"prompt":text(&v["prompt"]),"duration":integer(&v["duration"]).clamp(1,15)})).collect::<Vec<_>>()));
}
fn defaults(payload: &mut Object, model: &str) {
    if model == "flux-3-video" {
        if matches!(
            text(get(payload, "resolution"))
                .trim()
                .to_lowercase()
                .as_str(),
            "360p" | "360" | "480p" | "480"
        ) {
            insert(payload, "resolution", "720p");
        }
        if !empty(get(payload, "duration")) {
            insert(
                payload,
                "duration",
                integer(get(payload, "duration")).clamp(5, 20),
            );
        }
    }
    if model == "minimax-h3" {
        let value = if matches!(
            text(get(payload, "resolution")).as_str(),
            "480p" | "720p" | "768p"
        ) {
            "768P"
        } else {
            "2K"
        };
        insert(payload, "resolution", value);
        if !empty(get(payload, "duration")) {
            insert(
                payload,
                "duration",
                integer(get(payload, "duration")).clamp(4, 15),
            );
        }
    }
    if model.contains("wan2-5") && empty(get(payload, "audio")) {
        insert(payload, "audio", true);
    }
    if motion(model) {
        remove(payload, &["keep_original_sound", "watermark_info"]);
        if empty(get(payload, "character_orientation")) {
            insert(payload, "character_orientation", "video");
        }
        if empty(get(payload, "mode")) {
            insert(payload, "mode", "std");
        }
        return;
    }
    if model.contains("motion-control") {
        for (key, value) in [
            ("character_orientation", "image"),
            ("mode", "std"),
            ("keep_original_sound", "yes"),
        ] {
            if empty(get(payload, key)) {
                insert(payload, key, value);
            }
        }
    }
}
fn generate_audio(payload: &mut Object, model: &str) {
    let Some(value) = payload.remove("video_generate_audio") else {
        return;
    };
    let enabled = bool_like(&value);
    if model.contains("doubao-seedance-2") || model.contains("veo") && model.contains("official") {
        insert(payload, "generate_audio", enabled);
    } else if model.contains("doubao-seedance-1-5")
        || model.contains("seedance-1-5")
        || matches!(model, "wan2-6" | "wan2-6-i2v-flash")
    {
        insert(payload, "audio", enabled);
    } else if model.contains("kling-v3-omni") {
        if empty(get(payload, "video_list")) {
            insert(payload, "audio", enabled);
        }
    } else if model.contains("kling-v3") && !model.contains("omni")
        || ["pixverse-v6", "viduq3-pro", "vidu-q3-pro", "viduq3-turbo"]
            .iter()
            .any(|s| model.contains(s))
    {
        insert(payload, "audio", enabled);
    } else if (model.contains("kling-v2-6") || model.contains("kling-2-6"))
        && !model.contains("motion")
    {
        if enabled {
            if !crate::apimart_refs::has_last_frame(payload) {
                insert(payload, "audio", true);
                if empty(get(payload, "mode")) {
                    insert(payload, "mode", "pro");
                }
            }
        } else {
            insert(payload, "audio", false);
        }
    }
}
fn conflicts(payload: &mut Object, model: &str) {
    if model == "happyhorse-1-1" && !empty(get(payload, "first_frame_image")) {
        payload.remove("image_urls");
    }
    if model.contains("doubao-seedance-2") && !empty(get(payload, "image_with_roles")) {
        payload.remove("image_urls");
        if get(payload, "image_with_roles")
            .as_array()
            .is_some_and(|xs| {
                xs.iter()
                    .any(|x| matches!(x["role"].as_str(), Some("first_frame" | "last_frame")))
            })
        {
            remove(payload, &["video_urls", "audio_urls"]);
        }
    }
    if model.contains("wan2-7")
        && !model.contains("r2v")
        && !model.contains("videoedit")
        && !empty(get(payload, "video_urls"))
    {
        payload.remove("audio_url");
    }
    if model.contains("omni-flash-ext") && !empty(get(payload, "video_urls")) {
        payload.remove("duration");
    }
}
fn validate_video(payload: &Object, model: &str) -> ApiResult<()> {
    if model == "kling-3-0-turbo" {
        require(payload, &["prompt", "first_frame_image"])
    } else if model == "happyhorse-1-1" {
        if strings(get(payload, "image_urls")).len() > 9 {
            return Err("图片数量最多9张".into());
        }
        require(payload, &["prompt", "first_frame_image", "image_urls"])
    } else if model.contains("motion-control") {
        if empty(get(payload, "image_url")) || empty(get(payload, "video_url")) {
            Err("motion-control 模型缺少参考图和参考视频".into())
        } else {
            Ok(())
        }
    } else if model.contains("minimax-hailuo-2-3-fast") {
        require(payload, &["first_frame_image"])
    } else if model.contains("wan2-7-videoedit") {
        require(payload, &["video_urls"])
    } else if model.contains("wan2-7-r2v") {
        require(payload, &["image_with_roles", "video_urls"])
    } else if model.contains("wan2-6-i2v-flash") || matches!(model, "viduq3" | "viduq3-mix") {
        require(payload, &["image_urls"])
    } else {
        Ok(())
    }
}
fn require(payload: &Object, fields: &[&str]) -> ApiResult<()> {
    if fields.iter().any(|key| !empty(get(payload, key))) {
        Ok(())
    } else {
        Err(format!(
            "APIMart required input missing: {}",
            fields.join(" or ")
        ))
    }
}
