use log::debug;

pub fn parse_response_headers(headers: &str) -> (&str, Vec<(String, String)>) {
    debug!("Parsing response headers");

    let mut lines = headers.lines();
    let status_line = lines.next().unwrap_or("");
    debug!("Status line: {}", status_line);

    let headers = lines
        .filter_map(|line| {
            let parts: Vec<&str> = line.splitn(2, ':').collect();
            if parts.len() == 2 {
                let key = parts[0].trim().to_lowercase();
                let value = parts[1].trim().to_string();
                debug!("Header: {} = {}", key, value);
                Some((key, value))
            } else {
                debug!("Skipping invalid header line: {}", line);
                None
            }
        })
        .collect();

    (status_line, headers)
}
