//! Shared JSON:API envelope types used across every App Metadata resource.
//! Resource-specific attribute structs live in their own modules
//! (`app`, `app_store_version`, ...) and plug into these generics.

use serde::{Deserialize, Serialize};

/// A single-resource JSON:API document — the body of e.g. `GET
/// /v1/apps/{id}` or the response to a `POST`/`PATCH`.
#[derive(Debug, Clone, Deserialize)]
pub struct Document<A> {
    pub data: Resource<A>,
}

/// A resource-collection JSON:API document — the body of e.g. `GET
/// /v1/apps`.
#[derive(Debug, Clone, Deserialize)]
pub struct ListDocument<A> {
    pub data: Vec<Resource<A>>,
}

/// A single JSON:API resource object: its id, type, and typed attributes.
/// Relationships are intentionally left untyped (`serde_json::Value`) —
/// this crate builds relationship payloads explicitly on requests rather
/// than typing every relationship shape App Store Connect can return.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resource<A> {
    pub id: String,
    #[serde(rename = "type")]
    pub resource_type: String,
    pub attributes: A,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relationships: Option<serde_json::Value>,
}

/// A bare `{ type, id }` resource identifier, used to point a create
/// request's relationships at an existing resource.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceId {
    #[serde(rename = "type")]
    pub resource_type: &'static str,
    pub id: String,
}

/// A to-one relationship payload, e.g. `relationships.app` on a
/// `POST /v1/appStoreVersions` body.
#[derive(Debug, Clone, Serialize)]
pub struct ToOne {
    pub data: ResourceId,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateBody<A, R> {
    pub data: CreateData<A, R>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateData<A, R> {
    #[serde(rename = "type")]
    pub resource_type: &'static str,
    pub attributes: A,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relationships: Option<R>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateBody<A> {
    pub data: UpdateData<A>,
}

/// A `PATCH` body that updates only relationships (no attributes) — e.g.
/// attaching a `Build` to an `AppStoreVersion`.
#[derive(Debug, Clone, Serialize)]
pub struct UpdateRelationshipsBody<R> {
    pub data: UpdateRelationshipsData<R>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateRelationshipsData<R> {
    #[serde(rename = "type")]
    pub resource_type: &'static str,
    pub id: String,
    pub relationships: R,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateData<A> {
    #[serde(rename = "type")]
    pub resource_type: &'static str,
    pub id: String,
    pub attributes: A,
}

/// The `errors` envelope App Store Connect returns on non-2xx responses.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ErrorDocument {
    #[serde(default)]
    pub errors: Vec<ApiError>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApiError {
    pub status: String,
    pub code: String,
    pub title: String,
    #[serde(default)]
    pub detail: String,
}
