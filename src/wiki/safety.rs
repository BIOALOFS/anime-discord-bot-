//! The content filter every wiki page passes before it is shown or stored for display: explicit words in the title or
//! the text, categories that mark adult content, and the picture's address. No age ratings. A page that is doubtful in
//! any way is dropped silently.

use super::Page;
use crate::anilist::safety::contains_explicit_term;

/// Parts of a (lower-cased) category name that mean adult content.
const BLOCKED_CATEGORY_PARTS: &[&str] = &[
    "nsfw",
    "adult",
    "hentai",
    "erotic",
    "pornograph",
    "sexual",
    "explicit",
    "18+",
    "r-18",
    "r18",
    "lolicon",
    "shotacon",
    "fetish",
    "nudity",
    "uncensored",
    "mature content",
    "sex scene",
];

pub fn category_blocked(category: &str) -> bool {
    let c = format!(" {} ", category.to_lowercase());
    BLOCKED_CATEGORY_PARTS.iter().any(|p| c.contains(p)) || contains_explicit_term(category)
}

/// Is the picture's address one we would let Discord show? HTTPS, a plain image file, no credentials.
pub fn image_url_ok(url: &str) -> bool {
    let Ok(u) = reqwest::Url::parse(url) else {
        return false;
    };
    if u.scheme() != "https" || !u.username().is_empty() || u.password().is_some() {
        return false;
    }
    let Some(host) = u.host_str() else {
        return false;
    };
    if !super::public_host(host) {
        return false;
    }
    let path = u.path().to_lowercase();
    // Fandom puts the file name before `/revision/latest`; the extension is somewhere in the path.
    [".png", ".jpg", ".jpeg", ".webp", ".gif"]
        .iter()
        .any(|e| path.contains(e))
}

/// A page may be shown only when its title, text and categories are clean. Its picture is dropped (not the page) when
/// the address is doubtful.
pub fn clean_page(mut p: Page) -> Option<Page> {
    if contains_explicit_term(&p.title) || p.aliases.iter().any(|a| contains_explicit_term(a)) {
        return None;
    }
    if p.extract.as_deref().is_some_and(contains_explicit_term) {
        return None;
    }
    if p.categories.iter().any(|c| category_blocked(c)) {
        return None;
    }
    if p.parent_title
        .as_deref()
        .is_some_and(contains_explicit_term)
    {
        return None;
    }
    if p.image_url.as_deref().is_some_and(|u| !image_url_ok(u)) {
        p.image_url = None;
    }
    Some(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wiki::Kind;

    fn page() -> Page {
        Page {
            wiki_id: 1,
            page_id: 1,
            title: "Pyramid Head".into(),
            kind: Kind::Character,
            extract: Some("A monster that wields a great knife.".into()),
            image_url: Some("https://static.wikia.nocookie.net/silent/images/a/ab/Head.png/revision/latest?cb=1".into()),
            aliases: vec![],
            parent_title: Some("Silent Hill 2".into()),
            categories: vec!["Characters".into(), "Silent Hill 2 characters".into()],
        }
    }

    #[test]
    fn a_clean_page_passes_unchanged() {
        assert_eq!(clean_page(page()), Some(page()));
    }

    #[test]
    fn explicit_titles_text_aliases_and_categories_drop_the_page() {
        let mut p = page();
        p.title = "Hentai Gallery".into();
        assert!(clean_page(p).is_none());
        let mut p = page();
        p.extract = Some("An explicit porn scene.".into());
        assert!(clean_page(p).is_none());
        let mut p = page();
        p.aliases = vec!["Lewd Head".into()];
        assert!(clean_page(p).is_none());
        for cat in [
            "NSFW",
            "Adult characters",
            "Erotic content",
            "Pornographic pages",
            "R-18 characters",
            "Sexual content",
        ] {
            let mut p = page();
            p.categories.push(cat.into());
            assert!(clean_page(p).is_none(), "{cat}");
        }
        assert!(
            !category_blocked("Silent Hill 2 characters")
                && !category_blocked("Bosses")
                && !category_blocked("Essex characters")
        );
    }

    #[test]
    fn a_doubtful_picture_is_dropped_but_not_the_page() {
        for bad in [
            "http://static.wikia.nocookie.net/a.png",
            "https://127.0.0.1/a.png",
            "https://example.com/readme.txt",
            "https://user:pw@example.com/a.png",
            "javascript:alert(1)",
            "not a url",
        ] {
            let mut p = page();
            p.image_url = Some(bad.into());
            let cleaned = clean_page(p).expect("the page stays");
            assert_eq!(cleaned.image_url, None, "{bad}");
        }
        assert!(image_url_ok(
            "https://static.wikia.nocookie.net/silent/images/a/ab/Head.png/revision/latest"
        ));
        assert!(image_url_ok("https://wiki.gg/images/x.webp"));
    }
}
