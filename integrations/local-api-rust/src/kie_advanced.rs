use crate::direct::*;
use serde_json::{json, Value};

pub fn multi_prompt(value: &Value, max_duration: i64) -> Vec<Value> {
    value.as_array().into_iter().flatten().filter_map(|v|v.as_object()).map(|record|json!({"prompt":text(get(record,"prompt")).trim(),"duration":duration_text(get(record,"duration")).parse::<i64>().unwrap_or(1).clamp(1,max_duration)})).collect()
}
pub fn elements(value: &Value) -> Vec<Value> {
    value.as_array().into_iter().flatten().filter_map(|v|v.as_object()).filter_map(|record|{
        let mut input_urls=strings(get(record,"element_input_urls"));let mut audio_urls=strings(get(record,"element_input_audio_urls"));
        if let Some(refs)=get(record,"references").as_array(){for reference in refs{let Some(reference)=reference.as_object()else{continue;};let url=text(get(reference,"url")).trim().to_owned();if url.is_empty(){continue;}if text(get(reference,"kind")).trim().eq_ignore_ascii_case("audio"){audio_urls.push(url);}else{input_urls.push(url);}}}
        if input_urls.is_empty()&&audio_urls.is_empty(){return None;}
        let mut out=json!({"name":text(get(record,"name")).trim(),"description":text(get(record,"description")).trim()});if !input_urls.is_empty(){out["element_input_urls"]=json!(input_urls);}if !audio_urls.is_empty(){out["element_input_audio_urls"]=json!(audio_urls);}Some(out)
    }).collect()
}
fn normalized_list(input: &mut Object, key: &str, values: Vec<Value>) {
    if values.is_empty() {
        input.remove(key);
    } else {
        insert(input, key, json!(values));
    }
}
pub fn v3(input: &mut Object, model: &str) {
    if model != "kling-3.0/video" {
        return;
    }
    remove(input, &["negative_prompt", "shot_type"]);
    if !empty(get(input, "mode")) {
        let value = text(get(input, "mode"));
        insert(
            input,
            "mode",
            match value.trim().to_lowercase().as_str() {
                "4k" => "4K",
                "pro" => "pro",
                _ => "std",
            },
        );
    }
    if let Some(value) = input.remove("multi_shot") {
        insert(input, "multi_shots", bool_like(&value));
    }
    if let Some(value) = input.get("multi_shots").cloned() {
        insert(input, "multi_shots", bool_like(&value));
    }
    if let Some(value) = input.get("multi_prompt").cloned() {
        normalized_list(input, "multi_prompt", multi_prompt(&value, 12));
    }
    if let Some(value) = input.remove("element_list") {
        let values = elements(&value);
        if !values.is_empty() {
            insert(input, "kling_elements", json!(values));
        }
    }
    if let Some(value) = input.get("kling_elements").cloned() {
        normalized_list(input, "kling_elements", elements(&value));
    }
}
pub fn omni(input: &mut Object, model: &str) {
    let Some(variant) = model.strip_prefix("kling-3.0-omni/") else {
        return;
    };
    if ![
        "text-to-video",
        "image-to-video",
        "reference-to-video",
        "transformation",
    ]
    .contains(&variant)
    {
        return;
    }
    input.remove("negative_prompt");
    for key in ["resolution", "mode"] {
        if !empty(get(input, key)) {
            let value = text(get(input, key));
            insert(
                input,
                "resolution",
                match value.trim().to_lowercase().as_str() {
                    "4k" => "4k",
                    "pro" | "1080" | "1080p" => "1080p",
                    _ => "720p",
                },
            );
        }
    }
    input.remove("mode");
    if let Some(value) = input.remove("element_list") {
        normalized_list(input, "elements", elements(&value));
    } else if let Some(value) = input.get("elements").cloned() {
        normalized_list(input, "elements", elements(&value));
    }
    input.remove("kling_elements");
    let multi_shot = input
        .get("multi_shot")
        .or_else(|| input.get("multi_shots"))
        .cloned();
    remove(input, &["multi_shot", "multi_shots"]);
    let shot_type = text(get(input, "shot_type")).trim().to_lowercase();
    input.remove("shot_type");
    match variant {
        "text-to-video" | "image-to-video" => {
            let mut custom = bool_like(get(input, "customize_multi_shots"));
            let mut smart = bool_like(get(input, "prefer_multi_shots"));
            if let Some(value) = multi_shot {
                custom = bool_like(&value) && shot_type == "customize";
                smart = bool_like(&value) && !custom;
            }
            if custom {
                smart = false;
            }
            insert(input, "customize_multi_shots", custom);
            insert(input, "prefer_multi_shots", smart);
            if !custom {
                input.remove("multi_prompt");
            }
        }
        "reference-to-video" => {
            let custom = multi_shot
                .as_ref()
                .map(bool_like)
                .unwrap_or_else(|| bool_like(get(input, "customize_multi_shots")));
            insert(input, "customize_multi_shots", custom);
            input.remove("prefer_multi_shots");
            if !custom {
                input.remove("multi_prompt");
            }
        }
        "transformation" => remove(
            input,
            &[
                "customize_multi_shots",
                "prefer_multi_shots",
                "multi_prompt",
                "elements",
            ],
        ),
        _ => {}
    }
    if let Some(value) = input.get("multi_prompt").cloned() {
        normalized_list(
            input,
            "multi_prompt",
            multi_prompt(&value, 15).into_iter().take(6).collect(),
        );
    }
    let count = strings(get(input, "image_urls")).len();
    let has_video = !strings(get(input, "video_urls")).is_empty();
    match variant {
        "text-to-video" => {
            remove(input, crate::kie::IMAGES);
            remove(input, crate::kie::VIDEOS);
        }
        "image-to-video" => {
            if count > 1 {
                insert(input, "aspect_ratio", "auto");
            }
            remove(input, crate::kie::VIDEOS);
        }
        "reference-to-video" if has_video => {
            insert(input, "aspect_ratio", "auto");
            insert(input, "audio", false);
            if count == 0 && empty(get(input, "elements")) {
                input.remove("duration");
                insert(input, "customize_multi_shots", false);
                input.remove("multi_prompt");
            }
        }
        "transformation" if has_video && count == 0 => {
            insert(input, "aspect_ratio", "auto");
            input.remove("duration");
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn omni_video_reference_forces_valid_audio_and_aspect() {
        let mut input=json!({"prompt":"scene","mode":"std","aspect_ratio":"9:16","duration":10,"image_urls":["image"],"video_urls":["video"],"audio":true,"multi_shot":true,"shot_type":"customize","prefer_multi_shots":true,"multi_prompt":[{"prompt":"shot","duration":"5"}],"element_list":[{"name":"role","element_input_urls":["element"]}]}).as_object().unwrap().clone();
        omni(&mut input, "kling-3.0-omni/reference-to-video");
        assert_eq!(get(&input, "audio"), &json!(false));
        assert_eq!(get(&input, "aspect_ratio"), "auto");
        assert_eq!(
            get(&input, "elements")[0]["element_input_urls"],
            json!(["element"])
        );
        assert!(!input.contains_key("mode"));
    }
}
