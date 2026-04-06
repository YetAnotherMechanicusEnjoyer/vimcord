use serde::{Deserialize, Serialize};

use crate::api::{User, channel::Role};

#[derive(Debug, Deserialize, Clone)]
pub struct GuildMember {
    pub user: User,
    pub nick: Option<String>,
    pub roles: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PartialGuild {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Guild {
    pub id: String,
    pub name: String,
    pub roles: Vec<Role>,
}
