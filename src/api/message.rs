use std::collections::HashMap;

use serde::Deserialize;

use crate::api::{Guild, User};

#[derive(Debug, Deserialize, Clone)]
pub struct Message {
    pub id: String,
    pub channel_id: String,
    pub author: User,
    pub content: Option<String>,
    pub timestamp: String,
    pub mentions: Vec<User>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct PartialMessage {
    pub id: String,
    pub channel_id: String,
    pub author: Option<User>,
    pub content: Option<String>,
    pub timestamp: Option<String>,
}

impl Message {
    pub fn map_mentions(&self, guild: Option<Guild>) -> String {
        let Some(content) = self.content.as_ref() else {
            return "(*non-text*)".to_string();
        };

        let mut ids = std::collections::HashSet::new();
        let tmp = content.clone();
        let mut temp_content = tmp.as_str();

        while let Some(start_idx) = temp_content.find("<@") {
            let after_prefix = &temp_content[start_idx + 2..];
            if let Some(end_idx) = after_prefix.find('>') {
                let id = &after_prefix[..end_idx];
                if id.chars().all(|c| c.is_ascii_digit()) {
                    ids.insert(id.to_string());
                }
                temp_content = &after_prefix[end_idx + 1..];
            } else {
                break;
            }
        }

        let mut mentionned_users = std::collections::HashMap::new();
        for user in self.mentions.clone() {
            mentionned_users.insert(user.id, user.global_name.unwrap_or(user.username));
        }

        let mut map_usernames: HashMap<String, String> = std::collections::HashMap::new();
        for id in ids {
            if let Some(username) = mentionned_users.get(&id) {
                map_usernames.insert(id, username.clone());
            }
        }

        let mut final_content = content.clone();
        for (id, name) in map_usernames {
            let pattern = format!("<@{id}>");
            let replacement = format!("@{name}");
            final_content = final_content.replace(&pattern, &replacement);
        }

        if let Some(guild) = guild {
            let mut role_ids = std::collections::HashSet::new();
            let tmp = content.clone();
            let mut temp_content = tmp.as_str();

            while let Some(start_idx) = temp_content.find("<@&") {
                let after_prefix = &temp_content[start_idx + 3..];
                if let Some(end_idx) = after_prefix.find('>') {
                    let role_id = &after_prefix[..end_idx];
                    if role_id.chars().all(|c| c.is_ascii_digit()) {
                        role_ids.insert(role_id.to_string());
                    }
                    temp_content = &after_prefix[end_idx + 1..];
                } else {
                    break;
                }
            }

            let mut mentionned_roles = std::collections::HashMap::new();
            for role in guild.roles {
                mentionned_roles.insert(role.id, role.name);
            }

            let mut map_roles: HashMap<String, String> = std::collections::HashMap::new();
            for role_id in role_ids {
                if let Some(name) = mentionned_roles.get(&role_id) {
                    map_roles.insert(role_id, name.clone());
                }
            }

            for (id, name) in map_roles {
                let pattern = format!("<@&{id}>");
                let replacement = format!("@{name}");
                final_content = final_content.replace(&pattern, &replacement);
            }
        }

        final_content
    }
}
