//! Embed builder that can never exceed Discord's limits.

use super::text::truncate_words;
use crate::i18n;
use poise::serenity_prelude as serenity;

pub const BRAND_COLOUR: u32 = 0x3DB4F2;
pub const MAX_TITLE: usize = 256;
pub const MAX_DESCRIPTION: usize = 4096;
pub const MAX_FIELD_NAME: usize = 256;
pub const MAX_FIELD_VALUE: usize = 1024;
pub const MAX_FIELDS: usize = 25;
pub const MAX_FOOTER: usize = 2048;
pub const MAX_AUTHOR: usize = 256;
pub const MAX_TOTAL: usize = 6000;
/// Fields are dropped before the description is cut below this many characters.
const MIN_DESCRIPTION: usize = 500;

pub fn fit(s: &str, max: usize) -> String {
    truncate_words(s, max)
}

/// Parse `#rrggbb` (AniList cover colours) into a colour value.
pub fn parse_hex_colour(s: &str) -> Option<u32> {
    u32::from_str_radix(s.trim().trim_start_matches('#'), 16)
        .ok()
        .filter(|_| s.trim().len() >= 6)
}

#[derive(Debug, Clone, Default)]
pub struct Emb {
    pub title: Option<String>,
    pub url: Option<String>,
    pub description: Option<String>,
    pub colour: Option<u32>,
    pub author: Option<String>,
    pub author_icon: Option<String>,
    pub fields: Vec<(String, String, bool)>,
    pub footer: Option<String>,
    pub image: Option<String>,
    pub thumbnail: Option<String>,
}

impl Emb {
    pub fn new() -> Self {
        Self {
            footer: Some(i18n::POWERED_BY.into()),
            ..Self::default()
        }
    }

    pub fn title(mut self, s: impl Into<String>) -> Self {
        self.title = Some(s.into());
        self
    }
    pub fn url(mut self, s: Option<impl Into<String>>) -> Self {
        self.url = s.map(Into::into);
        self
    }
    pub fn description(mut self, s: impl Into<String>) -> Self {
        self.description = Some(s.into());
        self
    }
    pub fn colour(mut self, c: Option<u32>) -> Self {
        self.colour = c;
        self
    }
    pub fn author(mut self, s: impl Into<String>) -> Self {
        self.author = Some(s.into());
        self
    }
    pub fn field(
        mut self,
        name: impl Into<String>,
        value: impl Into<String>,
        inline: bool,
    ) -> Self {
        self.fields.push((name.into(), value.into(), inline));
        self
    }
    pub fn footer(mut self, s: impl Into<String>) -> Self {
        self.footer = Some(s.into());
        self
    }
    pub fn image(mut self, s: Option<impl Into<String>>) -> Self {
        self.image = s.map(Into::into);
        self
    }
    pub fn thumbnail(mut self, s: Option<impl Into<String>>) -> Self {
        self.thumbnail = s.map(Into::into);
        self
    }

    /// Characters Discord counts towards the 6000 total.
    pub fn total_len(&self) -> usize {
        let n = |s: &Option<String>| s.as_ref().map_or(0, |s| s.chars().count());
        n(&self.title)
            + n(&self.description)
            + n(&self.footer)
            + n(&self.author)
            + self
                .fields
                .iter()
                .map(|(a, b, _)| a.chars().count() + b.chars().count())
                .sum::<usize>()
    }

    /// Truncate every part, drop surplus/empty fields and shrink until the total fits.
    pub fn enforce(&mut self) {
        let cap = |o: &mut Option<String>, max| {
            if let Some(s) = o {
                *s = fit(s, max);
            }
        };
        cap(&mut self.title, MAX_TITLE);
        cap(&mut self.description, MAX_DESCRIPTION);
        cap(&mut self.footer, MAX_FOOTER);
        cap(&mut self.author, MAX_AUTHOR);
        if self.description.as_deref().is_some_and(str::is_empty) {
            self.description = None;
        }
        self.fields.retain(|(n, _, _)| !n.trim().is_empty());
        self.fields.truncate(MAX_FIELDS);
        for (n, v, _) in &mut self.fields {
            *n = fit(n, MAX_FIELD_NAME);
            *v = if v.trim().is_empty() {
                i18n::NONE.to_string()
            } else {
                fit(v, MAX_FIELD_VALUE)
            };
        }
        // Prefer shrinking the description; only drop trailing fields when it would get too short.
        loop {
            let desc_len = self.description.as_ref().map_or(0, |d| d.chars().count());
            let room = MAX_TOTAL.saturating_sub(self.total_len() - desc_len);
            if room >= MIN_DESCRIPTION.min(desc_len) || self.fields.is_empty() {
                if desc_len > room {
                    cap(&mut self.description, room.max(1));
                }
                break;
            }
            self.fields.pop();
        }
    }

    pub fn build(mut self) -> serenity::CreateEmbed {
        self.enforce();
        let mut e = serenity::CreateEmbed::new().colour(self.colour.unwrap_or(BRAND_COLOUR));
        if let Some(t) = self.title {
            e = e.title(t);
        }
        if let Some(u) = self.url {
            e = e.url(u);
        }
        if let Some(d) = self.description {
            e = e.description(d);
        }
        if let Some(a) = self.author {
            let mut author = serenity::CreateEmbedAuthor::new(a);
            if let Some(icon) = self.author_icon {
                author = author.icon_url(icon);
            }
            e = e.author(author);
        }
        for (n, v, inline) in self.fields {
            e = e.field(n, v, inline);
        }
        if let Some(f) = self.footer {
            e = e.footer(serenity::CreateEmbedFooter::new(f));
        }
        if let Some(i) = self.image {
            e = e.image(i);
        }
        if let Some(t) = self.thumbnail {
            e = e.thumbnail(t);
        }
        e
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn long(n: usize) -> String {
        "كلمة ".repeat(n)
    }

    #[test]
    fn every_limit_is_enforced() {
        let mut e = Emb::new()
            .title(long(200))
            .description(long(2000))
            .author(long(200))
            .footer(long(1000));
        for i in 0..40 {
            e = e.field(long(100), long(500), i % 2 == 0);
        }
        e.enforce();
        assert!(e
            .title
            .as_ref()
            .is_some_and(|t| t.chars().count() <= MAX_TITLE));
        assert!(e
            .description
            .as_ref()
            .is_some_and(|d| d.chars().count() <= MAX_DESCRIPTION));
        assert!(e
            .author
            .as_ref()
            .is_some_and(|a| a.chars().count() <= MAX_AUTHOR));
        assert!(e
            .footer
            .as_ref()
            .is_some_and(|f| f.chars().count() <= MAX_FOOTER));
        assert!(e.fields.len() <= MAX_FIELDS);
        for (n, v, _) in &e.fields {
            assert!(n.chars().count() <= MAX_FIELD_NAME);
            assert!(v.chars().count() <= MAX_FIELD_VALUE);
        }
        assert!(e.total_len() <= MAX_TOTAL, "total {}", e.total_len());
    }

    #[test]
    fn description_shrinks_when_fields_alone_fit() {
        let mut e = Emb::new().description("x".repeat(4096));
        for _ in 0..8 {
            e = e.field("name", "v".repeat(500), false);
        }
        e.enforce();
        assert_eq!(e.fields.len(), 8);
        assert!(e.total_len() <= MAX_TOTAL);
    }

    #[test]
    fn exactly_at_limits_is_untouched() {
        let mut e = Emb::new()
            .title("t".repeat(MAX_TITLE))
            .description("d".repeat(MAX_DESCRIPTION));
        e.enforce();
        assert_eq!(e.title.as_deref().map(str::len), Some(MAX_TITLE));
        assert_eq!(
            e.description.as_deref().map(str::len),
            Some(MAX_DESCRIPTION)
        );
    }

    #[test]
    fn empty_values_are_replaced_and_blank_names_dropped() {
        let mut e = Emb::new()
            .description("")
            .field("name", "  ", true)
            .field("", "v", true);
        e.enforce();
        assert!(e.description.is_none());
        assert_eq!(e.fields, vec![("name".to_string(), "—".to_string(), true)]);
    }

    #[test]
    fn hex_colours() {
        assert_eq!(parse_hex_colour("#e4a15d"), Some(0xe4a15d));
        assert_eq!(parse_hex_colour("e4a15d"), Some(0xe4a15d));
        assert_eq!(parse_hex_colour("zzz"), None);
        assert_eq!(parse_hex_colour(""), None);
    }
}
