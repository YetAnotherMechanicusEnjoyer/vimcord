use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct User {
    //pub id: Snowflake,
    pub username: String,
    //pub discriminator: String,
    //pub global_name: Option<String>,
    //pub avatar : Option<String>,
    //pub bot: Option<bool>,
}
