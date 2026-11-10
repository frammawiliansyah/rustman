use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Environment {
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub variables: Vec<Variable>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Variable {
    pub key: String,
    pub value: String,
    pub enabled: bool,
    pub secret: bool,
}
