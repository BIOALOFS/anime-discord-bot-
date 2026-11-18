//! Features added in round 8 that live next to the commands: badges, role permissions, daily posts, polls, watch
//! nights and profile comparison. None of them is a slash command: they extend `/admin`, `/profile`, `/recommend`
//! and the background tick.

pub mod badges;
pub mod daily;
pub mod perms;
pub mod poll;
pub mod scheduler;
pub mod settings;
pub mod watch;
