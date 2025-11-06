use reqwest::Client;

use crate::model::guild::Guild;

pub async fn get_current_user_guilds(client: &Client, token: &str) -> Result<Vec<Guild>, String> {
    let url = "https://discord.com/api/v10/users/@me/guilds?limit=100";
    let response = client
        .get(url)
        .header("Authorization", token)
        .send()
        .await
        .map_err(|e| format!("API Error: {e}"))?;

    if response.status().is_success() {
        response
            .json()
            .await
            .map_err(|e| format!("JSON Error: {e}"))
    } else {
        Err(format!("API Error: {}", response.status()))
    }
}
