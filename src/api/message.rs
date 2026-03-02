use serde::Deserialize;

use crate::api::User;

#[derive(Debug, Deserialize, Clone)]
pub struct Message {
    pub id: String,
    pub channel_id: String,
    pub author: User,
    pub content: Option<String>,
    pub timestamp: String,
}
