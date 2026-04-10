use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PartialEmoji {
    pub name: String,
    pub animated: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Activity {
    pub name: Option<String>,
    pub state: Option<String>,
    pub emoji: Option<PartialEmoji>,
    #[serde(rename = "type")]
    pub activity_type: usize,
}
