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
///
/// Collections are paginated. Apple caps `limit` at 200 per page and
/// advertises the following page in `links.next`; a list endpoint that
/// reads only `data` therefore silently truncates to one page, which is
/// indistinguishable from "there were only N of these". Use
/// [`Client::list_all`](crate::Client::list_all) rather than a bare
/// `request` for any collection that can exceed 200 rows.
#[derive(Debug, Clone, Deserialize)]
pub struct ListDocument<A> {
    pub data: Vec<Resource<A>>,
    #[serde(default)]
    pub links: Option<Links>,
}

/// The JSON:API pagination links on a collection response.
///
/// Only `next` is read: paging stops the moment Apple stops offering a
/// following page. `self`/`first`/`prev` are absent rather than ignored
/// because nothing here would use them, and a `prev` pointer invites a
/// caller to loop backwards through a collection that can change
/// underneath it.
#[derive(Debug, Clone, Deserialize)]
pub struct Links {
    /// Absolute URL of the next page, already carrying the caller's
    /// `include`/`filter`/`limit` parameters plus a `cursor`.
    #[serde(default)]
    pub next: Option<String>,
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

/// A to-many relationship payload, e.g. `relationships.certificates` on a
/// `POST /v1/profiles` body.
///
/// Apple distinguishes an empty array from an absent relationship: sending
/// `{"data": []}` for `devices` means "no devices", which a development
/// profile rejects, while omitting the key entirely is what an App Store
/// profile wants. Callers that mean "absent" should pass `None` for the
/// whole field rather than an empty `ToMany`.
#[derive(Debug, Clone, Serialize)]
pub struct ToMany {
    pub data: Vec<ResourceId>,
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

/// Turn an absolute [`Links::next`] URL into the path-and-query form
/// [`Client::request`](crate::Client::request) wants, since that method
/// prefixes its own base URL and cannot be handed a whole URL.
///
/// Returns `None` for anything unparseable, which a paging caller treats
/// as "no more pages" rather than an error — stopping early is recoverable
/// and shows up as a short list, where a hard failure would take down a
/// command that was otherwise working.
pub fn path_and_query(url: &str) -> Option<String> {
    let after_scheme = url.split_once("://")?.1;
    let (_host, rest) = after_scheme.split_once('/')?;
    Some(format!("/{rest}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    struct Attrs {
        name: String,
    }

    /// A real collection page. `links.next` is what tells a paging caller
    /// there is more, so it is pinned against the shape Apple sends.
    #[test]
    fn reads_a_next_link_from_a_collection_page() {
        let doc: ListDocument<Attrs> = serde_json::from_value(serde_json::json!({
            "data": [{ "id": "1", "type": "apps", "attributes": { "name": "Klepon" } }],
            "links": {
                "self": "https://api.appstoreconnect.apple.com/v1/apps?limit=200",
                "next": "https://api.appstoreconnect.apple.com/v1/apps?limit=200&cursor=AQ%3D%3D"
            }
        }))
        .unwrap();

        assert_eq!(doc.data.len(), 1);
        assert_eq!(doc.data[0].attributes.name, "Klepon");
        assert_eq!(
            doc.links.expect("links present").next.as_deref(),
            Some("https://api.appstoreconnect.apple.com/v1/apps?limit=200&cursor=AQ%3D%3D")
        );
    }

    /// Both spellings of "last page" Apple uses: `links` present with only a
    /// `self`, and `links` missing entirely. A single-page collection looks
    /// exactly like this, so neither may be an error.
    #[test]
    fn a_page_without_a_next_link_is_the_last_one() {
        let with_self_only: ListDocument<Attrs> = serde_json::from_value(serde_json::json!({
            "data": [],
            "links": { "self": "https://api.appstoreconnect.apple.com/v1/apps?limit=200" }
        }))
        .unwrap();
        assert!(with_self_only.links.expect("links present").next.is_none());

        let without_links: ListDocument<Attrs> =
            serde_json::from_value(serde_json::json!({ "data": [] })).unwrap();
        assert!(without_links.links.is_none());
    }

    /// `request` prefixes its own base URL, so handing it a whole URL would
    /// produce `https://api.appstoreconnect.apple.comhttps://...`.
    #[test]
    fn strips_the_host_off_a_next_link() {
        assert_eq!(
            path_and_query("https://api.appstoreconnect.apple.com/v1/apps?cursor=AQ").as_deref(),
            Some("/v1/apps?cursor=AQ")
        );
        // Percent-encoded query values must survive untouched — decoding
        // them would let `&` inside a cursor split the parameter.
        assert_eq!(
            path_and_query("https://api.appstoreconnect.apple.com/v1/apps?cursor=AQ%3D%3D")
                .as_deref(),
            Some("/v1/apps?cursor=AQ%3D%3D")
        );
    }

    #[test]
    fn gives_up_on_a_link_that_is_not_a_whole_url() {
        assert_eq!(path_and_query("/v1/apps?cursor=AQ"), None);
        assert_eq!(
            path_and_query("https://api.appstoreconnect.apple.com"),
            None
        );
    }
}
