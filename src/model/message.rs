use serde::Deserialize;

use crate::model::User;

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
