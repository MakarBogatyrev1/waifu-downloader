use crate::settings::valid_tag;
use reqwest::{
    blocking::{Client, Response},
    Url,
};
use serde_json::Value;
use std::{io::Read, time::Duration};

pub const MAX_BYTES: u64 = 32 * 1024 * 1024;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Query {
    pub mode: String,
    pub tag: String,
    pub custom_url: Option<String>,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Metadata {
    pub key: String,
    pub source: Option<String>,
    pub artist: String,
    pub mode: String,
    pub tag: String,
}
pub struct Download {
    pub bytes: Vec<u8>,
    pub meta: Metadata,
}
pub struct Api {
    client: Client,
    base: String,
}

impl Api {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            client: Client::builder()
                .user_agent("WaifuGallery/2.0")
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(30))
                .pool_idle_timeout(Duration::from_secs(90))
                .build()
                .map_err(|e| e.to_string())?,
            base: "https://api.waifu.im".into(),
        })
    }
    fn get(
        &self,
        url: &str,
        params: Option<&[(&str, &str)]>,
        api: bool,
    ) -> Result<Response, String> {
        let url = checked_url(url)?;
        for attempt in 0..4 {
            let mut request = self.client.get(url.clone());
            if api {
                request = request.header("Accept-Version", "v7");
            }
            if let Some(params) = params {
                request = request.query(params);
            }
            let response = request.send().map_err(|e| format!("Сеть: {e}"))?;
            if response.status().as_u16() == 429 && attempt < 3 {
                let delay = retry_delay(
                    response
                        .headers()
                        .get("Retry-After")
                        .and_then(|v| v.to_str().ok()),
                    attempt,
                )?;
                std::thread::sleep(delay);
                continue;
            }
            return response.error_for_status().map_err(|e| format!("API: {e}"));
        }
        unreachable!()
    }
    pub fn tags(&self) -> Result<Vec<String>, String> {
        let mut tags = vec![];
        for page in 1..=100 {
            let p = page.to_string();
            let response = self.get(
                &format!("{}/tags", self.base),
                Some(&[("Page", &p), ("PageSize", "100")]),
                true,
            )?;
            let data: Value = serde_json::from_slice(&bounded_body(response, 2 * 1024 * 1024)?)
                .map_err(|e| format!("Некорректный JSON: {e}"))?;
            let items = data
                .get("items")
                .unwrap_or(&data)
                .as_array()
                .ok_or("Некорректный список тегов")?;
            tags.extend(
                items
                    .iter()
                    .filter_map(|v| v["slug"].as_str())
                    .filter(|s| valid_tag(s))
                    .map(str::to_owned),
            );
            if !data["hasNextPage"].as_bool().unwrap_or(false) {
                tags.sort();
                tags.dedup();
                return if tags.is_empty() {
                    Err("API вернул пустой список тегов".into())
                } else {
                    Ok(tags)
                };
            }
        }
        Err("Слишком много страниц тегов".into())
    }
    pub fn direct(&self, meta: Metadata) -> Result<Download, String> {
        let bytes = bounded_body(self.get(&meta.key, None, false)?, MAX_BYTES)?;
        Ok(Download { bytes, meta })
    }
    pub fn image(&self, query: &Query) -> Result<Download, String> {
        let default_url = format!("{}/images", self.base);
        let params = [
            ("IncludedTags", query.tag.as_str()),
            (
                "IsNsfw",
                if query.mode == "NSFW" {
                    "True"
                } else {
                    "False"
                },
            ),
            ("PageSize", "1"),
        ];
        let custom = query.custom_url.as_ref().map(|url| crate::sources::expand(url, &query.tag, &query.mode));
        let response = self.get(
            custom.as_deref().unwrap_or(&default_url),
            if query.custom_url.is_none() {
                Some(&params)
            } else {
                None
            },
            query.custom_url.is_none(),
        )?;
        if custom.is_some() && response.headers().get("content-type").and_then(|v|v.to_str().ok()).is_some_and(|v|v.starts_with("image/")) {
            let key=response.url().to_string();
            return Ok(Download {bytes:bounded_body(response,MAX_BYTES)?,meta:Metadata{key,source:None,artist:String::new(),mode:query.mode.clone(),tag:query.tag.clone()}});
        }
        let data: Value = serde_json::from_slice(&bounded_body(response, 2 * 1024 * 1024)?)
            .map_err(|e| format!("Некорректный JSON: {e}"))?;
        let (url, meta) = parse_image(&data, query)?;
        let bytes = bounded_body(self.get(&url, None, false)?, MAX_BYTES)?;
        Ok(Download { bytes, meta })
    }
}
fn checked_url(url: &str) -> Result<Url, String> {
    let url = Url::parse(url).map_err(|_| "Укажите полный HTTP(S) URL".to_string())?;
    if !["http", "https"].contains(&url.scheme()) {
        return Err("Поддерживаются только HTTP(S) URL".into());
    }
    Ok(url)
}
fn bounded_body(response: Response, limit: u64) -> Result<Vec<u8>, String> {
    if response.content_length().is_some_and(|len| len > limit) {
        return Err("Файл превышает допустимый размер".into());
    }
    let mut bytes = Vec::new();
    response
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("Файл превышает допустимый размер".into());
    }
    Ok(bytes)
}
fn retry_delay(header: Option<&str>, attempt: u32) -> Result<Duration, String> {
    let delay = header
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|v| v.is_finite() && *v >= 0.0)
        .unwrap_or(2f64.powi(attempt as i32));
    if delay > 60.0 {
        return Err(format!("Лимит API: повторите запрос через {delay:.0} с"));
    }
    Ok(Duration::from_secs_f64(delay))
}
pub fn parse_image(data: &Value, query: &Query) -> Result<(String, Metadata), String> {
    let item = if query.custom_url.is_some() {
        crate::sources::image_item(data,0).map(|p|p.0).ok_or("Не найдена ссылка на изображение в JSON")?
    } else if let Some(items) = data.get("items") {
        items
            .as_array()
            .ok_or("Некорректный список изображений")?
            .first()
            .ok_or("Для этого тега и режима изображений нет")?
    } else if query.custom_url.is_some() {
        data
    } else {
        return Err("Нет списка изображений".into());
    };
    let raw = if query.custom_url.is_some() { crate::sources::image_item(item,0).map(|p|p.1) } else { item["url"].as_str() }.ok_or("Нет ссылки на изображение")?;
    let url = match checked_url(raw) {
        Ok(url)=>url,
        Err(_) if query.custom_url.is_some()=>checked_url(&crate::sources::expand(query.custom_url.as_ref().unwrap(),&query.tag,&query.mode))?.join(raw).map_err(|e|e.to_string())?,
        Err(e)=>return Err(e),
    };
    checked_url(url.as_str())?;
    let artist = item["artists"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v["name"].as_str())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    let source = item["source"]
        .as_str()
        .filter(|s| checked_url(s).is_ok())
        .map(str::to_owned);
    let mode = match item["isNsfw"].as_bool() {
        Some(true) => "NSFW".into(),
        Some(false) => "SFW".into(),
        None => query.mode.clone(),
    };
    Ok((
        url.to_string(),
        Metadata {
            key: url.to_string(),
            source,
            artist,
            mode,
            tag: query.tag.clone(),
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn query() -> Query {
        Query {
            mode: "SFW".into(),
            tag: "waifu".into(),
            custom_url: None,
        }
    }
    #[test]
    fn v7_metadata() {
        let (_, m) = parse_image(
            &serde_json::json!({"items":[{"url":"https://cdn.test/1.jpg",
            "isNsfw":true,"artists":[{"name":"Artist"}],"source":"https://artist.test"}]}),
            &query(),
        )
        .unwrap();
        assert_eq!(m.artist, "Artist");
        assert_eq!(m.mode, "NSFW");
        assert!(m.source.is_some());
    }
    #[test]
    fn empty_and_invalid_payloads() {
        for data in [
            serde_json::json!({}),
            serde_json::json!({"items":[]}),
            serde_json::json!({"items":{}}),
            serde_json::json!({"items":[null]}),
            serde_json::json!({"items":[{"url":"file:///x"}]}),
        ] {
            assert!(parse_image(&data, &query()).is_err());
        }
    }
    #[test]
    fn custom_legacy_payload() {
        let mut q = query();
        q.custom_url = Some("https://custom.test".into());
        assert!(parse_image(&serde_json::json!({"url":"https://cdn.test/x.png"}), &q).is_ok());
    }
    #[test]
    fn retry_after_is_bounded() {
        assert_eq!(
            retry_delay(Some("0.25"), 0).unwrap(),
            Duration::from_millis(250)
        );
        assert_eq!(retry_delay(Some("NaN"), 2).unwrap(), Duration::from_secs(4));
        assert_eq!(retry_delay(Some("-1"), 0).unwrap(), Duration::from_secs(1));
        assert!(retry_delay(Some("120"), 0).is_err());
    }
    #[test]
    fn http_version_filter_and_retries() {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let mut requests = vec![];
            for body in [
                None,
                Some(r#"{"items":[{"slug":"waifu"}],"hasNextPage":false}"#),
            ] {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut bytes = [0; 4096];
                let len = stream.read(&mut bytes).unwrap();
                requests.push(String::from_utf8_lossy(&bytes[..len]).into_owned());
                use std::io::Write;
                let response=match body {
                    None=>"HTTP/1.1 429 Too Many Requests\r\nRetry-After: 0\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into(),
                    Some(b)=>format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",b.len(),b)
                };
                stream.write_all(response.as_bytes()).unwrap();
            }
            requests
        });
        let api = Api {
            client: Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(4))
                .build()
                .unwrap(),
            base,
        };
        assert_eq!(api.tags().unwrap(), vec!["waifu"]);
        let requests = server.join().unwrap();
        assert!(requests
            .iter()
            .all(|r| r.to_lowercase().contains("accept-version: v7")));
        assert!(requests[0].contains("PageSize=100"));
    }
    #[test]
    #[ignore = "requires live Waifu.im network access"]
    fn live_sfw_and_nsfw_downloads() {
        let api = Api::new().unwrap();
        assert!(api.tags().unwrap().iter().any(|s| s == "waifu"));
        for mode in ["SFW", "NSFW"] {
            let q = Query {
                mode: mode.into(),
                tag: "waifu".into(),
                custom_url: None,
            };
            let download = api.image(&q).unwrap();
            assert_eq!(download.meta.mode, mode);
            let loaded = crate::assets::load(
                crate::assets::Source::Memory(std::sync::Arc::new(download.bytes)),
                download.meta,
            )
            .unwrap();
            assert!(loaded.dimensions[0] > 0 && loaded.dimensions[1] > 0);
            println!("{mode}: {}x{}", loaded.dimensions[0], loaded.dimensions[1]);
        }
    }
}
