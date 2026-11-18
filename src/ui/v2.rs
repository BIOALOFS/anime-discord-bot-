//! Discord Components V2 (message flag `IS_COMPONENTS_V2`, 1 << 15). serenity 0.12 cannot build these messages,
//! so they are plain JSON sent through serenity's raw interaction-response endpoint. A V2 message carries no
//! `content` and no `embeds`: everything lives in one container (media galleries, text, action rows), which is how a
//! profile or a suggestion becomes a single visual block with animated GIFs.
//!
//! Receiving works with serenity as is: interaction payloads whose message holds V2 components deserialize safely
//! (unknown top-level component types are skipped).

use poise::serenity_prelude::{CreateActionRow, CreateAttachment};
use serde_json::{json, Value};

pub const IS_COMPONENTS_V2: u64 = 1 << 15;
/// Discord's limits for a V2 message.
pub const MAX_COMPONENTS: usize = 40;
pub const MAX_TEXT_CHARS: usize = 4000;
pub const MAX_GALLERY_ITEMS: usize = 10;

/// A text display.
pub fn text(content: &str) -> Value {
    json!({"type": 10, "content": content})
}

/// A media gallery; `urls` may be `attachment://file` references or hosted https URLs. At most 10 items.
pub fn gallery(urls: &[String]) -> Value {
    let items: Vec<Value> = urls
        .iter()
        .take(MAX_GALLERY_ITEMS)
        .map(|u| json!({"media": {"url": u}}))
        .collect();
    json!({"type": 12, "items": items})
}

/// The container: children in order, with the side-bar colour (`0xRRGGBB`) when given.
pub fn container(children: Vec<Value>, accent: Option<u32>) -> Value {
    let mut c = json!({"type": 17, "components": children});
    if let Some(a) = accent {
        c["accent_color"] = json!(a & 0xFF_FFFF);
    }
    c
}

/// serenity's own serialisation of buttons and select menus, as JSON values (action rows are valid V2 children).
pub fn rows(rows: &[CreateActionRow]) -> Vec<Value> {
    rows.iter()
        .filter_map(|r| serde_json::to_value(r).ok())
        .collect()
}

/// Every component of the tree, counting nested ones (Discord's limit counts them all).
pub fn count_components(v: &Value) -> usize {
    match v {
        Value::Array(a) => a.iter().map(count_components).sum(),
        Value::Object(o) if o.contains_key("type") => {
            1 + o.get("components").map_or(0, count_components)
                + o.get("items")
                    .map_or(0, |i| i.as_array().map_or(0, Vec::len))
        }
        _ => 0,
    }
}

fn text_chars(v: &Value) -> usize {
    match v {
        Value::Array(a) => a.iter().map(text_chars).sum(),
        Value::Object(o) => {
            let own = if o.get("type") == Some(&json!(10)) {
                o.get("content")
                    .and_then(Value::as_str)
                    .map_or(0, |s| s.chars().count())
            } else {
                0
            };
            own + o.get("components").map_or(0, text_chars)
        }
        _ => 0,
    }
}

/// Check a component tree against Discord's limits before sending it.
pub fn validate(components: &[Value]) -> Result<(), String> {
    let n: usize = components
        .iter()
        .map(|c| count_components(&Value::Array(vec![c.clone()])))
        .sum();
    if n > MAX_COMPONENTS {
        return Err(format!("{n} components (max {MAX_COMPONENTS})"));
    }
    let t: usize = components.iter().map(text_chars).sum();
    if t > MAX_TEXT_CHARS {
        return Err(format!("{t} text characters (max {MAX_TEXT_CHARS})"));
    }
    Ok(())
}

/// The request body of a V2 message: no `content`, no `embeds`, attachments listed by index so that
/// `attachment://name` URLs resolve.
pub fn payload(components: Vec<Value>, files: &[CreateAttachment]) -> Value {
    let attachments: Vec<Value> = files
        .iter()
        .enumerate()
        .map(|(i, f)| json!({"id": i, "filename": f.filename}))
        .collect();
    json!({"flags": IS_COMPONENTS_V2, "components": components, "attachments": attachments})
}

/// What was already uploaded to a message: file name to `(attachment id, digest of its bytes)`.
pub type Known = std::collections::HashMap<String, (u64, u64)>;

/// A cheap digest of a file's bytes (to tell an unchanged picture from a changed one with the same name).
pub fn digest(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

/// Like `payload`, but a file whose name and bytes are already attached to the message is referenced by its attachment
/// id instead of being uploaded again (a banner of several megabytes is not re-sent on every click). Returns the body
/// and the files that really have to be uploaded.
pub fn payload_reusing(
    components: Vec<Value>,
    files: &[CreateAttachment],
    known: &Known,
) -> (Value, Vec<CreateAttachment>) {
    let mut attachments = Vec::new();
    let mut upload = Vec::new();
    for f in files {
        match known.get(&f.filename) {
            Some((id, d)) if *d == digest(&f.data) => {
                attachments.push(json!({"id": id.to_string()}))
            }
            _ => {
                attachments.push(json!({"id": upload.len(), "filename": f.filename}));
                upload.push(f.clone());
            }
        }
    }
    (
        json!({"flags": IS_COMPONENTS_V2, "components": components, "attachments": attachments}),
        upload,
    )
}

/// The attachments of a message Discord just returned, as `Known` (matched to the files of the page by name).
pub fn known_from(sent: &[(String, u64)], files: &[CreateAttachment]) -> Known {
    files
        .iter()
        .filter_map(|f| {
            let (_, id) = sent.iter().find(|(n, _)| *n == f.filename)?;
            Some((f.filename.clone(), (*id, digest(&f.data))))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use poise::serenity_prelude as serenity;

    #[test]
    fn an_unchanged_file_is_referenced_by_id_and_a_changed_or_new_one_is_uploaded() {
        let banner = CreateAttachment::bytes(vec![1; 100], "banner.gif");
        let pic = CreateAttachment::bytes(vec![2; 10], "custom-5.png");
        let known = known_from(
            &[("banner.gif".into(), 777), ("custom-5.png".into(), 888)],
            &[banner.clone(), pic.clone()],
        );
        // same banner, a different picture: only the picture is uploaded
        let pic2 = CreateAttachment::bytes(vec![3; 10], "custom-6.png");
        let (body, upload) = payload_reusing(vec![], &[banner.clone(), pic2.clone()], &known);
        assert_eq!(upload.len(), 1);
        assert_eq!(upload[0].filename, "custom-6.png");
        assert_eq!(body["attachments"][0], json!({"id": "777"}));
        assert_eq!(
            body["attachments"][1],
            json!({"id": 0, "filename": "custom-6.png"})
        );
        // the same name with other bytes is a new file
        let changed = CreateAttachment::bytes(vec![9; 100], "banner.gif");
        let (body, upload) = payload_reusing(vec![], &[changed], &known);
        assert_eq!(upload.len(), 1);
        assert_eq!(body["attachments"][0]["filename"], "banner.gif");
        // nothing known: everything is uploaded, numbered from 0
        let (_, upload) = payload_reusing(vec![], &[banner, pic], &Known::new());
        assert_eq!(upload.len(), 2);
    }

    fn sample() -> Vec<Value> {
        let buttons = CreateActionRow::Buttons(vec![
            serenity::CreateButton::new("a").label("A"),
            serenity::CreateButton::new("b").label("B"),
        ]);
        let mut kids = vec![
            gallery(&["attachment://banner.gif".into()]),
            text("hello"),
            gallery(&["attachment://item.png".into()]),
        ];
        kids.extend(rows(&[buttons]));
        vec![container(kids, Some(0x3DB4F2))]
    }

    #[test]
    fn the_container_holds_gallery_text_gallery_and_rows_in_order() {
        let c = &sample()[0];
        assert_eq!(c["type"], 17);
        assert_eq!(c["accent_color"], 0x3DB4F2);
        let kinds: Vec<i64> = c["components"]
            .as_array()
            .expect("children")
            .iter()
            .map(|k| k["type"].as_i64().unwrap_or(0))
            .collect();
        assert_eq!(
            kinds,
            vec![12, 10, 12, 1],
            "banner gallery, text, item gallery, action row"
        );
        assert_eq!(
            c["components"][0]["items"][0]["media"]["url"],
            "attachment://banner.gif"
        );
    }

    #[test]
    fn the_payload_sets_the_v2_flag_and_never_carries_content_or_embeds() {
        let files = vec![
            CreateAttachment::bytes(vec![1, 2, 3], "banner.gif"),
            CreateAttachment::bytes(vec![4], "item.png"),
        ];
        let p = payload(sample(), &files);
        assert_eq!(p["flags"], 32768);
        assert!(p.get("content").is_none() && p.get("embeds").is_none());
        assert_eq!(
            p["attachments"],
            json!([{"id": 0, "filename": "banner.gif"}, {"id": 1, "filename": "item.png"}])
        );
    }

    #[test]
    fn accent_colours_are_masked_to_24_bits_and_optional() {
        assert_eq!(
            container(vec![], Some(0x1_FF00FF))["accent_color"],
            0xFF00FF
        );
        assert!(container(vec![], None).get("accent_color").is_none());
    }

    #[test]
    fn galleries_are_capped_at_ten_items() {
        let urls: Vec<String> = (0..15).map(|i| format!("https://x/{i}.png")).collect();
        assert_eq!(gallery(&urls)["items"].as_array().map(Vec::len), Some(10));
    }

    #[test]
    fn limits_count_nested_components_and_text() {
        assert!(validate(&sample()).is_ok());
        // container + 3 non-row children + row + 2 buttons (+ gallery items)
        assert!(count_components(&Value::Array(sample())) >= 8);
        let many: Vec<Value> = (0..45).map(|_| text("x")).collect();
        assert!(
            validate(&[container(many, None)]).is_err(),
            "over 40 components"
        );
        assert!(
            validate(&[container(vec![text(&"x".repeat(4001))], None)]).is_err(),
            "over 4000 characters"
        );
    }
}
