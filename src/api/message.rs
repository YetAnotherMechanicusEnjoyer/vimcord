use reqwest::Response;
use serde::Deserialize;

use crate::{
    Error,
    api::{ApiClient, User},
};

#[derive(Debug, Deserialize, Clone)]
pub struct Message {
    //pub id: Snowflake,
    //pub channel_id: Snowflake,
    pub author: User,
    pub content: Option<String>,
    pub timestamp: String,
    /*pub edited_timestamp: Option<Timestamp>,
    pub tts: bool,
    pub mention_everyone: bool,
    pub mentions: Vec<User>,
    pub mention_roles: Vec<Role>,
    pub mention_channels: Vec<ChannelMention>,
    pub attachments: Vec<Attachment>,
    pub embeds: Vec<Embed>,
    pub reactions: Vec<Reaction>,
    pub nonce: Nonce,
    pub pinned: bool,
    pub webhook_id: Option<Snowflake>,
    pub message_type: i32,
    pub activity: Option<MessageActivity>,
    pub application: Option<Application>,
    pub application_id: Snowflake,
    pub flags: Option<i32>,
    pub message_reference: Option<MessageReference>,
    pub message_snapshots: Option<Vec<MessageSnapshot>>,
    pub referenced_message: Option<Box<Message>>,
    pub interaction_metadata: Option<Box<MessageInteractionMetadata>>,
    pub interaction: Option<Box<MessageInteraction>>,
    pub thread: Option<Channel>,
    pub components: Option<Vec<MessageComponent>>,
    pub sticker_items: Option<Vec<MessageStickerItem>>,
    pub stickers: Option<Vec<Sticker>>,
    pub position: i32,
    pub role_subscription_data: Option<RoleSubscriptionData>,
    pub resolved: Option<Resolved>,
    pub poll: Option<Box<Poll>>,
    pub call: Option<MessageCall>,*/
}

impl Message {
    pub async fn send(
        api_client: &ApiClient,
        channel_id: &str,
        content: Option<String>,
        tts: bool,
    ) -> Result<Response, Error> {
        let api_url = format!("{}/channels/{channel_id}/messages?", api_client.base_url);

        let content: &str = &content.unwrap_or("".to_string());

        let payload = serde_json::json!({
            "content": content,
            "tts": tts,
        });

        let response = api_client
            .http_client
            .post(&api_url)
            .header("Authorization", &api_client.auth_token)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await?;

        Ok(response)
    }

    pub async fn from_channel(
        api_client: &ApiClient,
        channel_id: &str,
        around: Option<String>,
        before: Option<String>,
        after: Option<String>,
        limit: Option<usize>,
    ) -> Result<Vec<Self>, Error> {
        let mut api_url = format!("{}/channels/{channel_id}/messages?", api_client.base_url);

        if let Some(around) = around {
            api_url.push_str(format!("around={}&", around.as_str()).as_str());
        }
        if let Some(before) = before {
            api_url.push_str(format!("before={}&", before.as_str()).as_str());
        }
        if let Some(after) = after {
            api_url.push_str(format!("after={}&", after.as_str()).as_str());
        }
        if let Some(limit) = limit {
            api_url.push_str(format!("limit={limit}").as_str());
        }

        let response = api_client
            .http_client
            .get(&api_url)
            .header("Authorization", &api_client.auth_token)
            .send()
            .await?;

        if !response.status().is_success() {
            eprintln!("API Error: Status code {}", response.status());
            eprintln!("Body response: {}", response.text().await?);
            return Err("Failed to request Discord API".into());
        }

        let messages: Vec<Self> = response.json().await?;

        Ok(messages)
    }
}
