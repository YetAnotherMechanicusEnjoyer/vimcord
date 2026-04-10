use serde::{Deserialize, Serialize};

use crate::api::Activity;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ClientStatus {
    pub desktop: Option<String>,
    pub mobile: Option<String>,
    pub web: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserId {
    pub id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Presence {
    pub user: UserId,
    pub status: String,
    pub activities: Vec<Activity>,
    pub client_status: ClientStatus,
}
