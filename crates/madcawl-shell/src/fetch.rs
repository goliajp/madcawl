//! A page fetched over the network in this process, the way a host
//! fetches a navigation itself: one GET through `madcawl-net`, redirects
//! followed, the body decoded, with the charset its `Content-Type`
//! gives.

use madcawl_html::Encoding;
use madcawl_net::{Client, Config, Request, Url};

pub struct Fetched {
    pub status: u16,
    /// Where the redirects ended
    pub url: String,
    pub content_type: String,
    pub charset: Option<Encoding>,
    pub body: Vec<u8>,
}

/// The `charset` parameter of a `Content-Type` value.
pub fn charset_of(content_type: &str) -> Option<Encoding> {
    content_type.split(';').skip(1).find_map(|p| {
        let (k, v) = p.split_once('=')?;
        k.trim()
            .eq_ignore_ascii_case("charset")
            .then(|| Encoding::for_label(v.trim().trim_matches('"').as_bytes()))
            .flatten()
    })
}

/// GETs `url` without cookies from any earlier run; only `http` and
/// `https` are fetched.
pub fn fetch(url: &str) -> Result<Fetched, String> {
    let u = Url::parse(url).map_err(|e| format!("not a URL: {url}: {e:?}"))?;
    if !matches!(u.scheme(), "http" | "https") {
        return Err(format!(
            "only http and https are fetched, not {}",
            u.scheme()
        ));
    }
    let client = Client::new(Config::default());
    let r = client
        .fetch(&Request::get(u))
        .map_err(|e| format!("{url}: {e}"))?;
    let content_type =
        String::from_utf8_lossy(r.header("content-type").unwrap_or_default()).into_owned();
    Ok(Fetched {
        status: r.status,
        url: r.url().as_str().to_owned(),
        charset: charset_of(&content_type),
        content_type,
        body: r.body,
    })
}

/// `madcawl-shell fetch <url> -o <file>`: the body into `file`, and one
/// JSON line on stdout with the status, the final URL, the content type
/// and the body's length.
pub fn command(url: &str, out: &str) -> Result<(), String> {
    let f = fetch(url)?;
    std::fs::write(out, &f.body).map_err(|e| format!("{out}: {e}"))?;
    let q = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    println!(
        "{{\"status\":{},\"url\":\"{}\",\"contentType\":\"{}\",\"bytes\":{}}}",
        f.status,
        q(&f.url),
        q(&f.content_type),
        f.body.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_charset_comes_from_the_content_type() {
        assert_eq!(
            charset_of("text/html; charset=\"Shift_JIS\""),
            Some(Encoding::ShiftJis)
        );
        assert_eq!(charset_of("text/html;Charset=utf-8"), Some(Encoding::Utf8));
        assert_eq!(charset_of("text/html"), None);
        assert_eq!(charset_of("text/html; charset=nonsense"), None);
    }

    #[test]
    fn a_page_is_fetched_from_a_local_server_with_its_charset() {
        use std::io::{Read as _, Write as _};
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut c, _) = l.accept().unwrap();
            let mut req = [0u8; 2048];
            let n = c.read(&mut req).unwrap();
            assert!(req[..n].starts_with(b"GET /p HTTP/1.1"));
            let body = "<p>hi</p>";
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=shift_jis\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            c.write_all(head.as_bytes()).unwrap();
            c.write_all(body.as_bytes()).unwrap();
        });
        let f = fetch(&format!("http://127.0.0.1:{port}/p")).unwrap();
        server.join().unwrap();
        assert_eq!(f.status, 200);
        assert_eq!(f.charset, Some(Encoding::ShiftJis));
        assert_eq!(f.body, b"<p>hi</p>");
        assert!(matches!(fetch("file:///etc/passwd"), Err(e) if e.contains("only http and https")));
    }
}
