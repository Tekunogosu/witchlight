//! Builds HTTP responses.
//!
//! Nothing here knows what the map is. Each function attaches a content type,
//! one cache instruction and a body. The routes decide what to say.

use std::io::{Cursor, Read as _};

use tiny_http::{Header, Request, Response};

/// The response type every handler returns.
pub type Reply = Response<Cursor<Vec<u8>>>;

/// Cache-Control values, by how long a browser may keep the response.
mod keep {
    /// The page and the feeds. These must never be stale.
    pub const NEVER: &str = "no-store";
    /// Vendored files and tiles. Both are versioned in their URL, so one
    /// address always stands for the same bytes.
    pub const FOREVER: &str = "public, max-age=31536000, immutable";
    /// A tile drawn for one person. As immutable as any tile, but kept only by
    /// the browser it was drawn for, so a shared cache in between never hands
    /// one reader's map to another.
    pub const PRIVATELY: &str = "private, max-age=31536000, immutable";
    /// A marker icon. Rarely changed, but installing a mod changes the set.
    pub const AN_HOUR: &str = "public, max-age=3600";
    /// A player's picture. The path names the player rather than the image, so
    /// the path alone cannot distinguish two versions. The map's own requests
    /// carry the time the picture was drawn. This value covers requests for the
    /// bare path.
    pub const A_MINUTE: &str = "public, max-age=60";
}

/// The largest request body that will be read.
///
/// Positions for a full server are a couple of kilobytes and markers are tens of
/// kilobytes. This limit stops a broken client from being read into memory
/// without bound.
pub const POST_LIMIT: u64 = 8 * 1024 * 1024;

/// Reads a request body, up to [`POST_LIMIT`]. Returns `None` if it cannot be read.
pub fn body(request: &mut Request) -> Option<String> {
    let mut body = String::new();
    request
        .as_reader()
        .take(POST_LIMIT)
        .read_to_string(&mut body)
        .ok()
        .map(|_| body)
}

/// Returns the `Cookie` header, or an empty string if the browser sent none.
#[must_use]
pub fn cookies(request: &Request) -> String {
    header(request, "Cookie").unwrap_or_default()
}

/// Returns one request header by name.
///
/// The name is a `'static` literal because tiny_http's comparison requires one.
/// Every header this program asks for is a literal already.
#[must_use]
pub fn header(request: &Request, name: &'static str) -> Option<String> {
    request
        .headers()
        .iter()
        .find(|header| header.field.equiv(name))
        .map(|header| header.value.as_str().to_owned())
}

pub fn html(body: &str) -> Reply {
    cached(body.as_bytes(), "text/html; charset=utf-8", keep::NEVER)
}

pub fn json(body: &str) -> Reply {
    cached(body.as_bytes(), "application/json", keep::NEVER)
}

/// A vendored or versioned file, which never changes for a given build.
pub fn asset(body: &str, kind: &str) -> Reply {
    cached(body.as_bytes(), kind, keep::FOREVER)
}

pub fn svg(bytes: &[u8]) -> Reply {
    cached(bytes, "image/svg+xml", keep::AN_HOUR)
}

/// A file shipped by a plugin.
///
/// Kept for an hour rather than forever, because these arrive with a plugin
/// rather than with a build and their address carries no build number to change
/// when one of them does. An operator who installs a new plugin version should
/// not have to wait a day for players to see it.
pub fn plugin_asset(bytes: &[u8], kind: &str) -> Reply {
    cached(bytes, kind, keep::AN_HOUR)
}

/// The script a plugin runs on the page.
///
/// Never cached, unlike the files a plugin ships beside it. A plugin is
/// installed by dropping a mod in a folder, so its address carries no version,
/// and its contents change on every reinstall. Cached for an hour, an updated
/// plugin went on running its old script in every browser that had already
/// loaded it, and the only symptom was a fix that appeared not to be installed.
pub fn plugin_script(bytes: &[u8]) -> Reply {
    cached(bytes, "application/javascript", keep::NEVER)
}

pub fn portrait(bytes: &[u8]) -> Reply {
    cached(bytes, "image/png", keep::A_MINUTE)
}

/// A rendered tile. Safe to cache forever, because a changed world changes the
/// `?v=` in the URL.
pub fn tile(bytes: &[u8], mime: &str) -> Reply {
    cached(bytes, mime, keep::FOREVER)
}

/// A tile composed for one reader. See `keep::PRIVATELY`.
pub fn private_tile(bytes: &[u8], mime: &str) -> Reply {
    cached(bytes, mime, keep::PRIVATELY)
}

pub fn text(status: u16, body: &str) -> Reply {
    Response::from_data(body.as_bytes().to_vec()).with_status_code(status)
}

/// Redirects to another address, optionally setting a cookie.
///
/// The status is `303` rather than `302`, which tells the browser to fetch the
/// new address with a GET. A resubmitted login form then behaves predictably.
pub fn redirect(to: &str, cookie: Option<&str>) -> Reply {
    let mut response = Response::from_data(Vec::new()).with_status_code(303);
    put(&mut response, "Location", to.as_bytes());
    if let Some(cookie) = cookie {
        put(&mut response, "Set-Cookie", cookie.as_bytes());
    }
    // A cached redirect is a login that cannot be repeated.
    put(&mut response, "Cache-Control", keep::NEVER.as_bytes());
    response
}

/// Builds a response with a content type and one cache instruction.
///
/// Exactly one `Cache-Control` header. Two are ambiguous rather than stronger,
/// and a browser takes the first, so an `immutable` added after a `no-store`
/// gives an asset that is never cached but looks cached.
fn cached(body: &[u8], content_type: &str, keep: &str) -> Reply {
    let mut response = Response::from_data(body.to_vec());
    put(&mut response, "Content-Type", content_type.as_bytes());
    put(&mut response, "Cache-Control", keep.as_bytes());
    response
}

fn put(response: &mut Reply, field: &str, value: &[u8]) {
    if let Ok(header) = Header::from_bytes(field.as_bytes(), value) {
        response.add_header(header);
    }
}
