use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SourceProfile {
    pub name: String,
    pub url: String,
    pub categories_url: String,
    pub categories: Vec<String>,
    pub categories_mode: String,
}
pub fn category_ok(s: &str) -> bool {
    !s.trim().is_empty() && s.len() <= 100 && !s.contains("://") && !s.chars().any(|c| c.is_control())
}
fn encode(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
        (b as char).to_string()
    } else { format!("%{b:02X}") }).collect()
}
pub fn expand(url: &str, category: &str, mode: &str) -> String {
    url.replace("{category}", &encode(category)).replace("{tag}", &encode(category))
        .replace("{mode}", &mode.to_lowercase())
}
pub fn infer_template(input: &str, tags: &[String]) -> String {
    if input.contains('{') {return input.into();}
    let Ok(mut url)=reqwest::Url::parse(input) else {return input.into();};
    let mut found=false;
    let mut segments:Vec<String>=url.path_segments().map(|s|s.map(str::to_owned).collect()).unwrap_or_default();
    if let Some(last)=segments.last_mut() {if tags.contains(last){*last="{category}".into();found=true;}}
    let pairs:Vec<(String,String)>=url.query_pairs().map(|(k,v)|{
        let value=if ["tag","category","included_tags","IncludedTags"].contains(&k.as_ref())&&tags.iter().any(|t|t==v.as_ref()){found=true;"{category}".to_string()}else{v.into_owned()};(k.into_owned(),value)
    }).collect();
    if !found {return input.into();}
    for segment in &mut segments {if segment=="sfw"||segment=="nsfw"{*segment="{mode}".into();}}
    if let Ok(mut path)=url.path_segments_mut(){path.clear();for segment in segments{path.push(&segment);}}
    if !pairs.is_empty(){url.query_pairs_mut().clear().extend_pairs(pairs);}
    url.to_string().replace("%7Bcategory%7D","{category}").replace("%7Bmode%7D","{mode}")
}
pub fn categories(data: &Value, mode: &str) -> Vec<String> {
    fn walk(v: &Value, mode: &str, out: &mut Vec<String>, depth: usize) {
        if depth > 8 || out.len() >= 2000 { return; }
        match v {
            Value::String(s) if category_ok(s) => out.push(s.trim().to_owned()),
            Value::Array(a) => for v in a.iter().take(2000) { walk(v,mode,out,depth+1); },
            Value::Object(o) => {
                if let Some(s)=o.get("slug").or_else(||o.get("name")).and_then(Value::as_str) {
                    if category_ok(s) { out.push(s.trim().to_owned()); }
                    return;
                }
                if let Some(v)=o.get(&mode.to_lowercase()) { walk(v,mode,out,depth+1); return; }
                for key in ["categories","tags","endpoints","items","data","results","versatile"] {
                    if let Some(v)=o.get(key) { walk(v,mode,out,depth+1); }
                }
            }
            _=>{}
        }
    }
    let mut out=vec![];walk(data,mode,&mut out,0);out.sort();out.dedup();out.truncate(2000);out
}
pub fn image_item(data: &Value, depth: usize) -> Option<(&Value, &str)> {
    if depth > 8 { return None; }
    if let Some(s)=data.as_str() { return Some((data,s)); }
    if let Some(o)=data.as_object() {
        for key in ["url","file_url","image_url","large_file_url","src"] {
            if let Some(s)=o.get(key).and_then(Value::as_str) { return Some((data,s)); }
        }
        for key in ["items","images","results","data","image","file"] {
            if let Some(v)=o.get(key) { if let Some(found)=image_item(v,depth+1){return Some(found);} }
        }
    }
    if let Some(a)=data.as_array() { for v in a.iter().take(100) {if let Some(found)=image_item(v,depth+1){return Some(found);}} }
    None
}
pub fn discover(profile: &SourceProfile, mode: &str) -> Result<Vec<String>,String> {
    let url=reqwest::Url::parse(&expand(&profile.url,"waifu",mode)).map_err(|_|"Укажите полный URL API")?;
    if !["http","https"].contains(&url.scheme()) || !url.username().is_empty() || url.password().is_some() {return Err("Нужен HTTP(S) URL без логина и пароля".into());}
    let client=reqwest::blocking::Client::builder().timeout(std::time::Duration::from_secs(6)).user_agent("WaifuGallery").build().map_err(|e|e.to_string())?;
    let mut candidates=vec![];
    if !profile.categories_url.trim().is_empty() {candidates.push(expand(profile.categories_url.trim(),"waifu",mode));}
    else {
        for path in ["tags","categories","endpoints"] {if let Ok(u)=url.join(path){candidates.push(u.to_string());}}
        candidates.push(url.to_string());
    }
    let mut last=String::new();
    for candidate in candidates {
        let result=(|| -> Result<Vec<String>,String> {
            let u=reqwest::Url::parse(&candidate).map_err(|e|e.to_string())?;
            if !["http","https"].contains(&u.scheme()) {return Err("Нужен HTTP(S) URL".into());}
            let mut collected=vec![];
            for page in 1..=20 {
            let mut request=client.get(u.clone());if page>1{request=request.query(&[("Page",page.to_string())]);}
            let response=request.send().and_then(|r|r.error_for_status()).map_err(|e|e.to_string())?;
            use std::io::Read;
            let mut body=vec![];response.take(2*1024*1024+1).read_to_end(&mut body).map_err(|e|e.to_string())?;
            if body.len()>2*1024*1024{return Err("Список категорий слишком большой".into());}
            let data:Value=serde_json::from_slice(&body).map_err(|_|"Ответ категорий должен быть JSON")?;
            let tags=categories(&data,mode);
            if tags.is_empty(){return Err("В JSON не найдены категории".into());}
            collected.extend(tags);collected.sort();collected.dedup();
            if !data["hasNextPage"].as_bool().unwrap_or(false) {collected.truncate(2000);return Ok(collected);}
            }
            Err("Слишком много страниц категорий (более 20)".into())
        })();
        match result {Ok(tags)=>return Ok(tags),Err(e)=>last=e}
    }
    Err(format!("Категории не определены: {last}. Укажите URL списка категорий или добавьте их вручную."))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn categories_respect_mode_and_ignore_metadata() {
        let v=serde_json::json!({"sfw":["waifu",{"name":"cat ears"}],"nsfw":["adult"],"message":"ignore"});
        assert_eq!(categories(&v,"SFW"),vec!["cat ears","waifu"]);
        assert_eq!(categories(&v,"NSFW"),vec!["adult"]);
        assert!(categories(&serde_json::json!({"message":"not a category"}),"SFW").is_empty());
    }
    #[test] fn nested_images_and_safe_templates() {
        let v=serde_json::json!({"data":[{"file_url":"https://cdn.test/a.png"}]});
        assert_eq!(image_item(&v,0).unwrap().1,"https://cdn.test/a.png");
        assert_eq!(expand("https://x/{mode}/{category}","a/b &猫","SFW"),"https://x/sfw/a%2Fb%20%26%E7%8C%AB");
        assert_eq!(infer_template("https://x/sfw/waifu",&["waifu".into()]),"https://x/{mode}/{category}");
        assert_eq!(infer_template("https://x/random?category=waifu",&["waifu".into()]),"https://x/random?category={category}");
        assert_eq!(infer_template("https://x/random",&["waifu".into()]),"https://x/random");
    }
    #[test] fn custom_api_discovers_categories_and_downloads_nested_image() {
        use std::io::{Read,Write};
        let listener=std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base=format!("http://{}",listener.local_addr().unwrap());
        let server_base=base.clone();
        let server=std::thread::spawn(move || {
            for _ in 0..3 {
                let (mut stream,_)=listener.accept().unwrap();stream.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
                let mut buffer=[0u8;8192];let n=stream.read(&mut buffer).unwrap();let request=String::from_utf8_lossy(&buffer[..n]);
                let (kind,body)=if request.starts_with("GET /categories ") {("application/json",r#"{"categories":[{"slug":"cat ears"},"waifu"]}"#.to_string())}
                else if request.starts_with("GET /sfw/cat%20ears ") {assert!(!request.to_lowercase().contains("accept-version:"));("application/json",format!(r#"{{"data":[{{"file_url":"{server_base}/image"}}]}}"#))}
                else {assert!(request.starts_with("GET /image "));("image/png","fixture bytes".to_string())};
                write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
            }
        });
        let profile=SourceProfile{url:format!("{base}/{{mode}}/{{category}}"),categories_url:format!("{base}/categories"),..Default::default()};
        assert_eq!(discover(&profile,"SFW").unwrap(),vec!["cat ears","waifu"]);
        let image=crate::api::Api::new().unwrap().image(&crate::api::Query{custom_url:Some(profile.url),mode:"SFW".into(),tag:"cat ears".into()}).unwrap();
        assert_eq!(image.bytes,b"fixture bytes");assert_eq!(image.meta.key,format!("{base}/image"));
        server.join().unwrap();
    }
}
