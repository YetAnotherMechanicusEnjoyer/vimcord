use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::Error;

const VIEW_CHANNEL_PERMISSION: u64 = 1 << 10;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Role {
    pub id: String,
    pub name: String,
    pub permissions: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct PermissionContext {
    pub user_id: String,
    pub user_role_ids: Vec<String>,
    pub all_guild_roles: Vec<Role>,
    pub everyone_role_id: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Overwrite {
    pub id: String,
    pub r#type: u8,
    pub allow: String,
    pub deny: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Channel {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub channel_type: u8,
    pub guild_id: Option<String>,
    pub parent_id: Option<String>,
    #[serde(default)]
    pub permission_overwrites: Vec<Overwrite>,
    pub children: Option<Vec<Channel>>,
}

fn parse_permission_string(hex_string: &str) -> u64 {
    hex_string
        .parse::<u64>()
        .unwrap_or_else(|_| u64::from_str_radix(hex_string, 16).unwrap_or(0))
}

impl Channel {
    fn calculate_permissions(&self, context: &PermissionContext) -> u64 {
        let everyone_role = context
            .all_guild_roles
            .iter()
            .find(|r| r.id == context.everyone_role_id)
            .cloned()
            .unwrap_or_else(|| Role {
                id: context.everyone_role_id.clone(),
                name: "@everyone".to_string(),
                permissions: "0".to_string(),
            });

        let mut permissions = parse_permission_string(&everyone_role.permissions);
        for user_role_id in context
            .user_role_ids
            .iter()
            .filter(|&id| id != &context.everyone_role_id)
        {
            if let Some(role) = context
                .all_guild_roles
                .iter()
                .find(|r| &r.id == user_role_id)
            {
                permissions |= parse_permission_string(&role.permissions);
            }
        }

        if let Some(everyone_overwrite) = self
            .permission_overwrites
            .iter()
            .find(|o| o.r#type == 0 && o.id == context.everyone_role_id)
        {
            let deny = parse_permission_string(&everyone_overwrite.deny);
            let allow = parse_permission_string(&everyone_overwrite.allow);

            permissions &= !deny;
            permissions |= allow;
        }

        let mut role_denies = 0u64;
        let mut role_allows = 0u64;

        for user_role_id in context
            .user_role_ids
            .iter()
            .filter(|&id| id != &context.everyone_role_id)
        {
            if let Some(overwrite) = self
                .permission_overwrites
                .iter()
                .find(|o| o.r#type == 0 && &o.id == user_role_id)
            {
                role_denies |= parse_permission_string(&overwrite.deny);
                role_allows |= parse_permission_string(&overwrite.allow);
            }
        }

        permissions &= !role_denies;
        permissions |= role_allows;

        let user_id = context
            .user_role_ids
            .first()
            .unwrap_or(&String::new())
            .clone();

        if let Some(member_overwrite) = self
            .permission_overwrites
            .iter()
            .find(|o| o.r#type == 1 && o.id == user_id)
        {
            let deny = parse_permission_string(&member_overwrite.deny);
            let allow = parse_permission_string(&member_overwrite.allow);

            permissions &= !deny;
            permissions |= allow;
        }

        permissions
    }

    pub fn is_readable(&self, context: &PermissionContext) -> bool {
        let permissions = self.calculate_permissions(context);
        (permissions & VIEW_CHANNEL_PERMISSION) != 0
    }

    pub fn filter_channels_by_categories(channels: Vec<Self>) -> Result<Vec<Self>, Error> {
        if channels.is_empty() {
            return Err("Error: channels must not be empty.".into());
        }

        let (categories, other_channels): (Vec<Self>, Vec<Self>) =
            channels.into_iter().partition(|c| c.channel_type == 4);

        let mut categorized_map: HashMap<String, Vec<Self>> = HashMap::new();
        let mut uncategorized_channels: Vec<Self> = Vec::new();

        for channel in other_channels.into_iter() {
            if let Some(parent_id) = &channel.parent_id {
                categorized_map
                    .entry(parent_id.clone())
                    .or_default()
                    .push(channel);
            } else {
                uncategorized_channels.push(channel);
            }
        }

        let mut final_list: Vec<Self> = Vec::new();

        for mut category in categories {
            if let Some(children) = categorized_map.remove(&category.id) {
                category.children = Some(children);
            }
            final_list.push(category);
        }

        final_list.extend(uncategorized_channels);

        Ok(final_list)
    }
}
