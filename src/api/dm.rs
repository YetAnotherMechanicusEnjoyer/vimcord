use serde::Deserialize;

use crate::api::User;

#[derive(Debug, Deserialize, Clone)]
pub struct DM {
    pub id: String,
    #[serde(rename = "type")]
    pub channel_type: u8,
    pub last_message_id: Option<String>,
    pub recipients: Vec<User>,
    pub name: Option<String>,
}

impl DM {
    pub fn get_name(&self) -> String {
        if self.channel_type == 1
            && let Some(user) = self.recipients.first()
        {
            let global_name = user.global_name.clone();
            if let Some(name) = global_name {
                format!("{name} ({})", user.username)
            } else {
                user.username.clone()
            }
        } else {
            let users = self
                .recipients
                .iter()
                .map(|u| u.global_name.clone().unwrap_or(u.username.clone()))
                .collect::<Vec<String>>()
                .join(", ");
            if let Some(name) = self.name.clone() {
                format!("{name} ({users})")
            } else {
                users
            }
        }
    }
}
