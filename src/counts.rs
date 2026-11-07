use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::line::Request;
use crate::top::Top;

const BROWSERS: [(&str, &str); 5] = [
    ("Edg/", "Edge"),
    ("OPR/", "Opera"),
    ("Firefox/", "Firefox"),
    ("Chrome/", "Chrome"),
    ("Safari/", "Safari"),
];

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct Counts {
    pub requests: u64,
    pub statuses: BTreeMap<String, u64>,
    pub cache: BTreeMap<String, u64>,
    pub countries: BTreeMap<String, u64>,
    pub clients: Top,
    pub referrers: Top,
    pub paths: Top,
    pub params: BTreeMap<String, Top>,
}

impl Counts {
    pub fn add(&mut self, request: &Request, params: &[String]) {
        self.requests += 1;
        add(&mut self.statuses, &format!("{}xx", request.status / 100));
        add(&mut self.cache, request.cache);
        add(&mut self.countries, request.country);
        self.clients.add(client(request.user_agent));
        self.referrers.add(request.referer.map_or("none", host));
        self.paths.add(request.path);
        for param in params {
            let Some((_, value)) =
                form_urlencoded::parse(request.query.as_bytes()).find(|(name, _)| name == param)
            else {
                continue;
            };
            let value = value.trim().to_lowercase();
            if !value.is_empty() {
                self.params.entry(param.clone()).or_default().add(&value);
            }
        }
    }

    pub fn merge(&mut self, other: &Counts) {
        self.requests += other.requests;
        merge(&mut self.statuses, &other.statuses);
        merge(&mut self.cache, &other.cache);
        merge(&mut self.countries, &other.countries);
        self.clients.merge(&other.clients);
        self.referrers.merge(&other.referrers);
        self.paths.merge(&other.paths);
        for (name, top) in &other.params {
            self.params.entry(name.clone()).or_default().merge(top);
        }
    }
}

fn client(user_agent: Option<&str>) -> &str {
    let Some(user_agent) = user_agent else {
        return "unknown";
    };
    if user_agent.starts_with("Mozilla/") {
        return BROWSERS
            .iter()
            .find(|(marker, _)| user_agent.contains(marker))
            .map_or("other browser", |(_, name)| name);
    }
    user_agent.split(['/', ' ']).next().unwrap_or(user_agent)
}

fn host(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    rest.split(['/', '?', '#']).next().unwrap_or(rest)
}

fn add(counts: &mut BTreeMap<String, u64>, key: &str) {
    match counts.get_mut(key) {
        Some(count) => *count += 1,
        None => {
            counts.insert(key.to_owned(), 1);
        }
    }
}

fn merge(counts: &mut BTreeMap<String, u64>, other: &BTreeMap<String, u64>) {
    for (key, count) in other {
        *counts.entry(key.clone()).or_default() += count;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request<'a>(user_agent: &'a str, referer: Option<&'a str>, query: &'a str) -> Request<'a> {
        Request {
            zone: 1,
            time: 0,
            cache: "HIT",
            status: 200,
            referer,
            path: "/search",
            query,
            user_agent: Some(user_agent),
            country: "DE",
            token: None,
        }
    }

    #[test]
    fn names_clients_referrers_and_query_values() {
        let mut counts = Counts::default();
        let params = ["q".to_owned()];
        let browser = "Mozilla/5.0 (Macintosh) AppleWebKit/537.36 Chrome/130.0 Safari/537.36";
        counts.add(
            &request("Bluesky/1.98 (iOS)", None, "q=%20Jay&q=jay"),
            &params,
        );
        counts.add(
            &request(browser, Some("https://tangled.org/x?y"), "q=jay&limit=8"),
            &params,
        );

        let values = |top: &Top| -> Vec<String> {
            top.ranked().iter().map(|(v, _)| (*v).to_owned()).collect()
        };
        assert_eq!(values(&counts.clients), ["Bluesky", "Chrome"]);
        assert_eq!(values(&counts.referrers), ["none", "tangled.org"]);
        assert_eq!(counts.params["q"].ranked()[0].1.count, 2);
        assert_eq!(counts.statuses["2xx"], 2);
    }
}
