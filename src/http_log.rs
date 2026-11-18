//! One place that logs a failed HTTP answer from a provider: who, which endpoint, the status and a short, scrubbed
//! excerpt of the body. Keys, tokens and secrets never reach the log.

/// Longest body excerpt that is logged.
pub const MAX_EXCERPT: usize = 300;

/// Parameters whose value is a credential (`name=value` in a URL or form).
const SECRET_NAMES: &[&str] = &[
    "api_key",
    "apikey",
    "client_secret",
    "client_id",
    "access_token",
    "token",
    "authorization",
    "secret",
    "key",
];

fn token_like(word: &str) -> bool {
    word.len() >= 24
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '='))
}

/// `"name": "value"` pairs of a JSON body whose name is a credential: the value is replaced.
fn scrub_json_pairs(text: &str) -> String {
    let mut out = text.to_string();
    for name in SECRET_NAMES {
        let needle = format!("\"{name}\"");
        let mut from = 0;
        while let Some(rel) = out.to_lowercase()[from..].find(&needle) {
            let after = from + rel + needle.len();
            let rest = &out[after..];
            let trimmed = rest.trim_start();
            if let Some(v) = trimmed
                .strip_prefix(':')
                .map(str::trim_start)
                .and_then(|v| v.strip_prefix('"'))
            {
                let start = out.len() - v.len();
                if let Some(end) = v.find('"') {
                    out.replace_range(start..start + end, "<redacted>");
                }
            }
            from = after;
        }
    }
    out
}

/// A short, single-line excerpt of a response body with anything credential-like removed.
pub fn excerpt(body: &str) -> String {
    let flat: String = scrub_json_pairs(body)
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let mut out = Vec::new();
    for word in flat.split_whitespace() {
        // `name=value` / `"name":"value"` forms.
        let lower = word.to_lowercase();
        let named = SECRET_NAMES.iter().any(|n| {
            lower.starts_with(&format!("{n}=")) || lower.starts_with(&format!("\"{n}\":"))
        });
        if named {
            let cut = word.find(['=', ':']).map_or(word.len(), |i| i + 1);
            out.push(format!("{}<redacted>", &word[..cut]));
            continue;
        }
        // Bearer-style and bare tokens.
        let core = word.trim_matches(|c: char| !c.is_ascii_alphanumeric());
        if token_like(core) || core.len() > 40 {
            out.push(word.replace(core, "<redacted>"));
        } else {
            out.push(word.to_string());
        }
    }
    let joined = out.join(" ");
    let mut short: String = joined.chars().take(MAX_EXCERPT).collect();
    if joined.chars().count() > MAX_EXCERPT {
        short.push('…');
    }
    short
}

/// Log a non-2xx answer at warn level and return a short description for the error value.
pub async fn failed(provider: &str, endpoint: &str, resp: reqwest::Response) -> String {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    let shown = excerpt(&body);
    tracing::warn!(provider, endpoint, status = status.as_u16(), body = %shown, "provider answered with an error");
    crate::ops::health::err(
        provider,
        &format!("{} {endpoint}: {shown}", status.as_u16()),
    );
    status.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excerpts_are_short_flat_and_scrubbed() {
        let body = format!(
            "{{\"message\":\"bad field\"}}\nclient_secret=abcdef123456 Bearer {} api_key=XYZ {}",
            "a".repeat(40),
            "x ".repeat(400)
        );
        let e = excerpt(&body);
        assert!(e.chars().count() <= MAX_EXCERPT + 1 && !e.contains('\n'));
        assert!(e.contains("bad field"));
        assert!(
            !e.contains("abcdef123456") && !e.contains(&"a".repeat(40)) && !e.contains("XYZ"),
            "{e}"
        );
        assert!(e.contains("client_secret=<redacted>") && e.contains("api_key=<redacted>"));
    }

    #[test]
    fn json_credentials_and_plain_text_are_handled() {
        assert!(!excerpt(r#"{"access_token":"supersecretvalue"}"#).contains("supersecretvalue"));
        assert_eq!(
            excerpt("Unknown field 'releases.minage'"),
            "Unknown field 'releases.minage'"
        );
        assert_eq!(excerpt(""), "");
    }
}
