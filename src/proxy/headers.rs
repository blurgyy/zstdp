use log::debug;

pub fn parse_response_headers(headers: &str) -> (&str, Vec<(String, String)>) {
    debug!("Parsing response headers");

    let mut lines = headers.lines();
    let status_line = lines.next().unwrap_or("");
    debug!("Status line: {status_line}");

    let headers = lines
        .filter_map(|line| {
            let parts: Vec<&str> = line.splitn(2, ':').collect();
            if parts.len() == 2 {
                let key = parts[0].trim().to_lowercase();
                let value = parts[1].trim().to_string();
                debug!("Header: {key} = {value}");
                Some((key, value))
            } else {
                debug!("Skipping invalid header line: {line}");
                None
            }
        })
        .collect();

    (status_line, headers)
}

/// The status code of an HTTP/1.x status line, e.g. `304` for `HTTP/1.1 304 Not Modified`.
pub fn status_code(status_line: &str) -> Option<u16> {
    status_line.split_whitespace().nth(1)?.parse().ok()
}

/// Lists `Accept-Encoding` in `Vary` (appending to an existing `Vary`), since the
/// response's coding depends on the request's `Accept-Encoding`. Expects lowercase
/// header names, as produced by [`parse_response_headers`].
pub fn add_vary_accept_encoding(headers: &mut Vec<(String, String)>) {
    let already_covered = headers.iter().filter(|(k, _)| k == "vary").any(|(_, v)| {
        v.split(',')
            .map(str::trim)
            .any(|field| field == "*" || field.eq_ignore_ascii_case("accept-encoding"))
    });
    if already_covered {
        return;
    }

    match headers.iter_mut().find(|(k, _)| k == "vary") {
        Some((_, value)) if value.trim().is_empty() => *value = "Accept-Encoding".to_string(),
        Some((_, value)) => value.push_str(", Accept-Encoding"),
        None => headers.push(("Vary".to_string(), "Accept-Encoding".to_string())),
    }
}

/// Weakens a strong `ETag` (`"x"` -> `W/"x"`) on a response whose content coding
/// zstdp applied: the encoded bytes differ from upstream's, so the strong validator
/// must not match across codings. Weak comparison (`If-None-Match`) still matches
/// upstream's strong tag, so revalidation keeps working. Expects lowercase header names.
pub fn weaken_etag(headers: &mut [(String, String)]) {
    for (_, value) in headers.iter_mut().filter(|(k, _)| k == "etag") {
        let tag = value.trim();
        if tag.starts_with("W/") {
            continue;
        } else if tag.starts_with('"') {
            *value = format!("W/{tag}");
        } else {
            log::warn!("Leaving malformed ETag untouched: {tag}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn status_codes() {
        assert_eq!(status_code("HTTP/1.1 304 Not Modified"), Some(304));
        assert_eq!(status_code("HTTP/1.1 200"), Some(200));
        assert_eq!(status_code(""), None);
    }

    #[test]
    fn vary_is_added_or_appended_once() {
        let mut h = headers(&[("content-type", "text/html")]);
        add_vary_accept_encoding(&mut h);
        assert_eq!(h[1], ("Vary".into(), "Accept-Encoding".into()));

        let mut h = headers(&[("vary", "Origin")]);
        add_vary_accept_encoding(&mut h);
        assert_eq!(h, headers(&[("vary", "Origin, Accept-Encoding")]));

        for already in ["origin, accept-encoding", "*"] {
            let mut h = headers(&[("vary", already)]);
            add_vary_accept_encoding(&mut h);
            assert_eq!(h, headers(&[("vary", already)]));
        }

        let mut h = headers(&[("vary", "Origin"), ("vary", "Accept-Encoding")]);
        add_vary_accept_encoding(&mut h);
        assert_eq!(h.len(), 2);
    }

    #[test]
    fn etags_are_weakened_once() {
        let mut h = headers(&[("etag", "\"abc\"")]);
        weaken_etag(&mut h);
        assert_eq!(h, headers(&[("etag", "W/\"abc\"")]));

        let mut h = headers(&[("etag", "W/\"abc\"")]);
        weaken_etag(&mut h);
        assert_eq!(h, headers(&[("etag", "W/\"abc\"")]));
    }
}
