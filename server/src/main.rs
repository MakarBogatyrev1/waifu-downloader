use argon2::{
    password_hash::{PasswordHash, SaltString},
    Argon2, PasswordHasher, PasswordVerifier,
};
use axum::{
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use rand::{rngs::OsRng, RngCore};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::Semaphore;
type Result<T> = std::result::Result<T, Error>;
#[derive(Debug)]
struct Error(StatusCode, &'static str);
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({"error":self.1}))).into_response()
    }
}
impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        eprintln!("database operation failed: {e}");
        Self(StatusCode::INTERNAL_SERVER_ERROR, "Ошибка хранилища")
    }
}
fn bad(s: &'static str) -> Error {
    Error(StatusCode::BAD_REQUEST, s)
}
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}
fn random() -> String {
    let mut b = [0u8; 32];
    OsRng.fill_bytes(&mut b);
    b.iter().map(|v| format!("{v:02x}")).collect()
}
fn digest(s: &str) -> String {
    format!("{:x}", Sha256::digest(s.as_bytes()))
}
struct Service {
    db: Mutex<Connection>,
    hash_slots: Arc<Semaphore>,
    attempts: Mutex<HashMap<String, (i64, u32)>>,
    dummy: String,
}
#[derive(Clone, Serialize, Deserialize, Default)]
struct Data {
    revision: u64,
    favorites: Vec<Picture>,
    albums: Vec<Album>,
    appearance: Option<Appearance>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Picture {
    key: String,
    source: Option<String>,
    artist: String,
    mode: String,
    tag: String,
}
#[derive(Clone, Serialize, Deserialize)]
struct Album {
    id: String,
    name: String,
    items: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Appearance {
    theme: String,
    accent: [u8; 3],
    system_accent: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Credentials {
    username: String,
    password: String,
}
#[derive(Serialize)]
struct Session {
    token: String,
    username: String,
    state: Data,
}
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Operation {
    FavoritePut { picture: Picture },
    FavoriteRemove { key: String },
    AlbumCreate { name: String },
    AlbumRename { id: String, name: String },
    AlbumDelete { id: String },
    AlbumAdd { id: String, key: String },
    AlbumRemove { id: String, key: String },
    Appearance { appearance: Appearance },
}
fn url(s: &str) -> bool {
    s.len() <= 2048
        && (s.starts_with("https://") || s.starts_with("http://"))
        && !s.chars().any(|c| c.is_control())
}
fn name(s: &str) -> Result<()> {
    if s.trim().is_empty() || s.chars().count() > 80 || s.chars().any(|c| c.is_control()) {
        Err(bad("Название должно содержать от 1 до 80 символов"))
    } else {
        Ok(())
    }
}
fn apply(data: &mut Data, op: Operation) -> Result<()> {
    match op {
        Operation::FavoritePut { picture: p } => {
            if !url(&p.key)
                || p.source.as_ref().is_some_and(|s| !url(s))
                || p.artist.len() > 300
                || p.tag.len() > 100
                || !["SFW", "NSFW"].contains(&p.mode.as_str())
            {
                return Err(bad("Некорректное изображение"));
            }
            if let Some(old) = data.favorites.iter_mut().find(|f| f.key == p.key) {
                *old = p;
            } else {
                if data.favorites.len() >= 10000 {
                    return Err(bad("Лимит: 10000 избранных изображений"));
                }
                data.favorites.push(p);
            }
        }
        Operation::FavoriteRemove { key } => {
            data.favorites.retain(|p| p.key != key);
            for a in &mut data.albums {
                a.items.retain(|k| k != &key);
            }
        }
        Operation::AlbumCreate { name: n } => {
            name(&n)?;
            if data.albums.len() >= 100 {
                return Err(bad("Лимит: 100 альбомов"));
            }
            data.albums.push(Album {
                id: random(),
                name: n.trim().into(),
                items: vec![],
            });
        }
        Operation::AlbumRename { id, name: n } => {
            name(&n)?;
            data.albums
                .iter_mut()
                .find(|a| a.id == id)
                .ok_or(bad("Альбом не найден"))?
                .name = n.trim().into();
        }
        Operation::AlbumDelete { id } => data.albums.retain(|a| a.id != id),
        Operation::AlbumAdd { id, key } => {
            if !data.favorites.iter().any(|p| p.key == key) {
                return Err(bad("Сначала добавьте изображение в избранное"));
            }
            let a = data
                .albums
                .iter_mut()
                .find(|a| a.id == id)
                .ok_or(bad("Альбом не найден"))?;
            if !a.items.contains(&key) {
                a.items.push(key);
            }
        }
        Operation::AlbumRemove { id, key } => {
            data.albums
                .iter_mut()
                .find(|a| a.id == id)
                .ok_or(bad("Альбом не найден"))?
                .items
                .retain(|k| k != &key);
        }
        Operation::Appearance { appearance: a } => {
            if !["light", "dark", "system"].contains(&a.theme.as_str()) {
                return Err(bad("Неизвестная тема"));
            }
            data.appearance = Some(a);
        }
    }
    data.revision += 1;
    Ok(())
}
fn read_data(db: &Connection, uid: i64) -> Result<Data> {
    let s: String = db.query_row("SELECT data FROM users WHERE id=?", [uid], |r| r.get(0))?;
    serde_json::from_str(&s).map_err(|_| {
        Error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Не удалось прочитать коллекцию",
        )
    })
}
fn identity(db: &Connection, h: &HeaderMap) -> Result<(i64, String)> {
    let token = h
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .filter(|s| s.len() == 64)
        .ok_or(Error(StatusCode::UNAUTHORIZED, "Войдите в аккаунт"))?;
    db.query_row("SELECT u.id,u.username FROM sessions s JOIN users u ON u.id=s.uid WHERE s.hash=? AND s.expires>?",params![digest(token),now()],|r|Ok((r.get(0)?,r.get(1)?))).optional()?.ok_or(Error(StatusCode::UNAUTHORIZED,"Сессия истекла. Войдите снова"))
}
async fn authenticate(
    service: Arc<Service>,
    mut c: Credentials,
    register: bool,
) -> Result<Json<Session>> {
    c.username = c.username.trim().to_ascii_lowercase();
    if !(3..=32).contains(&c.username.len())
        || !c
            .username
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
    {
        return Err(bad("Логин: 3–32 символа a-z, 0-9, _ или -"));
    }
    if c.password.len() < 10 || c.password.len() > 128 {
        return Err(bad("Пароль: от 10 до 128 байт"));
    }
    {
        let mut a = service.attempts.lock().unwrap();
        a.retain(|_, v| now() - v.0 < 300);
        if a.len() >= 10000 {
            return Err(Error(StatusCode::TOO_MANY_REQUESTS, "Попробуйте позже"));
        }
        let entry = a.entry(c.username.clone()).or_insert((now(), 0));
        entry.1 += 1;
        if entry.1 > 10 {
            return Err(Error(
                StatusCode::TOO_MANY_REQUESTS,
                "Слишком много попыток. Подождите 5 минут",
            ));
        }
    }
    let slot = service
        .hash_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            Error(
                StatusCode::TOO_MANY_REQUESTS,
                "Сервер занят. Попробуйте чуть позже",
            )
        })?;
    tokio::task::spawn_blocking(move||{
  let _slot=slot;
  let existing:Option<(i64,String)>={service.db.lock().unwrap().query_row("SELECT id,password FROM users WHERE username=?",[&c.username],|r|Ok((r.get(0)?,r.get(1)?))).optional()?};
  let uid=if register {
   if existing.is_some(){return Err(Error(StatusCode::CONFLICT,"Этот логин уже занят"))}
   let hash=Argon2::default().hash_password(c.password.as_bytes(),&SaltString::generate(&mut OsRng)).map_err(|_|Error(StatusCode::INTERNAL_SERVER_ERROR,"Ошибка создания аккаунта"))?.to_string();
   let db=service.db.lock().unwrap();
   if db.query_row("SELECT count(*) FROM users",[],|r|r.get::<_,i64>(0))?>=10000{return Err(bad("Регистрация временно закрыта"))}
   match db.execute("INSERT INTO users(username,password,data) VALUES(?,?,?)",params![c.username,hash,serde_json::to_string(&Data::default()).unwrap()]){Ok(_)=>db.last_insert_rowid(),Err(rusqlite::Error::SqliteFailure(e,_)) if e.code==rusqlite::ErrorCode::ConstraintViolation=>return Err(Error(StatusCode::CONFLICT,"Этот логин уже занят")),Err(e)=>return Err(e.into())}
  }else{
   let ph=existing.as_ref().map(|v|v.1.as_str()).unwrap_or(&service.dummy);
   let valid=PasswordHash::new(ph).ok().is_some_and(|hash|Argon2::default().verify_password(c.password.as_bytes(),&hash).is_ok());
   if !valid||existing.is_none(){return Err(Error(StatusCode::UNAUTHORIZED,"Неверный логин или пароль"))}existing.unwrap().0
  };
  let token=random();let db=service.db.lock().unwrap();
  db.execute("DELETE FROM sessions WHERE expires<=?",[now()])?;
  db.execute("DELETE FROM sessions WHERE uid=? AND hash NOT IN (SELECT hash FROM sessions WHERE uid=? ORDER BY expires DESC LIMIT 9)",params![uid,uid])?;
  db.execute("INSERT INTO sessions(hash,uid,expires) VALUES(?,?,?)",params![digest(&token),uid,now()+30*86400])?;
  Ok(Json(Session{token,username:c.username,state:read_data(&db,uid)?}))
 }).await.map_err(|_|Error(StatusCode::INTERNAL_SERVER_ERROR,"Ошибка обработки запроса"))?
}
async fn register(
    State(s): State<Arc<Service>>,
    Json(c): Json<Credentials>,
) -> Result<Json<Session>> {
    authenticate(s, c, true).await
}
async fn login(State(s): State<Arc<Service>>, Json(c): Json<Credentials>) -> Result<Json<Session>> {
    authenticate(s, c, false).await
}
async fn state(State(s): State<Arc<Service>>, h: HeaderMap) -> Result<Json<Data>> {
    tokio::task::spawn_blocking(move || {
        let db = s.db.lock().unwrap();
        let (uid, _) = identity(&db, &h)?;
        Ok(Json(read_data(&db, uid)?))
    })
    .await
    .map_err(|_| {
        Error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Ошибка обработки запроса",
        )
    })?
}
fn encoded_state(data: &Data) -> Result<String> {
    let encoded = serde_json::to_string(data)
        .map_err(|_| Error(StatusCode::INTERNAL_SERVER_ERROR, "Ошибка записи коллекции"))?;
    if encoded.len() > 8 * 1024 * 1024 {
        return Err(bad(
            "Лимит коллекции: 8 МБ метаданных. Удали ненужные элементы",
        ));
    }
    Ok(encoded)
}
async fn operation(
    State(s): State<Arc<Service>>,
    h: HeaderMap,
    Json(op): Json<Operation>,
) -> Result<Json<Data>> {
    tokio::task::spawn_blocking(move || {
        let mut db = s.db.lock().unwrap();
        let tx = db.transaction()?;
        let (uid, _) = identity(&tx, &h)?;
        let mut data = read_data(&tx, uid)?;
        apply(&mut data, op)?;
        tx.execute(
            "UPDATE users SET data=? WHERE id=?",
            params![encoded_state(&data)?, uid],
        )?;
        tx.commit()?;
        Ok(Json(data))
    })
    .await
    .map_err(|_| {
        Error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Ошибка обработки запроса",
        )
    })?
}
async fn logout(State(s): State<Arc<Service>>, h: HeaderMap) -> Result<Json<serde_json::Value>> {
    let db = s.db.lock().unwrap();
    identity(&db, &h)?;
    let token = h["authorization"]
        .to_str()
        .unwrap()
        .strip_prefix("Bearer ")
        .unwrap();
    db.execute("DELETE FROM sessions WHERE hash=?", [digest(token)])?;
    Ok(Json(serde_json::json!({"ok":true})))
}
#[tokio::main]
async fn main() {
    let path = std::env::var("DATABASE_PATH").unwrap_or("waifu-sync.sqlite3".into());
    let db = Connection::open(path).expect("open database");
    db.busy_timeout(std::time::Duration::from_secs(5)).unwrap();
    db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; CREATE TABLE IF NOT EXISTS users(id INTEGER PRIMARY KEY,username TEXT UNIQUE NOT NULL,password TEXT NOT NULL,data TEXT NOT NULL); CREATE TABLE IF NOT EXISTS sessions(hash TEXT PRIMARY KEY,uid INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,expires INTEGER NOT NULL); CREATE INDEX IF NOT EXISTS sessions_uid ON sessions(uid);").expect("initialize database");
    let dummy = Argon2::default()
        .hash_password(random().as_bytes(), &SaltString::generate(&mut OsRng))
        .unwrap()
        .to_string();
    let service = Arc::new(Service {
        db: Mutex::new(db),
        hash_slots: Arc::new(Semaphore::new(2)),
        attempts: Mutex::new(HashMap::new()),
        dummy,
    });
    let app = Router::new()
        .route(
            "/health",
            get(|| async { Json(serde_json::json!({"ok":true,"version":"2.1.0"})) }),
        )
        .route("/auth/register", post(register))
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/state", get(state))
        .route("/op", post(operation))
        .layer(DefaultBodyLimit::max(16384))
        .with_state(service);
    let bind = std::env::var("BIND_ADDRESS").unwrap_or("127.0.0.1:8787".into());
    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .expect("bind address");
    println!("Waifu Sync listening on {bind}");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            #[cfg(unix)]
            {
                let mut term =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                        .expect("signal handler");
                tokio::select! { _=tokio::signal::ctrl_c()=>{}, _=term.recv()=>{} }
            }
            #[cfg(not(unix))]
            {
                let _ = tokio::signal::ctrl_c().await;
            }
        })
        .await
        .unwrap();
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn delete_favorite_cleans_membership() {
        let mut d = Data::default();
        let p = Picture {
            key: "https://example.com/a.png".into(),
            source: None,
            artist: String::new(),
            mode: "SFW".into(),
            tag: "waifu".into(),
        };
        let k = p.key.clone();
        apply(&mut d, Operation::FavoritePut { picture: p }).unwrap();
        apply(&mut d, Operation::AlbumCreate { name: "A".into() }).unwrap();
        let id = d.albums[0].id.clone();
        apply(&mut d, Operation::AlbumAdd { id, key: k.clone() }).unwrap();
        apply(&mut d, Operation::FavoriteRemove { key: k }).unwrap();
        assert!(d.favorites.is_empty());
        assert!(d.albums[0].items.is_empty());
    }
    #[test]
    fn total_metadata_is_bounded() {
        let mut data = Data::default();
        data.albums.push(Album {
            id: "test".into(),
            name: "test".into(),
            items: vec!["x".repeat(8 * 1024 * 1024)],
        });
        assert!(encoded_state(&data).is_err());
        data.albums.clear();
        assert!(encoded_state(&data).is_ok());
    }
    #[test]
    fn missing_album_cannot_receive_items() {
        let mut d = Data::default();
        assert!(apply(
            &mut d,
            Operation::AlbumAdd {
                id: "other".into(),
                key: "unknown".into()
            }
        )
        .is_err());
        assert_eq!(d.revision, 0);
    }
}
