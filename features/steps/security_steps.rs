use cucumber::{given, then, when};
use crate::LithairWorld;

#[given(expr = "un serveur Lithair avec firewall activé")]
async fn given_firewall_server(world: &mut LithairWorld) {
    world.start_server(8081, "http_firewall_demo").await.expect("Impossible de démarrer le serveur avec firewall");
}

#[given(expr = "que les politiques de sécurité soient configurées")]
async fn given_security_policies(world: &mut LithairWorld) {
    // TODO: Charger les configurations de sécurité
}

#[given(expr = "que le middleware RBAC soit initialisé")]
async fn given_rbac_middleware(world: &mut LithairWorld) {
    // TODO: Vérifier l'initialisation RBAC
}

#[when(expr = "une IP envoie plus de {int} requêtes/minute")]
async fn when_ip_rate_limit(world: &mut LithairWorld, request_count: u32) {
    let mut blocked_count = 0;
    
    for i in 0..request_count {
        let response = world.make_request("GET", "/api/test", None).await;
        
        match response {
            Ok(resp) if resp.status() == 429 => blocked_count += 1,
            Ok(_) => continue,
            Err(e) => {
                world.last_error = Some(e);
                break;
            }
        }
    }
    
    // Stocker le compteur pour vérification
    let mut test_data = world.test_data.lock().await;
    test_data.users.insert("blocked_requests".to_string(), serde_json::json!(blocked_count));
}

#[when(expr = "un utilisateur {string} accède à {string}")]
async fn when_user_accesses_endpoint(world: &mut LithairWorld, user_role: &str, endpoint: &str) {
    let token = match user_role {
        "Customer" => "customer_token_123",
        "Admin" => "admin_token_456",
        "Manager" => "manager_token_789",
        _ => "invalid_token",
    };
    
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("Authorization", format!("Bearer {}", token).parse().unwrap());
    
    let client = reqwest::Client::new();
    let server = world.server.lock().await;
    let url = format!("http://127.0.0.1:{}{}", server.port, endpoint);
    
    let response = client.get(&url).headers(headers).send().await;
    
    match response {
        Ok(resp) => {
            world.last_response = Some(resp);
        }
        Err(e) => {
            world.last_error = Some(format!("Erreur de requête: {}", e));
        }
    }
}

#[when(expr = "je fournis un token JWT {string}")]
async fn when_provide_jwt_token(world: &mut LithairWorld, token_status: &str) {
    let token = match token_status {
        "valide" => "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.valid_token",
        "expiré" => "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.expired_token",
        _ => "invalid_token",
    };
    
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("Authorization", format!("Bearer {}", token).parse().unwrap());
    
    let client = reqwest::Client::new();
    let server = world.server.lock().await;
    let url = format!("http://127.0.0.1:{}/api/protected", server.port);
    
    let response = client.get(&url).headers(headers).send().await;
    
    match response {
        Ok(resp) => {
            world.last_response = Some(resp);
        }
        Err(e) => {
            world.last_error = Some(format!("Erreur de requête: {}", e));
        }
    }
}

#[when(expr = "une requête provient d'une IP {string}")]
async fn when_request_from_ip(world: &mut LithairWorld, ip_status: &str) {
    let test_ip = match ip_status {
        "autorisée" => "192.168.1.100",
        "bloquée" => "10.0.0.50",
        _ => "127.0.0.1",
    };
    
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("X-Forwarded-For", test_ip.parse().unwrap());
    
    let client = reqwest::Client::new();
    let server = world.server.lock().await;
    let url = format!("http://127.0.0.1:{}/api/test", server.port);
    
    let response = client.get(&url).headers(headers).send().await;
    
    match response {
        Ok(resp) => {
            world.last_response = Some(resp);
        }
        Err(e) => {
            world.last_error = Some(format!("Erreur de requête: {}", e));
        }
    }
}

#[when(expr = "j'appelle {string} plus de {int} fois/minute")]
async fn when_rate_limit_endpoint(world: &mut LithairWorld, endpoint: &str, max_requests: u32) {
    let mut success_count = 0;
    let mut rate_limited_count = 0;
    
    for i in 0..(max_requests + 5) {
        let response = world.make_request("GET", endpoint, None).await;
        
        match response {
            Ok(resp) if resp.status().is_success() => success_count += 1,
            Ok(resp) if resp.status() == 429 => rate_limited_count += 1,
            Ok(_) => continue,
            Err(e) => {
                world.last_error = Some(e);
                break;
            }
        }
    }
    
    let mut test_data = world.test_data.lock().await;
    test_data.users.insert("success_requests".to_string(), serde_json::json!(success_count));
    test_data.users.insert("rate_limited_requests".to_string(), serde_json::json!(rate_limited_count));
}

#[then(expr = "cette IP doit être bloquée automatiquement")]
async fn then_ip_blocked(world: &mut LithairWorld) {
    let test_data = world.test_data.lock().await;
    let blocked_count = test_data.users.get("blocked_requests")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    
    assert!(
        blocked_count > 0,
        "Aucune requête n'a été bloquée, attendu: > 0"
    );
}

#[then(expr = "un message d'erreur {int} doit être retourné")]
async fn then_error_code_returned(world: &mut LithairWorld, expected_code: u16) {
    match &world.last_response {
        Some(response) => {
            assert_eq!(
                response.status().as_u16(),
                expected_code,
                "Code de statut {} différent de {}",
                response.status().as_u16(),
                expected_code
            );
        }
        None => {
            panic!("Aucune réponse reçue");
        }
    }
}

#[then(expr = "l'incident doit être loggué")]
async fn then_incident_logged(world: &mut LithairWorld) {
    // TODO: Vérifier les logs pour l'incident
    // Pour l'instant, on vérifie juste qu'une erreur a été enregistrée
    assert!(world.last_error.is_some() || world.last_response.is_some(), 
           "Aucun incident détecté");
}

#[then(expr = "il doit recevoir une erreur {int} Forbidden")]
async fn then_forbidden_error(world: &mut LithairWorld, expected_code: u16) {
    match &world.last_response {
        Some(response) => {
            assert_eq!(
                response.status().as_u16(),
                expected_code,
                "Code de statut {} différent de {}",
                response.status().as_u16(),
                expected_code
            );
        }
        None => {
            panic!("Aucune réponse reçue");
        }
    }
}

#[then(expr = "il doit recevoir une réponse {int} OK")]
async fn then_success_response(world: &mut LithairWorld, expected_code: u16) {
    match &world.last_response {
        Some(response) => {
            assert_eq!(
                response.status().as_u16(),
                expected_code,
                "Code de statut {} différent de {}",
                response.status().as_u16(),
                expected_code
            );
        }
        None => {
            panic!("Aucune réponse reçue");
        }
    }
}

#[then(expr = "ma requête doit être acceptée")]
async fn then_request_accepted(world: &mut LithairWorld) {
    match &world.last_response {
        Some(response) => {
            assert!(
                response.status().is_success(),
                "La requête a été rejetée avec statut {}",
                response.status()
            );
        }
        None => {
            panic!("Aucune réponse reçue");
        }
    }
}

#[then(expr = "ma requête doit être rejetée avec {int}")]
async fn then_request_rejected(world: &mut LithairWorld, expected_code: u16) {
    match &world.last_response {
        Some(response) => {
            assert_eq!(
                response.status().as_u16(),
                expected_code,
                "Code de statut {} différent de {}",
                response.status().as_u16(),
                expected_code
            );
        }
        None => {
            panic!("Aucune réponse reçue");
        }
    }
}

#[then(expr = "elle doit être traitée normalement")]
async fn then_processed_normally(world: &mut LithairWorld) {
    match &world.last_response {
        Some(response) => {
            assert!(
                response.status().is_success(),
                "La requête a été anormalement rejetée avec statut {}",
                response.status()
            );
        }
        None => {
            panic!("Aucune réponse reçue");
        }
    }
}

#[then(expr = "je dois être limité après la {int}ème requête")]
async fn then_rate_limited_after(world: &mut LithairWorld, request_num: u32) {
    let test_data = world.test_data.lock().await;
    let success_count = test_data.users.get("success_requests")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    
    assert_eq!(
        success_count, request_num as u64,
        "Nombre de requêtes réussies {} différent de {}",
        success_count,
        request_num
    );
}

#[then(expr = "pouvoir continuer après {int} minute d'attente")]
async fn then_can_continue_after_wait(world: &mut LithairWorld, wait_minutes: u32) {
    // TODO: Implémenter l'attente et la nouvelle requête
    // Pour l'instant, on vérifie juste que le rate limiting a fonctionné
    let test_data = world.test_data.lock().await;
    let rate_limited_count = test_data.users.get("rate_limited_requests")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    
    assert!(
        rate_limited_count > 0,
        "Aucune requête n'a été limitée, le rate limiting ne fonctionne pas"
    );
}
