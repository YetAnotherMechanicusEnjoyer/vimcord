use reqwest::Client;

use crate::{Error, api::DISCORD_API_BASE_URL, model::channel::Channel};

pub async fn get_guild_channels(
    client: &Client,
    token: &str,
    guild_id: &str,
) -> Result<Vec<Channel>, Error> {
    let url = format!("{DISCORD_API_BASE_URL}/guilds/{guild_id}/channels");
    let response = client
        .get(&url)
        .header("Authorization", token)
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
