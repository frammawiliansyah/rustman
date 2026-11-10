//! GraphQL client helpers and schema introspection.

pub mod client;
pub mod introspection;

pub use client::{GraphQLRequest, GraphQLResponse};
pub use introspection::{GraphQLIntrospector, INTROSPECTION_QUERY};
