//! Answering a request.
//!
//! Nothing here knows what the map is. It knows what a browser is owed: a
//! content type, one cache instruction, and a body — which is why it sits apart
//! from the routes that decide what to say.

use std::io::{Cursor, Read as _};

use tiny_http::{Header, Request, Response};

/// What every handler answers with.
pub type Reply = Response<Cursor<Vec<u8>>>;

/// How long a browser may keep something.
mod keep {
    /// The page and the feeds, which must never be stale.
    pub const NEVER: &str = "no-store";
    /// Anything vendored, and any tile: both are versioned in their URL, so a
    /// given address can never stand for different bytes.
    pub const FOREVER: &str = "public, max-age=31536000, immutable";
    /// A marker icon. Rarely changed, but adding a mod changes the set.
    pub const AN_HOUR: &str = "public, max-age=3600";
    /// A player's picture. Its name comes from who they are rather than from
    /// what the picture holds, so this path alone cannot tell two apart; what
    /// the map asks for carries the time it was drawn, and this stands behind
    /// anyone who asks for the bare path instead.
    pub const A_MINUTE: &str = "public, max-age=60";
}

/// The most a body may carry.
///
/// Positions for a full server are a couple of kilobytes and markers are tens;
/// this only stops a broken poster from being read into memory without limit.
pub const POST_LIMIT: u64 = 8 * 1024 * 1024;

/// What a request carries, up to [`POST_LIMIT`]. `None` where it cannot be read.
pub fn body(request: &mut Request) -> Option<String> {
    let mut body = String::new();
    request
        .as_reader()
        .take(POST_LIMIT)
        .read_to_string(&mut body)
        .ok()
        .map(|_| body)
}

/// The `Cookie` header, or nothing where the browser sent none.
#[must_use]
pub fn cookies(request: &Request) -> String {
    header(request, "Cookie").unwrap_or_default()
}

/// One request header, by name.
///
/// The name is a literal because tiny_http's own comparison wants one, which is
/// no loss: every header this asks for is written down in this program.
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

/// Something vendored or versioned, which never changes for a given build.
pub fn asset(body: &str, kind: &str) -> Reply {
    cached(body.as_bytes(), kind, keep::FOREVER)
}

pub fn svg(bytes: &[u8]) -> Reply {
    cached(bytes, "image/svg+xml", keep::AN_HOUR)
}

pub fn portrait(bytes: &[u8]) -> Reply {
    cached(bytes, "image/png", keep::A_MINUTE)
}

/// A rendered tile. Safe to keep forever: a changed world is a changed `?v=`.
pub fn tile(bytes: &[u8]) -> Reply {
    cached(bytes, "image/png", keep::FOREVER)
}

pub fn text(status: u16, body: &str) -> Reply {
    Response::from_data(body.as_bytes().to_vec()).with_status_code(status)
}

/// Somewhere else, optionally leaving a cookie behind.
///
/// `303` rather than `302`, so the browser is told in as many words to fetch the
/// new address with a GET. It is the difference between a login that works on a
/// resubmitted form and one that does something surprising.
pub fn redirect(to: &str, cookie: Option<&str>) -> Reply {
    let mut response = Response::from_data(Vec::new()).with_status_code(303);
    put(&mut response, "Location", to.as_bytes());
    if let Some(cookie) = cookie {
        put(&mut response, "Set-Cookie", cookie.as_bytes());
    }
    // A redirect a browser remembers is a login that cannot be repeated.
    put(&mut response, "Cache-Control", keep::NEVER.as_bytes());
    response
}

/// A response that says what it is and how long it may be kept.
///
/// One `Cache-Control` and one only: two of them is not a stronger instruction,
/// it is an ambiguous one, and a browser takes the first — so an `immutable`
/// added after a `no-store` is an asset that is never cached and looks cached.
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
