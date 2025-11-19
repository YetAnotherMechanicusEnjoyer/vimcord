use serde::{Deserialize, Serialize};

use crate::{
    Error,
    api::{ApiClient, Channel},
};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Guild {
    pub id: String,
    pub name: String,
}

impl Guild {
    pub async fn get_channels(
        api_client: &ApiClient,
        guild_id: &str,
    ) -> Result<Vec<Channel>, Error> {
        let url = format!("{}/guilds/{guild_id}/channels", api_client.base_url);
        let response = api_client
            .http_client
            .get(&url)
            .header("Authorization", &api_client.auth_token)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "Failed to read error body".to_string());

            return Err(format!("API Error: Status {status}. Details: {body}").into());
        }

        Ok(response
            .json::<Vec<Channel>>()
            .await
            .map_err(|e| format!("JSON Decoding Error: {e}."))?)
    }
}
