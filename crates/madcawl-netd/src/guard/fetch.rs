//! What a content process's FETCH may ask for. Besides its initiator and
//! credentials, a download: like a link with a `download` attribute, it
//! may ask for a response of its document's origin to be saved; another
//! origin's would be a navigation, which only the host fetches. A
//! response saved because it is an attachment or of a type not shown as a
//! page is always a navigation's, so the host's too. A download it asks
//! for is fetched with redirects refused, since a redirect could take the
//! saved response, and the cookies sent for it, to another origin.

use super::{flag_of, same_site};
use madcawl_net::Url;
use madcawl_net::wire::{Frame, tag};

/// the FETCH `REDIRECT` value that fails the fetch on a redirect
const REDIRECT_ERROR: u64 = 2;

fn same_origin(a: &Url, b: &Url) -> bool {
    a.scheme() == b.scheme() && a.host_str().is_some() && a.host_str() == b.host_str() && a.port_or_known_default() == b.port_or_known_default()
}

/// A FETCH with one `DOWNLOAD` value, the lowest it carried, when it
/// carried any; refused when it asks to save a response of another origin
/// than the document, and with redirects refused when it asks to save one.
fn screen_download(doc: &Url, mut frame: Frame) -> Result<Frame, &'static str> {
    let Some(asked) = flag_of(&frame, tag::DOWNLOAD).map_err(|()| "a download value other than 0 and 1")? else { return Ok(frame) };
    frame.fields.retain(|(t, _)| *t != tag::DOWNLOAD);
    if asked == 1 && !frame.get_str(tag::URL).and_then(|s| Url::parse(s).ok()).is_some_and(|u| same_origin(&u, doc)) {
        return Err("a download of another origin than the document");
    }
    if asked == 1 {
        frame.fields.retain(|(t, _)| *t != tag::REDIRECT);
        frame = frame.num(tag::REDIRECT, REDIRECT_ERROR);
    }
    Ok(frame.num(tag::DOWNLOAD, asked))
}

/// The lowest of a FETCH's credentials values, ranked omit (0), then
/// same-origin (2), then include (1); include when it carried none.
fn credentials_of(frame: &Frame) -> Result<u64, &'static str> {
    let rank = |v: &[u8]| match v {
        [0] => Ok(0),
        [2] => Ok(1),
        [1] => Ok(2),
        _ => Err("a credentials value other than 0, 1 and 2"),
    };
    let low = frame.all(tag::CREDENTIALS).try_fold(2, |low: u8, v| rank(v).map(|r| low.min(r)))?;
    Ok([0, 2, 1][usize::from(low)])
}

/// A FETCH with its initiator checked and, when it claimed none, set to
/// the document, never a navigation, with one credentials value and a
/// download only of the document's origin; the reason when it is refused.
pub(super) fn screen(doc: &Url, mut frame: Frame) -> Result<Frame, &'static str> {
    if frame.all(tag::NAVIGATION).any(|v| v != [0]) {
        return Err("a navigation from a content process");
    }
    frame = screen_download(doc, frame)?;
    let credentials = credentials_of(&frame)?;
    let mut claimed = None;
    for raw in frame.all(tag::INITIATOR) {
        let url = std::str::from_utf8(raw).ok().and_then(|s| Url::parse(s).ok()).ok_or("an initiator that is not a url")?;
        if !same_site(&url, doc) {
            return Err("an initiator of another site than the document");
        }
        claimed.get_or_insert(url);
    }
    let initiator = claimed.unwrap_or_else(|| doc.clone());
    frame.fields.retain(|(t, _)| !matches!(*t, tag::INITIATOR | tag::NAVIGATION | tag::CREDENTIALS));
    Ok(frame.str(tag::INITIATOR, initiator.as_str()).num(tag::NAVIGATION, 0).num(tag::CREDENTIALS, credentials))
}

#[cfg(test)]
mod tests {
    use super::*;
    use madcawl_net::wire::kind;

    fn fetch(url: &str, values: &[u64]) -> Frame {
        values.iter().fold(Frame::new(kind::FETCH).str(tag::URL, url), |f, &v| f.num(tag::DOWNLOAD, v))
    }

    #[test]
    fn the_lowest_credentials_value_holds() {
        let creds = |values: &[u64]| credentials_of(&values.iter().fold(Frame::new(kind::FETCH), |f, &v| f.num(tag::CREDENTIALS, v)));
        assert_eq!([creds(&[]), creds(&[1]), creds(&[1, 2]), creds(&[2, 1]), creds(&[2, 0]), creds(&[1, 0])], [Ok(1), Ok(1), Ok(2), Ok(2), Ok(0), Ok(0)]);
        assert!(creds(&[3]).is_err() && creds(&[1, 3]).is_err());
    }

    #[test]
    fn only_the_documents_origin_may_be_saved() {
        let doc = Url::parse("https://www.example.com/page").unwrap();
        let ok = screen_download(&doc, fetch("https://www.example.com:443/report.pdf", &[1]).num(tag::REDIRECT, 0)).unwrap();
        assert_eq!(ok.all(tag::DOWNLOAD).collect::<Vec<_>>(), [[1]]);
        assert_eq!(ok.all(tag::REDIRECT).collect::<Vec<_>>(), [[2]], "a download never follows a redirect");
        for other in ["https://cdn.example.com/a", "http://www.example.com/a", "https://www.example.com:8443/a", "data:,x", "bad"] {
            assert!(screen_download(&doc, fetch(other, &[1])).is_err(), "{other}");
        }
        // asking for no download, or for both, saves nothing and needs no check
        let low = screen_download(&doc, fetch("https://cdn.example.com/a", &[1, 0])).unwrap();
        assert_eq!(low.all(tag::DOWNLOAD).collect::<Vec<_>>(), [[0]]);
        assert!(low.field(tag::REDIRECT).is_none());
        assert!(screen_download(&doc, fetch("https://cdn.example.com/a", &[])).unwrap().field(tag::DOWNLOAD).is_none());
        assert!(screen_download(&doc, fetch("https://www.example.com/a", &[2])).is_err());
    }
}
