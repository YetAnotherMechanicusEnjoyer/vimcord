use serde::Deserialize;

use crate::api::User;

#[derive(Debug, Deserialize, Clone)]
pub struct DM {
    pub id: String,
    #[serde(rename = "type")]
    pub channel_type: u8,
    pub recipients: Vec<User>,
}

impl DM {
    pub fn get_name(&self) -> String {
        self.recipients
            .iter()
            .map(|u| u.username.clone())
            .collect::<Vec<String>>()
            .join(", ")
    }
}
