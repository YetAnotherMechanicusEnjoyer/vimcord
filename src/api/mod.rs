pub mod channel;
pub mod dm;
pub mod emoji;
pub mod guild;
pub mod message;
pub mod user;

use reqwest::{Client, Response};

pub use channel::Channel;
pub use dm::DM;
pub use emoji::Emoji;
pub use guild::Guild;
pub use message::Message;
pub use user::User;

use crate::Error;

#[derive(Debug, Clone)]
pub struct ApiClient {
    pub http_client: Client,
    pub auth_token: String,
    pub base_url: String,
}

impl ApiClient {
    pub fn new(http_client: Client, auth_token: String, base_url: String) -> Self {
        Self {
            http_client,
            auth_token,
            base_url,
        }
    }

    pub async fn get_channel(&self, channel_id: &str) -> Result<Channel, Error> {
        Channel::from_id(self, channel_id).await
    }

    pub async fn get_dms(&self) -> Result<Vec<DM>, Error> {
        DM::from_user(self).await
    }

    pub async fn get_guild_emojis(&self, guild_id: &str) -> Result<Vec<Emoji>, Error> {
        Emoji::from_guild(self, guild_id).await
    }

    pub async fn get_guild_channels(&self, guild_id: &str) -> Result<Vec<Channel>, Error> {
        Guild::get_channels(self, guild_id).await
    }

    pub async fn create_message(
        &self,
        channel_id: &str,
        content: Option<String>,
        tts: bool,
    ) -> Result<Response, Error> {
        Message::send(self, channel_id, content, tts).await
    }

    pub async fn get_channel_messages(
        &self,
        channel_id: &str,
        around: Option<String>,
        before: Option<String>,
        after: Option<String>,
        limit: Option<usize>,
    ) -> Result<Vec<Message>, Error> {
        Message::from_channel(self, channel_id, around, before, after, limit).await
    }

    pub async fn get_current_user_guilds(&self) -> Result<Vec<Guild>, Error> {
        User::get_guilds(self).await
    }
}
