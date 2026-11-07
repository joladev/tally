use jiff::Timestamp;

const HEADER_FIELDS: usize = 6;

const BOM: char = '\u{feff}';

#[derive(Debug, PartialEq)]
pub struct Request<'a> {
    pub zone: u64,
    pub time: i64,
    pub cache: &'a str,
    pub status: u16,
    pub referer: Option<&'a str>,
    pub path: &'a str,
    pub query: &'a str,
    pub user_agent: Option<&'a str>,
    pub country: &'a str,
    pub token: Option<&'a str>,
}

pub fn parse(line: &str) -> Option<Request<'_>> {
    let (structured_data, message) = split(line)?;
    let fields: [&str; 12] = message.split('|').collect::<Vec<_>>().try_into().ok()?;
    let [
        cache,
        status,
        millis,
        _bytes,
        zone,
        _ip,
        referer,
        url,
        _edge,
        user_agent,
        _id,
        country,
    ] = fields;
    let (path, query) = path_and_query(url)?;
    Some(Request {
        zone: zone.parse().ok()?,
        time: Timestamp::from_millisecond(millis.parse().ok()?)
            .ok()?
            .as_second(),
        cache,
        status: status.parse().ok()?,
        referer: present(referer),
        path,
        query,
        user_agent: present(user_agent),
        country,
        token: token(structured_data),
    })
}

fn split(line: &str) -> Option<(&str, &str)> {
    let mut rest = line.trim_end();
    for _ in 0..HEADER_FIELDS {
        rest = rest.split_once(' ')?.1;
    }
    let (structured_data, message) = rest.split_at(structured_data_length(rest)?);
    Some((
        structured_data,
        message.strip_prefix(' ')?.trim_start_matches(BOM),
    ))
}

fn structured_data_length(text: &str) -> Option<usize> {
    if text.starts_with('-') {
        return Some(1);
    }
    let bytes = text.as_bytes();
    let mut end = 0;
    while bytes.get(end) == Some(&b'[') {
        let mut quoted = false;
        loop {
            end += 1;
            match *bytes.get(end)? {
                b'\\' if quoted => end += 1,
                b'"' => quoted = !quoted,
                b']' if !quoted => break,
                _ => {}
            }
        }
        end += 1;
    }
    (end > 0).then_some(end)
}

fn token(structured_data: &str) -> Option<&str> {
    let element = structured_data.strip_prefix('[')?;
    Some(&element[..element.find('@')?])
}

fn path_and_query(url: &str) -> Option<(&str, &str)> {
    let rest = url.split_once("://")?.1;
    let target = &rest[rest.find('/').unwrap_or(rest.len())..];
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    Some((if path.is_empty() { "/" } else { path }, query))
}

fn present(field: &str) -> Option<&str> {
    (!field.is_empty() && field != "-").then_some(field)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINE: &str = "<62>1 2026-11-07T12:51:06.325703+00:00 BunnyCDN 5531598 p1 HTTP \
        [secret@32473 tag=\"joladev\"] \u{feff}HIT|200|1794055865711|7342|5531598|\
        192.0.2.0|https://example.com/|https://example.com/search?q=Jay&limit=8|UK|\
        Bluesky/1.98 (iOS)|bf803526857582332df59aad2ea0c482|GB";

    #[test]
    fn reads_a_line_as_bunny_sends_it() {
        let request = parse(LINE).unwrap();

        assert_eq!(
            request,
            Request {
                zone: 5_531_598,
                time: 1_794_055_865,
                cache: "HIT",
                status: 200,
                referer: Some("https://example.com/"),
                path: "/search",
                query: "q=Jay&limit=8",
                user_agent: Some("Bluesky/1.98 (iOS)"),
                country: "GB",
                token: Some("secret"),
            }
        );
    }

    #[test]
    fn a_line_without_a_token_has_no_structured_data() {
        let line = LINE.replace("[secret@32473 tag=\"joladev\"]", "-");

        let request = parse(&line).unwrap();

        assert_eq!((request.zone, request.token), (5_531_598, None));
    }

    #[test]
    fn rejects_lines_that_are_not_bunny_logs() {
        assert_eq!(
            parse("<165>1 2021-01-01T22:14:15.003Z host app - ID47 - hello"),
            None
        );
        assert_eq!(parse("hello"), None);
    }
}
