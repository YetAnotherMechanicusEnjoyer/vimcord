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
        let users = self
            .recipients
            .iter()
            .map(|u| u.username.clone())
            .collect::<Vec<String>>()
            .join(", ");
        if let Some(name) = self.name.clone() {
            format!("{name} ({users})")
        } else {
            users
        }
    }
}
