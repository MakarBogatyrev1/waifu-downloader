use crate::{
    api::Metadata,
    settings::{Settings, Theme},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    io::Read,
    sync::mpsc::{self, Receiver, SyncSender},
    time::Duration,
};
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Data {
    pub revision: u64,
    pub favorites: Vec<Metadata>,
    pub albums: Vec<Album>,
    pub appearance: Option<Appearance>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Album {
    pub id: String,
    pub name: String,
    pub items: Vec<String>,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Appearance {
    pub theme: Theme,
    pub accent: [u8; 3],
    pub system_accent: bool,
}
impl Appearance {
    pub fn from_settings(s: &Settings) -> Self {
        Self {
            theme: s.theme,
            accent: s.accent,
            system_accent: s.system_accent,
        }
    }
    pub fn apply(&self, s: &mut Settings) {
        s.theme = self.theme;
        s.accent = self.accent;
        s.system_accent = self.system_accent;
    }
}
#[derive(Serialize, Deserialize)]
struct Session {
    token: String,
    username: String,
    #[serde(default)]
    state: Data,
}
pub enum Job {
    Resume(String),
    Auth {
        server: String,
        username: String,
        password: String,
        register: bool,
    },
    Refresh,
    Op(Value),
    Logout,
}
pub enum Event {
    Connected {
        username: String,
        state: Data,
        remembered: bool,
    },
    State(Data),
    LoggedOut,
    Error(String, bool),
}
pub struct Worker {
    pub tx: SyncSender<Job>,
    pub rx: Receiver<Event>,
}
fn server_url(s: &str) -> Result<String, String> {
    let u = reqwest::Url::parse(s.trim()).map_err(|_| "Укажите адрес сервера".to_string())?;
    if u.scheme() != "https"
        && !(u.scheme() == "http"
            && matches!(u.host_str(), Some("127.0.0.1" | "localhost" | "[::1]")))
    {
        return Err("Для сервера нужен HTTPS".into());
    }
    if !u.username().is_empty()
        || u.password().is_some()
        || u.query().is_some()
        || u.fragment().is_some()
        || u.path() != "/"
    {
        return Err("Укажите адрес без пути и параметров".into());
    }
    Ok(u.as_str().trim_end_matches('/').into())
}
fn request(
    c: &reqwest::blocking::Client,
    base: &str,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> Result<Value, (String, bool)> {
    let mut r = if let Some(b) = body {
        c.post(format!("{base}{path}")).json(&b)
    } else {
        c.get(format!("{base}{path}"))
    };
    if let Some(t) = token {
        r = r.bearer_auth(t)
    }
    let res = r.send().map_err(|_| {
        (
            "Сервер недоступен. Проверьте подключение и адрес сервера".into(),
            false,
        )
    })?;
    let status = res.status();
    let mut bytes = vec![];
    res.take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ("Ответ сервера прерван".into(), false))?;
    if bytes.len() > 64 * 1024 * 1024 {
        return Err(("Ответ сервера слишком большой".into(), false));
    }
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|_| ("Сервер вернул неверный ответ".into(), false))?;
    if !status.is_success() {
        return Err((
            value["error"].as_str().unwrap_or("Ошибка сервера").into(),
            status.as_u16() == 401,
        ));
    }
    Ok(value)
}
impl Worker {
    pub fn start(ctx: eframe::egui::Context) -> Self {
        let (tx, jobs) = mpsc::sync_channel(4);
        let (events, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let client = reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(30))
                .connect_timeout(Duration::from_secs(10))
                .redirect(reqwest::redirect::Policy::none())
                .user_agent("WaifuGallery/2.1")
                .build();
            let mut session: Option<(String, Session)> = None;
            while let Ok(job) = jobs.recv() {
                let result = (|| -> Result<Event, (String, bool)> {
                    let c = client
                        .as_ref()
                        .map_err(|_| ("Не удалось создать сетевой клиент".into(), false))?;
                    match job {
                        Job::Auth {
                            server,
                            username,
                            password,
                            register,
                        } => {
                            let base = server_url(&server).map_err(|e| (e, false))?;
                            let v = request(
                                c,
                                &base,
                                if register {
                                    "/auth/register"
                                } else {
                                    "/auth/login"
                                },
                                None,
                                Some(json!({"username":username,"password":password})),
                            )?;
                            let s: Session = serde_json::from_value(v)
                                .map_err(|_| ("Неверный ответ входа".into(), false))?;
                            let remembered = vault::save(&base, &s).is_ok();
                            let event = Event::Connected {
                                username: s.username.clone(),
                                state: s.state.clone(),
                                remembered,
                            };
                            session = Some((base, s));
                            Ok(event)
                        }
                        Job::Resume(server) => {
                            let base = server_url(&server).map_err(|e| (e, false))?;
                            let Some(mut s) = vault::load(&base).map_err(|e| (e, false))? else {
                                return Ok(Event::LoggedOut);
                            };
                            let v = match request(c, &base, "/state", Some(&s.token), None) {
                                Ok(v) => v,
                                Err((m, expired)) => {
                                    if expired {
                                        let _ = vault::delete(&base);
                                    }
                                    return Err((m, expired));
                                }
                            };
                            s.state = serde_json::from_value(v)
                                .map_err(|_| ("Неверная коллекция".into(), false))?;
                            let event = Event::Connected {
                                username: s.username.clone(),
                                state: s.state.clone(),
                                remembered: true,
                            };
                            session = Some((base, s));
                            Ok(event)
                        }
                        Job::Logout => {
                            if let Some((base, s)) = session.as_ref() {
                                let removed = vault::delete(base).is_ok();
                                let revoked = request(
                                    c,
                                    base,
                                    "/auth/logout",
                                    Some(&s.token),
                                    Some(json!({})),
                                )
                                .is_ok();
                                if !removed && !revoked {
                                    return Err(("Не удалось удалить сохранённую сессию. Подключись к серверу и повтори выход".into(),false));
                                }
                            }
                            session = None;
                            Ok(Event::LoggedOut)
                        }
                        Job::Refresh | Job::Op(_) => {
                            let (base, s) =
                                session.as_mut().ok_or(("Войдите в аккаунт".into(), true))?;
                            let (path, body) = match job {
                                Job::Op(op) => ("/op", Some(op)),
                                _ => ("/state", None),
                            };
                            let v = request(c, base, path, Some(&s.token), body)?;
                            let state: Data = serde_json::from_value(v)
                                .map_err(|_| ("Неверная коллекция".into(), false))?;
                            s.state = state.clone();
                            Ok(Event::State(state))
                        }
                    }
                })();
                let event = match result {
                    Ok(e) => e,
                    Err((message, expired)) => {
                        if expired {
                            if let Some((base, _)) = session.take() {
                                let _ = vault::delete(&base);
                            }
                        }
                        Event::Error(message, expired)
                    }
                };
                if events.send(event).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        });
        Self { tx, rx }
    }
}
#[cfg(windows)]
mod vault {
    use super::Session;
    use windows_sys::Win32::Security::Credentials::*;
    fn target(base: &str) -> Vec<u16> {
        format!("WaifuGallery:{base}")
            .encode_utf16()
            .chain(Some(0))
            .collect()
    }
    pub fn save(base: &str, s: &Session) -> Result<(), String> {
        let mut name = target(base);
        let mut bytes =
            serde_json::to_vec(&serde_json::json!({"token":s.token,"username":s.username}))
                .map_err(|e| e.to_string())?;
        let cred = CREDENTIALW {
            Type: CRED_TYPE_GENERIC,
            TargetName: name.as_mut_ptr(),
            CredentialBlobSize: bytes.len() as u32,
            CredentialBlob: bytes.as_mut_ptr(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            ..Default::default()
        };
        if unsafe { CredWriteW(&cred, 0) } == 0 {
            Err("Не удалось сохранить сессию в Windows".into())
        } else {
            Ok(())
        }
    }
    pub fn load(base: &str) -> Result<Option<Session>, String> {
        let name = target(base);
        let mut ptr = std::ptr::null_mut();
        if unsafe { CredReadW(name.as_ptr(), CRED_TYPE_GENERIC, 0, &mut ptr) } == 0 {
            return if std::io::Error::last_os_error().raw_os_error() == Some(1168) {
                Ok(None)
            } else {
                Err("Хранилище учётных данных Windows недоступно".into())
            };
        }
        let parsed = unsafe {
            let bytes = std::slice::from_raw_parts(
                (*ptr).CredentialBlob,
                (*ptr).CredentialBlobSize as usize,
            );
            let r = serde_json::from_slice(bytes)
                .map_err(|_| "Сохранённая сессия повреждена".to_string());
            CredFree(ptr as _);
            r
        };
        parsed.map(Some)
    }
    pub fn delete(base: &str) -> Result<(), String> {
        let name = target(base);
        if unsafe { CredDeleteW(name.as_ptr(), CRED_TYPE_GENERIC, 0) } == 0
            && std::io::Error::last_os_error().raw_os_error() != Some(1168)
        {
            Err("Не удалось удалить сессию из Windows".into())
        } else {
            Ok(())
        }
    }
}
#[cfg(not(windows))]
mod vault {
    use super::Session;
    pub fn save(_: &str, _: &Session) -> Result<(), String> {
        Err("Хранилище сессий доступно только в Windows".into())
    }
    pub fn load(_: &str) -> Result<Option<Session>, String> {
        Ok(None)
    }
    pub fn delete(_: &str) -> Result<(), String> {
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn server_requires_tls_and_plain_origin() {
        assert!(server_url("https://waifudownload.hopto.org").is_ok());
        assert!(server_url("http://127.0.0.1:8787").is_ok());
        assert!(server_url("http://example.com").is_err());
        assert!(server_url("https://user:pass@example.com").is_err());
        assert!(server_url("https://example.com/path").is_err());
    }
}

#[cfg(test)]
mod integration {
    use super::*;
    #[test]
    #[ignore = "requires WAIFU_SYNC_TEST_URL pointing at an isolated local server"]
    fn client_server_and_windows_vault_roundtrip() {
        let base = std::env::var("WAIFU_SYNC_TEST_URL").expect("isolated server URL");
        assert!(base.starts_with("http://127.0.0.1:"));
        let a = Worker::start(eframe::egui::Context::default());
        let username = format!(
            "client{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        );
        let recv = |worker: &Worker| {
            worker
                .rx
                .recv_timeout(Duration::from_secs(30))
                .expect("worker response")
        };
        a.tx.send(Job::Auth {
            server: base.clone(),
            username: username.clone(),
            password: "test-password-only-12345".into(),
            register: true,
        })
        .unwrap();
        let remembered = match recv(&a) {
            Event::Connected {
                username: u,
                state,
                remembered,
            } => {
                assert_eq!(u, username);
                assert!(state.favorites.is_empty());
                remembered
            }
            Event::Error(m, _) => panic!("{m}"),
            _ => panic!("expected login"),
        };
        a.tx.send(Job::Op(json!({"op":"favorite_put","picture":{"key":"https://cdn.waifu.im/client-test.png","source":null,"artist":"Test","mode":"SFW","tag":"waifu"}}))).unwrap();
        match recv(&a) {
            Event::State(d) => assert_eq!(d.favorites.len(), 1),
            Event::Error(m, _) => panic!("{m}"),
            _ => panic!("expected state"),
        }
        a.tx.send(Job::Op(json!({"op":"appearance","appearance":{"theme":"dark","accent":[20,100,180],"system_accent":false}}))).unwrap();
        match recv(&a) {
            Event::State(d) => assert_eq!(d.appearance.unwrap().theme, Theme::Dark),
            _ => panic!("expected preferences"),
        }
        if remembered {
            let b = Worker::start(eframe::egui::Context::default());
            b.tx.send(Job::Resume(base)).unwrap();
            match recv(&b) {
                Event::Connected { state, .. } => {
                    assert_eq!(state.favorites.len(), 1);
                    assert_eq!(state.appearance.unwrap().accent, [20, 100, 180]);
                }
                Event::Error(m, _) => panic!("{m}"),
                _ => panic!("expected saved login"),
            }
            b.tx.send(Job::Logout).unwrap();
            assert!(matches!(recv(&b), Event::LoggedOut));
            a.tx.send(Job::Refresh).unwrap();
            assert!(matches!(recv(&a), Event::Error(_, true)));
        } else {
            a.tx.send(Job::Logout).unwrap();
            assert!(matches!(recv(&a), Event::LoggedOut));
            println!("Credential Manager unavailable; in-memory session tested");
        }
    }
}
