//! The weekly server summary: on Monday morning (the server's time zone) one message in the notifications channel
//! with the week's most added works, characters and the event leaders. Sent at most once a week per server.

use super::Feed;
use crate::{
    commands::top::{self, Range, Row, Section},
    db::notify,
    ui::embeds::{fit, Emb},
};
use chrono::{Datelike, Timelike, Weekday};
use chrono_tz::Tz;
use poise::serenity_prelude as serenity;

/// The sections shown, with their heading.
const SECTIONS: &[(Section, &str)] = &[
    (Section::Anime, "🎞 أنمي"),
    (Section::Manga, "📖 مانجا"),
    (Section::Drama, "📺 دراما"),
    (Section::Series, "📺 مسلسلات"),
    (Section::Movies, "🎬 أفلام"),
    (Section::Games, "🎮 ألعاب"),
    (Section::Characters, "👤 شخصيات"),
    (Section::Events, "🏆 نقاط الفعاليات"),
];

/// Is it summary time? Monday, from nine o'clock, in the server's time zone.
pub fn is_summary_time(now: i64, tz: Tz) -> bool {
    chrono::DateTime::from_timestamp(now, 0)
        .map(|d| d.with_timezone(&tz))
        .is_some_and(|d| d.weekday() == Weekday::Mon && d.hour() >= 9)
}

fn section_lines(rows: &[Row]) -> String {
    rows.iter()
        .take(3)
        .enumerate()
        .map(|(i, r)| {
            format!(
                "{} {} — {}",
                ["🥇", "🥈", "🥉"][i],
                fit(&r.label, 60),
                r.count
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The summary embed, or `None` when the week was quiet.
pub fn embed(parts: &[(&str, Vec<Row>)]) -> Option<Emb> {
    let fields: Vec<(&str, String)> = parts
        .iter()
        .filter(|(_, r)| !r.is_empty())
        .map(|(h, r)| (*h, section_lines(r)))
        .collect();
    if fields.is_empty() {
        return None;
    }
    let mut e = Emb::new()
        .title("🗓 ملخص الأسبوع")
        .description("هذا ما أضافه أعضاء السيرفر وأنجزوه خلال الأسبوع الماضي.");
    for (h, v) in fields {
        e = e.field(h, v, false);
    }
    Some(e)
}

pub async fn send(f: &Feed, guild: i64, channel: i64, now: i64) -> anyhow::Result<()> {
    let tz: Tz = crate::db::admin::timezone(&f.pool, guild)
        .await?
        .parse()
        .unwrap_or(chrono_tz::Asia::Muscat);
    if !is_summary_time(now, tz) {
        return Ok(());
    }
    // Claimed per server and week (a negative id marks a server rather than a member): sent once even after a restart.
    let week = crate::db::game::week_key(now);
    if !notify::claim_event(&f.pool, -guild, "server", &format!("summary:{week}")).await? {
        return Ok(());
    }
    let mut parts = Vec::new();
    for (section, heading) in SECTIONS {
        parts.push((
            *heading,
            top::rows_for(&f.pool, guild, *section, Range::Week)
                .await?
                .as_ref()
                .clone(),
        ));
    }
    let Some(e) = embed(&parts) else {
        return Ok(());
    };
    if let Err(err) = serenity::ChannelId::new(channel as u64)
        .send_message(&f.http, serenity::CreateMessage::new().embed(e.build()))
        .await
    {
        tracing::debug!("could not post the weekly summary: {err}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn row(label: &str, n: i64) -> Row {
        Row {
            key: label.into(),
            label: label.into(),
            count: n,
            image: None,
            sub: None,
        }
    }

    #[test]
    fn the_summary_goes_out_on_monday_morning_in_the_servers_zone() {
        let monday_9_muscat = chrono_tz::Asia::Muscat
            .with_ymd_and_hms(2026, 11, 16, 9, 5, 0)
            .single()
            .expect("t")
            .timestamp();
        assert!(is_summary_time(monday_9_muscat, chrono_tz::Asia::Muscat));
        assert!(
            !is_summary_time(monday_9_muscat - 3600, chrono_tz::Asia::Muscat),
            "too early"
        );
        assert!(
            !is_summary_time(monday_9_muscat + 86_400, chrono_tz::Asia::Muscat),
            "Tuesday"
        );
        // The same instant is still Sunday evening in a zone far behind.
        assert!(!is_summary_time(
            monday_9_muscat,
            chrono_tz::America::Los_Angeles
        ));
    }

    #[test]
    fn a_quiet_week_sends_nothing_and_a_busy_one_lists_three_per_section() {
        assert!(embed(&[("🎞 أنمي", vec![])]).is_none());
        let e = embed(&[
            (
                "🎞 أنمي",
                vec![row("A", 5), row("B", 4), row("C", 3), row("D", 2)],
            ),
            ("📖 مانجا", vec![]),
        ])
        .expect("embed");
        assert_eq!(e.fields.len(), 1, "empty sections are left out");
        let v = &e.fields[0].1;
        assert!(v.contains("🥇 A — 5") && v.contains("🥉 C — 3") && !v.contains("D"));
    }
}
