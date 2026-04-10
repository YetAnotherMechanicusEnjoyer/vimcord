use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct CustomStatus {
    pub text: Option<String>,
    pub emoji_id: Option<String>,
    pub emoji_name: Option<String>,
    pub expires_at: Option<u128>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct UserSettings {
    pub custom_status: Option<CustomStatus>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct User {
    pub id: String,
    pub username: String,
    //pub discriminator: String,
    pub global_name: Option<String>,
    //pub avatar : Option<String>,
    //pub bot: Option<bool>,
}
