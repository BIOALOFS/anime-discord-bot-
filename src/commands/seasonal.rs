//! `/seasonal [season] [year] [sort] [format]`.

use super::{
    fail,
    media::{pages_of, MediaView, Mode, Source},
    policy, say_ephemeral,
};
use crate::{
    anilist::SeasonalQuery,
    error::{Context, Error},
    i18n,
    ui::{format::season_from_date, session},
};
use chrono::{DateTime, Datelike, Utc};

#[derive(Debug, Clone, Copy, poise::ChoiceParameter)]
pub enum SeasonChoice {
    #[name = "شتاء"]
    #[name_localized("en-US", "Winter")]
    Winter,
    #[name = "ربيع"]
    #[name_localized("en-US", "Spring")]
    Spring,
    #[name = "صيف"]
    #[name_localized("en-US", "Summer")]
    Summer,
    #[name = "خريف"]
    #[name_localized("en-US", "Fall")]
    Fall,
}

impl SeasonChoice {
    fn api(self) -> &'static str {
        match self {
            Self::Winter => "WINTER",
            Self::Spring => "SPRING",
            Self::Summer => "SUMMER",
            Self::Fall => "FALL",
        }
    }
}

#[derive(Debug, Clone, Copy, poise::ChoiceParameter)]
pub enum SortChoice {
    #[name = "الأشهر"]
    #[name_localized("en-US", "Most popular")]
    Popular,
    #[name = "الأعلى تقييمًا"]
    #[name_localized("en-US", "Top rated")]
    Score,
    #[name = "الأحدث"]
    #[name_localized("en-US", "Newest")]
    Newest,
}

impl SortChoice {
    fn api(self) -> &'static str {
        match self {
            Self::Popular => "POPULARITY_DESC",
            Self::Score => "SCORE_DESC",
            Self::Newest => "START_DATE_DESC",
        }
    }
}

#[derive(Debug, Clone, Copy, poise::ChoiceParameter)]
pub enum FormatChoice {
    #[name = "الكل"]
    #[name_localized("en-US", "All")]
    All,
    #[name = "مسلسل"]
    #[name_localized("en-US", "TV")]
    Tv,
    #[name = "فيلم"]
    #[name_localized("en-US", "Movie")]
    Movie,
    #[name = "ONA"]
    Ona,
}

impl FormatChoice {
    fn api(self) -> &'static [&'static str] {
        match self {
            Self::All => &[],
            Self::Tv => &["TV", "TV_SHORT"],
            Self::Movie => &["MOVIE"],
            Self::Ona => &["ONA"],
        }
    }
}

/// Defaults: the current season when nothing is given, the current year when only a season is.
pub fn build_query(
    season: Option<SeasonChoice>,
    year: Option<i32>,
    sort: Option<SortChoice>,
    format: Option<FormatChoice>,
    now: DateTime<Utc>,
) -> SeasonalQuery {
    let (cur_season, cur_year) = season_from_date(now.year(), now.month());
    let (season, default_year) = match season {
        Some(s) => (s.api(), now.year()),
        None => (cur_season, cur_year),
    };
    SeasonalQuery {
        season,
        year: year.unwrap_or(default_year),
        sort: sort.unwrap_or(SortChoice::Popular).api(),
        formats: format.unwrap_or(FormatChoice::All).api(),
    }
}

/// Seasonal anime list.
#[poise::command(
    slash_command,
    name_localized("ar", "موسمي"),
    description_localized("ar", "أنمي الموسم")
)]
pub async fn seasonal(
    ctx: Context<'_>,
    #[description = "Season (default: current)"]
    #[description_localized("ar", "الموسم (الافتراضي: الحالي)")]
    #[name_localized("ar", "الموسم")]
    season: Option<SeasonChoice>,
    #[description = "Year (default: current)"]
    #[description_localized("ar", "السنة (الافتراضي: الحالية)")]
    #[name_localized("ar", "السنة")]
    #[min = 1940]
    #[max = 2100]
    year: Option<i32>,
    #[description = "Sort order"]
    #[description_localized("ar", "الترتيب")]
    #[name_localized("ar", "الترتيب")]
    sort: Option<SortChoice>,
    #[description = "Format filter"]
    #[description_localized("ar", "النوع")]
    #[name_localized("ar", "النوع")]
    format: Option<FormatChoice>,
) -> Result<(), Error> {
    ctx.defer().await?;
    let api = ctx.data().anilist.clone();
    let policy = policy(ctx);
    let q = build_query(season, year, sort, format, Utc::now());
    match api.seasonal(&q, 1, policy).await {
        Ok(p) if p.items.is_empty() => say_ephemeral(ctx, i18n::NO_RESULTS).await,
        Ok(p) => {
            let pages = pages_of(p.last_page, p.has_next, 0);
            let mode = Mode::Seasonal {
                page: 0,
                pages,
                items: p.items,
            };
            session::run(ctx, MediaView::new(api, policy, Source::Seasonal(q), mode)).await
        }
        Err(e) => fail(ctx, e).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(y: i32, m: u32, d: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, 12, 0, 0)
            .single()
            .expect("date")
    }

    #[test]
    fn defaults_to_the_current_season() {
        let q = build_query(None, None, None, None, at(2026, 11, 14));
        assert_eq!(
            (q.season, q.year, q.sort),
            ("FALL", 2026, "POPULARITY_DESC")
        );
        assert!(q.formats.is_empty());
        let q = build_query(None, None, None, None, at(2026, 12, 20));
        assert_eq!(
            (q.season, q.year),
            ("WINTER", 2027),
            "December rolls into next year's winter"
        );
        let q = build_query(None, None, None, None, at(2026, 1, 5));
        assert_eq!((q.season, q.year), ("WINTER", 2026));
    }

    #[test]
    fn explicit_arguments_win() {
        let q = build_query(
            Some(SeasonChoice::Spring),
            None,
            Some(SortChoice::Score),
            Some(FormatChoice::Movie),
            at(2026, 12, 20),
        );
        assert_eq!((q.season, q.year, q.sort), ("SPRING", 2026, "SCORE_DESC"));
        assert_eq!(q.formats, &["MOVIE"]);
        let q = build_query(
            Some(SeasonChoice::Summer),
            Some(2019),
            None,
            Some(FormatChoice::Tv),
            at(2026, 1, 1),
        );
        assert_eq!((q.season, q.year), ("SUMMER", 2019));
        assert_eq!(q.formats, &["TV", "TV_SHORT"]);
    }
}
