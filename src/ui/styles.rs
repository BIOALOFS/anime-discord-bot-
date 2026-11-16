//! Profile badges (computed from activity; nothing is stored).

use crate::i18n;

/// Profile badges, computed from activity (nothing is stored).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Badge {
    pub letter: char,
    pub label: &'static str,
}

pub fn badges(characters: i64, works: i64, watched: i64, guess_correct: i64) -> Vec<Badge> {
    let mut out = Vec::new();
    if characters + works >= 50 {
        out.push(Badge {
            letter: 'C',
            label: i18n::BADGE_COLLECTOR,
        });
    }
    if guess_correct >= 20 {
        out.push(Badge {
            letter: 'G',
            label: i18n::BADGE_GAMER,
        });
    }
    if watched >= 10 {
        out.push(Badge {
            letter: 'W',
            label: i18n::BADGE_WATCHER,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn badges_follow_activity() {
        assert!(badges(0, 0, 0, 0).is_empty());
        let b = badges(40, 15, 10, 20);
        assert_eq!(
            b.iter().map(|b| b.letter).collect::<Vec<_>>(),
            vec!['C', 'G', 'W']
        );
        assert_eq!(badges(10, 0, 9, 19).len(), 0);
    }
}
