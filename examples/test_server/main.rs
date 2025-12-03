/// Serveur de test pour les tests Robot Framework
/// Usage: test_server --port 8080 --persist /tmp/data
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Article {
    id: u64,
    title: String,
    content: String,
}

#[derive(Clone)]
struct AppState {
    articles: Arc<Mutex<HashMap<u64, Article>>>,
    next_id: Arc<Mutex<u64>>,
    persist_path: Option<String>,
}

impl AppState {
    fn new(persist_path: Option<String>) -> Self {
        let state = Self {
            articles: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(Mutex::new(0)),
            persist_path,
        };
        
        // Charger depuis le fichier si il existe
        if let Some(ref path) = state.persist_path {
            let log_file = format!("{}/events.raftlog", path);
            if let Ok(content) = fs::read_to_string(&log_file) {
                let mut articles = state.articles.lock().unwrap();
                let mut next_id = state.next_id.lock().unwrap();
                
                for line in content.lines() {
                    if let Ok(article) = serde_json::from_str::<Article>(line) {
                        *next_id = (*next_id).max(article.id + 1);
                        articles.insert(article.id, article);
                    }
                }
                println!("✅ Chargé {} articles depuis {}", articles.len(), log_file);
            }
        }
        
        state
    }
    
    fn persist_article(&self, article: &Article) {
        if let Some(ref path) = self.persist_path {
            fs::create_dir_all(path).ok();
            let log_file = format!("{}/events.raftlog", path);
            
            if let Ok(mut file) = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_file)
            {
                if let Ok(json) = serde_json::to_string(article) {
                    writeln!(file, "{}", json).ok();
                    file.sync_all().ok(); // fsync
                }
            }
        }
    }
}

fn handle_request(stream: &mut TcpStream, state: &AppState) {
    // Activer TCP_NODELAY pour réduire la latence
    stream.set_nodelay(true).ok();
    
    // Timeout pour éviter les connexions bloquées
    stream.set_read_timeout(Some(std::time::Duration::from_secs(30))).ok();
    
    // Boucle pour supporter HTTP/1.1 keep-alive
    loop {
        let mut buffer = [0; 8192];
        
        // Lire la requête
        let n = match stream.read(&mut buffer) {
            Ok(0) => break, // Client a fermé la connexion
            Ok(n) => n,
            Err(_) => break,
        };
        
        let request = match std::str::from_utf8(&buffer[..n]) {
            Ok(s) => s,
            Err(_) => break,
        };
        
        // Parser la requête HTTP
        let (method, path, headers, body) = match parse_http_request(request) {
            Some(parsed) => parsed,
            None => {
                let _ = stream.write_all(b"HTTP/1.1 400 BAD REQUEST\r\n\r\n");
                break;
            }
        };
        
        // Traiter la requête
        let response = match (method, path) {
            ("POST", "/api/articles") => handle_create_article(body, state),
            ("GET", "/api/articles") => handle_list_articles(state),
            ("GET", "/health") => "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: keep-alive\r\n\r\n{\"status\":\"ok\"}".to_string(),
            _ => "HTTP/1.1 404 NOT FOUND\r\nConnection: keep-alive\r\n\r\n".to_string(),
        };
        
        // Envoyer la réponse
        if stream.write_all(response.as_bytes()).is_err() {
            break;
        }
        if stream.flush().is_err() {
            break;
        }
        
        // Vérifier si le client veut fermer la connexion
        if headers.get("connection").map(|v| v.to_lowercase()) == Some("close".to_string()) {
            break;
        }
    }
}

fn parse_http_request(request: &str) -> Option<(&str, &str, HashMap<String, String>, &str)> {
    let mut lines = request.lines();
    
    // Parser la ligne de requête (GET /path HTTP/1.1)
    let request_line = lines.next()?;
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 {
        return None;
    }
    
    let method = parts[0];
    let path = parts[1];
    
    // Parser les headers
    let mut headers = HashMap::new();
    let mut body_start = 0;
    
    for (_i, line) in lines.enumerate() {
        if line.is_empty() {
            body_start = request.find("\r\n\r\n")
                .or_else(|| request.find("\n\n"))
                .map(|idx| idx + 4)
                .unwrap_or(request.len());
            break;
        }
        
        if let Some(colon_pos) = line.find(':') {
            let key = line[..colon_pos].trim().to_lowercase();
            let value = line[colon_pos + 1..].trim().to_string();
            headers.insert(key, value);
        }
    }
    
    // Extraire le body
    let body = if body_start < request.len() {
        &request[body_start..]
    } else {
        ""
    };
    
    Some((method, path, headers, body))
}

fn handle_create_article(body: &str, state: &AppState) -> String {
    #[derive(Deserialize)]
    struct CreateArticle {
        title: String,
        content: String,
    }
    
    let create_req: CreateArticle = match serde_json::from_str(body) {
        Ok(req) => req,
        Err(_) => {
            return "HTTP/1.1 400 BAD REQUEST\r\nContent-Type: application/json\r\n\r\n{\"error\":\"Invalid JSON\"}".to_string();
        }
    };
    
    let mut next_id = state.next_id.lock().unwrap();
    let id = *next_id;
    *next_id += 1;
    drop(next_id);
    
    let article = Article {
        id,
        title: create_req.title,
        content: create_req.content,
    };
    
    // Persister
    state.persist_article(&article);
    
    // Stocker en mémoire
    let mut articles = state.articles.lock().unwrap();
    articles.insert(id, article.clone());
    drop(articles);
    
    let json = serde_json::to_string(&article).unwrap();
    format!("HTTP/1.1 201 CREATED\r\nContent-Type: application/json\r\nConnection: keep-alive\r\nContent-Length: {}\r\n\r\n{}", json.len(), json)
}

fn handle_list_articles(state: &AppState) -> String {
    let articles = state.articles.lock().unwrap();
    let list: Vec<Article> = articles.values().cloned().collect();
    let json = serde_json::to_string(&list).unwrap();
    
    format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: keep-alive\r\nContent-Length: {}\r\n\r\n{}", json.len(), json)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    
    // Parser arguments
    let mut port = 8080u16;
    let mut persist_path: Option<String> = None;
    
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--port" | "-p" => {
                if i + 1 < args.len() {
                    port = args[i + 1].parse().unwrap_or(8080);
                    i += 2;
                } else {
                    i += 1;
                }
            }
            "--persist" => {
                if i + 1 < args.len() {
                    persist_path = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    i += 1;
                }
            }
            _ => {
                i += 1;
            }
        }
    }
    
    println!("🚀 Test Server");
    println!("   Port: {}", port);
    println!("   Persist: {:?}", persist_path);
    
    let state = Arc::new(AppState::new(persist_path));
    
    let addr = format!("127.0.0.1:{}", port);
    let listener = match TcpListener::bind(&addr) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("❌ Erreur bind sur {}: {}", addr, e);
            std::process::exit(1);
        }
    };
    
    println!("✅ Serveur démarré sur http://{}", addr);
    println!("   Endpoints:");
    println!("   - POST   /api/articles");
    println!("   - GET    /api/articles");
    println!("   - GET    /health");
    
    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                let state = Arc::clone(&state);
                std::thread::spawn(move || {
                    handle_request(&mut stream, &state);
                });
            }
            Err(e) => {
                eprintln!("❌ Erreur connexion: {}", e);
            }
        }
    }
}
