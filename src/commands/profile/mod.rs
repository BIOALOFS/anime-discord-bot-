//! `/profile ...` and `/top characters`.

mod groups;
mod groups_ac;
mod manage;
mod reorder;
mod social;
mod util;
mod view;

use crate::{
    db::profile::Privacy,
    error::{Context, Error},
    ui::styles::Style,
};
use groups::group;
use manage::{
    add, bulk_add, clear, create, move_to, note, privacy, rank, remove, set_banner, set_bio,
    set_color, style,
};
use reorder::reorder;
pub use social::top;
use social::{compare, export, import};
use view::view;

#[derive(Debug, Clone, Copy, poise::ChoiceParameter)]
pub enum StyleChoice {
    #[name = "صور"]
    #[name_localized("en-US", "Collage")]
    Collage,
    #[name = "أسماء"]
    #[name_localized("en-US", "Names")]
    Names,
    #[name = "بطاقات"]
    #[name_localized("en-US", "Cards")]
    Cards,
    #[name = "معرض"]
    #[name_localized("en-US", "Gallery")]
    Gallery,
    #[name = "منصة التتويج"]
    #[name_localized("en-US", "Podium")]
    Podium,
    #[name = "مختصر"]
    #[name_localized("en-US", "Compact")]
    Compact,
    #[name = "جدول التصنيفات"]
    #[name_localized("en-US", "Tier list")]
    Tier,
    #[name = "عشوائي"]
    #[name_localized("en-US", "Random")]
    Random,
}

impl StyleChoice {
    pub fn style(self) -> Style {
        match self {
            Self::Collage => Style::Collage,
            Self::Names => Style::Names,
            Self::Cards => Style::Cards,
            Self::Gallery => Style::Gallery,
            Self::Podium => Style::Podium,
            Self::Compact => Style::Compact,
            Self::Tier => Style::Tier,
            Self::Random => Style::Random,
        }
    }
}

#[derive(Debug, Clone, Copy, poise::ChoiceParameter)]
pub enum PrivacyChoice {
    #[name = "عام"]
    #[name_localized("en-US", "Public")]
    Public,
    #[name = "السيرفرات المشتركة"]
    #[name_localized("en-US", "Shared servers")]
    Servers,
    #[name = "خاص"]
    #[name_localized("en-US", "Private")]
    Private,
}

impl PrivacyChoice {
    pub fn privacy(self) -> Privacy {
        match self {
            Self::Public => Privacy::Public,
            Self::Servers => Privacy::Servers,
            Self::Private => Privacy::Private,
        }
    }
}

/// Your global character profile.
#[poise::command(
    slash_command,
    subcommands(
        "create",
        "add",
        "bulk_add",
        "remove",
        "clear",
        "reorder",
        "move_to",
        "rank",
        "group",
        "note",
        "set_bio",
        "set_color",
        "set_banner",
        "view",
        "style",
        "privacy",
        "export",
        "import",
        "compare"
    ),
    name_localized("ar", "بروفايل"),
    description_localized("ar", "بروفايلك العام للشخصيات المفضلة")
)]
pub async fn profile(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}
