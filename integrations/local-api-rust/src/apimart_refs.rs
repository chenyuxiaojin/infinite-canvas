//! APIMart reference shapes. Only URL metadata enters this module; files stay in the frontend.
use crate::{apimart::Config, direct::*};
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub const IMAGE_KEYS: &[&str] = &[
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
    "first_frame_image",
    "last_frame_url",
    "last_frame_image",
];
const VIDEO_KEYS: &[&str] = &[
    "video",
    "videos",
    "video_url",
    "video_urls",
    "input_video_url",
    "input_video_urls",
    "video_reference",
    "video_reference[]",
    "reference_video_url",
    "reference_video_urls",
];
const AUDIO_KEYS: &[&str] = &[
    "audio",
    "audios",
    "audio_url",
    "audio_urls",
    "input_audio_url",
    "input_audio_urls",
    "audio_reference",
    "audio_reference[]",
    "reference_audio_url",
    "reference_audio_urls",
];
pub fn collect(value: &Value, depth: usize) -> Vec<String> {
    if depth > 6 || value.is_null() {
        return vec![];
    }
    match value {
        Value::Array(xs) => xs.iter().flat_map(|v| collect(v, depth + 1)).collect(),
        Value::Object(o) => {
            let url = first(
                o,
                &[
                    "url",
                    "image_url",
                    "imageUrl",
                    "video_url",
                    "videoUrl",
                    "download_url",
                    "downloadUrl",
                ],
            );
            if !url.is_empty() {
                vec![url]
            } else {
                o.values().flat_map(|v| collect(v, depth + 1)).collect()
            }
        }
        _ => {
            let value = text(value);
            if value.trim().is_empty() {
                vec![]
            } else {
                vec![value]
            }
        }
    }
}
fn merge(existing: &Value, values: &[String]) -> Value {
    let mut seen = BTreeSet::new();
    json!(collect(existing, 0)
        .iter()
        .chain(values)
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty() && seen.insert(s.clone()))
        .collect::<Vec<_>>())
}
fn first_frame(key: &str) -> bool {
    matches!(key, "first_frame_url" | "first_frame_image")
}
fn last_frame(key: &str) -> bool {
    matches!(key, "last_frame_url" | "last_frame_image")
}
fn source(key: &str, values: &[String], keys: &[&str], kind: &str, extensions: &[&str]) -> bool {
    keys.contains(&key)
        || values.iter().any(|v| {
            let v = v.trim().to_lowercase();
            v.starts_with(&format!("data:{kind}/")) || extensions.iter().any(|e| v.ends_with(e))
        })
}
pub fn normalize(payload: &mut Object, cfg: &Config) {
    for key in IMAGE_KEYS.iter().chain(VIDEO_KEYS).chain(AUDIO_KEYS) {
        let values = collect(get(payload, key), 0)
            .iter()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        if values.is_empty() {
            continue;
        }
        let direct_audio = matches!(
            *key,
            "audio_url"
                | "audio_urls"
                | "input_audio_url"
                | "input_audio_urls"
                | "reference_audio_url"
                | "reference_audio_urls"
        );
        let direct_video = matches!(
            *key,
            "video_url"
                | "video_urls"
                | "input_video_url"
                | "input_video_urls"
                | "reference_video_url"
                | "reference_video_urls"
        );
        let direct_image = matches!(
            *key,
            "image_url"
                | "image_urls"
                | "input_url"
                | "input_urls"
                | "image_input"
                | "reference_image_url"
                | "reference_image_urls"
                | "first_frame_url"
                | "first_frame_image"
                | "last_frame_url"
                | "last_frame_image"
        );
        if source(key, &values, AUDIO_KEYS, "audio", &[".mp3", ".wav", ".m4a"])
            && (!cfg.audio_ref_field.is_empty() || direct_audio)
        {
            set_audio(payload, cfg, key, &values);
        } else if source(
            key,
            &values,
            VIDEO_KEYS,
            "video",
            &[".mp4", ".mov", ".webm"],
        ) && (!cfg.video_ref_field.is_empty() || direct_video)
        {
            set_video(payload, cfg, key, &values);
        } else if source(
            key,
            &values,
            IMAGE_KEYS,
            "image",
            &[".png", ".jpg", ".jpeg", ".webp"],
        ) && (!cfg.image_ref_field.is_empty() || direct_image)
        {
            set_image(payload, cfg, key, &values);
        }
    }
    let mut keep = BTreeSet::from([
        cfg.image_ref_field,
        cfg.video_ref_field,
        cfg.audio_ref_field,
        "first_frame_image",
        "last_frame_image",
    ]);
    match cfg.image_ref_kind {
        "pixverse" => keep.extend(["image_urls", "img_references"]),
        "skyreels" => keep.extend(["end_frame_image", "ref_images"]),
        "happyhorse" => {
            keep.insert("image_urls");
        }
        "seedance2" => keep.extend(["image_urls", "image_with_roles"]),
        "roles" => {
            keep.insert("image_with_roles");
        }
        _ => {}
    }
    match cfg.video_ref_kind {
        "skyreels" => {
            keep.insert("ref_videos");
        }
        "kling_video_list" => {
            keep.insert("video_list");
        }
        _ => {}
    }
    for key in IMAGE_KEYS.iter().chain(VIDEO_KEYS).chain(AUDIO_KEYS) {
        if !keep.contains(key) {
            payload.remove(*key);
        }
    }
}
fn set_image(payload: &mut Object, cfg: &Config, key: &str, values: &[String]) {
    let mut field = match key {
        "first_frame_url" | "first_frame_image" => "first_frame_image",
        "last_frame_url" | "last_frame_image" => "last_frame_image",
        "image_url" | "input_url" | "reference_image_url" => "image_url",
        "image_urls"
        | "images"
        | "input_urls"
        | "image_input"
        | "reference_images"
        | "reference_image_urls" => "image_urls",
        _ => "",
    };
    if cfg.image_ref_field == "image_urls"
        && cfg.image_ref_kind == "array"
        && matches!(
            key,
            "image"
                | "image_url"
                | "input_url"
                | "input_reference"
                | "input_reference[]"
                | "reference_image"
                | "reference_image_url"
        )
    {
        field = cfg.image_ref_field;
    }
    if field.is_empty() {
        field = cfg.image_ref_field;
    }
    if field.is_empty() {
        return;
    }
    let values = if cfg.max_image_refs > 0 {
        &values[..values.len().min(cfg.max_image_refs)]
    } else {
        values
    };
    let named = first_frame(key) || last_frame(key);
    if matches!(cfg.image_ref_kind, "seedance2" | "roles") && named {
        append_role(payload, key, &values[0]);
        return;
    }
    if cfg.image_ref_kind == "array_frames" && named {
        let mut items = collect(get(payload, "image_urls"), 0);
        let index = if first_frame(key) { 0 } else { 1 };
        if items.len() <= index {
            items.push(values[0].clone());
        } else {
            items[index] = values[0].clone();
        }
        insert(payload, "image_urls", json!(items));
        return;
    }
    if cfg.image_ref_kind == "happyhorse11" {
        if first_frame(key) {
            insert(payload, "first_frame_image", values[0].clone());
        } else {
            let value = merge(get(payload, "image_urls"), values);
            insert(payload, "image_urls", value);
        }
        return;
    }
    if cfg.image_ref_kind == "first_only" && first_frame(key) {
        insert(payload, "first_frame_image", values[0].clone());
        return;
    }
    let supports = matches!(
        cfg.image_ref_kind,
        "first_last" | "skyreels" | "pixverse" | "happyhorse" | "minimax_h3"
    );
    if named && !supports {
        return;
    }
    if supports && named {
        if first_frame(key) {
            insert(payload, "first_frame_image", values[0].clone());
        } else if cfg.image_ref_kind == "skyreels" {
            insert(payload, "end_frame_image", values[0].clone());
        } else if cfg.image_ref_kind != "happyhorse" {
            insert(payload, "last_frame_image", values[0].clone());
        }
        return;
    }
    match cfg.image_ref_kind{
        "pixverse"=>insert(payload,if values.len()>1{"img_references"}else{"image_urls"},json!(values)),
        "skyreels"=>insert(payload,"ref_images",json!([{"tag":"@image1","type":"image","image_urls":values}])),
        "happyhorse"=>{if values.len()==1{insert(payload,"first_frame_image",values[0].clone());}else{insert(payload,"image_urls",json!(values));}},
        "roles"=>insert(payload,"image_with_roles",json!(values.iter().enumerate().map(|(i,v)|json!({"url":v,"role":if i==0{"first_frame"}else if i==1{"last_frame"}else{"reference_image"}})).collect::<Vec<_>>())),
        "array_frames"=>{let value=merge(get(payload,field),values);insert(payload,field,value);},
        "first_only"=>insert(payload,"first_frame_image",values[0].clone()),
        _=>{
            if field=="first_frame_image"||cfg.image_ref_kind=="first_last"{insert(payload,"first_frame_image",values[0].clone());if values.len()>1{insert(payload,"last_frame_image",values[1].clone());}}
            else if cfg.image_ref_kind=="single"||field=="image_url"{insert(payload,field,values[0].clone());}
            else{let value=merge(get(payload,field),values);insert(payload,field,value);}
        }
    }
}
fn append_role(payload: &mut Object, key: &str, value: &str) {
    let role = if first_frame(key) {
        "first_frame"
    } else if last_frame(key) {
        "last_frame"
    } else {
        "reference_image"
    };
    let mut roles = get(payload, "image_with_roles")
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_object())
        .filter_map(|o| {
            let url = text(get(o, "url")).trim().to_owned();
            let role = text(get(o, "role")).trim().to_owned();
            if url.is_empty() || role.is_empty() {
                None
            } else {
                Some(json!({"url":url,"role":role}))
            }
        })
        .collect::<Vec<_>>();
    let replacement = json!({"url":value,"role":role});
    if let Some(item) = roles.iter_mut().find(|v| v["role"] == role) {
        *item = replacement;
    } else {
        roles.push(replacement);
    }
    insert(payload, "image_with_roles", json!(roles));
}
fn set_video(payload: &mut Object, cfg: &Config, key: &str, values: &[String]) {
    let field = match key {
        "video" | "video_url" | "input_video_url" | "reference_video_url" => "video_url",
        "videos" | "video_urls" | "input_video_urls" | "reference_video_urls" => "video_urls",
        _ => cfg.video_ref_field,
    };
    if field.is_empty() {
        return;
    }
    match cfg.video_ref_kind {
        "skyreels" => insert(
            payload,
            "ref_videos",
            json!(values
                .iter()
                .enumerate()
                .map(
                    |(i, v)| json!({"tag":format!("@video{}",i+1),"type":"reference","video_url":v})
                )
                .collect::<Vec<_>>()),
        ),
        "kling_video_list" => insert(
            payload,
            "video_list",
            json!(values
                .iter()
                .map(|v| json!({"video_url":v,"refer_type":"base","keep_original_sound":"no"}))
                .collect::<Vec<_>>()),
        ),
        _ => {
            if cfg.video_ref_kind == "single" || field == "video_url" {
                insert(payload, field, values[0].clone());
            } else {
                insert(payload, field, json!(values));
            }
        }
    }
}
fn set_audio(payload: &mut Object, cfg: &Config, key: &str, values: &[String]) {
    if matches!(cfg.audio_ref_kind, "skyreels_ref_images" | "wan_r2v_voice") {
        let (field, property) = if cfg.audio_ref_kind == "skyreels_ref_images" {
            ("ref_images", "audio_url")
        } else {
            ("image_with_roles", "reference_voice")
        };
        let mut items = get(payload, field)
            .as_array()
            .into_iter()
            .flatten()
            .filter(|v| v.is_object())
            .cloned()
            .collect::<Vec<_>>();
        if let Some(item) = items.first_mut() {
            item[property] = json!(values[0]);
            insert(payload, field, json!(items));
        }
        return;
    }
    let field = match key {
        "audio" | "audio_url" | "input_audio_url" | "reference_audio_url" => "audio_url",
        "audios" | "audio_urls" | "input_audio_urls" | "reference_audio_urls" => "audio_urls",
        _ => cfg.audio_ref_field,
    };
    if field.is_empty() {
        return;
    }
    if cfg.audio_ref_kind == "array" || matches!(field, "audio_urls" | "reference_audio_urls") {
        insert(payload, field, json!(values));
    } else {
        insert(payload, field, values[0].clone());
    }
}
pub fn has_last_frame(payload: &Object) -> bool {
    !empty(get(payload, "last_frame_image"))
        || !empty(get(payload, "end_frame_image"))
        || get(payload, "image_with_roles")
            .as_array()
            .is_some_and(|xs| {
                xs.iter()
                    .any(|v| v["role"].as_str().is_some_and(|s| s.trim() == "last_frame"))
            })
}
