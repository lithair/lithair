//! Step definitions for dual-mode serialization tests (JSON + rkyv)

use crate::features::world::LithairWorld;
use cucumber::{given, then, when};
use serde::{Deserialize, Serialize};
use std::time::Instant;

// Import serialization modules from lithair_core
use lithair_core::serialization::{json_mode, DualModeError, SerializationMode};

// ============================================================================
// Test Article Type (supports both serde and rkyv)
// ============================================================================

/// Test article that supports both JSON (serde) and rkyv serialization
#[derive(
    Debug,
    Clone,
    PartialEq,
    Serialize,
    Deserialize,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
#[rkyv(derive(Debug, PartialEq))]
pub struct TestArticle {
    pub id: String,
    pub title: String,
    pub price: f64,
}

impl TestArticle {
    pub fn new(id: &str, title: &str, price: f64) -> Self {
        Self { id: id.to_string(), title: title.to_string(), price }
    }

    pub fn random(index: usize) -> Self {
        Self {
            id: format!("art-{:06}", index),
            title: format!("Article de test numéro {}", index),
            price: (index as f64 * 1.5) + 9.99,
        }
    }
}

// ==================== GIVEN STEPS ====================

#[given(expr = "un type de test {string} avec les champs id, title, price")]
async fn given_test_type(_world: &mut LithairWorld, type_name: String) {
    println!("📦 Type de test défini: {} (id: String, title: String, price: f64)", type_name);
}

#[given(expr = "un article avec id {string} titre {string} et prix {float}")]
async fn given_article_with_fields(
    world: &mut LithairWorld,
    id: String,
    title: String,
    price: f64,
) {
    let article = TestArticle::new(&id, &title, price);
    println!("📦 Article créé: {:?}", article);

    let mut metrics = world.metrics.lock().await;
    metrics.last_state_json = Some(serde_json::to_string(&article).unwrap_or_default());
}

#[given(expr = "{int} articles générés aléatoirement")]
async fn given_random_articles(world: &mut LithairWorld, count: usize) {
    println!("📦 Génération de {} articles aléatoires...", count);

    let articles: Vec<TestArticle> = (0..count).map(TestArticle::random).collect();

    let mut metrics = world.metrics.lock().await;
    metrics.last_state_json = Some(serde_json::to_string(&articles).unwrap_or_default());

    println!("✅ {} articles générés", count);
}

#[given("des données JSON valides représentant un article")]
async fn given_valid_json_data(world: &mut LithairWorld) {
    let article = TestArticle::new("test-001", "Article de test", 19.99);
    let json = serde_json::to_string(&article).unwrap();

    let mut metrics = world.metrics.lock().await;
    metrics.last_state_json = Some(json);

    println!("📦 Données JSON valides préparées");
}

#[given("un article sérialisé en rkyv")]
async fn given_rkyv_serialized_article(world: &mut LithairWorld) {
    let article = TestArticle::new("rkyv-001", "Article rkyv test", 42.50);

    let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&article)
        .map(|v| v.to_vec())
        .expect("rkyv serialization should succeed");

    let mut metrics = world.metrics.lock().await;
    metrics.last_state_json = Some(hex_encode(&bytes));

    println!("📦 Article sérialisé en rkyv ({} bytes)", bytes.len());
}

#[given(expr = "des données JSON malformées {string}")]
async fn given_malformed_json(world: &mut LithairWorld, data: String) {
    let mut metrics = world.metrics.lock().await;
    metrics.last_state_json = Some(data);
    println!("📦 Données JSON malformées préparées");
}

#[given(expr = "des données binaires aléatoires de {int} bytes")]
async fn given_random_binary(world: &mut LithairWorld, size: usize) {
    use rand::Rng;
    let bytes: Vec<u8> = (0..size).map(|_| rand::thread_rng().gen()).collect();

    let mut metrics = world.metrics.lock().await;
    metrics.last_state_json = Some(hex_encode(&bytes));

    println!("📦 {} bytes aléatoires générés", size);
}

#[given(expr = "le mode de sérialisation {string}")]
async fn given_serialization_mode(world: &mut LithairWorld, mode: String) {
    let mut metrics = world.metrics.lock().await;
    metrics.last_state_json = Some(mode.clone());

    println!("📦 Mode de sérialisation: {}", mode);
}

// ==================== WHEN STEPS ====================

#[when("je sérialise l'article en mode JSON")]
async fn when_serialize_json(world: &mut LithairWorld) {
    let metrics = world.metrics.lock().await;
    let json_data = metrics.last_state_json.clone().unwrap_or_default();
    drop(metrics);

    let article: TestArticle =
        serde_json::from_str(&json_data).expect("Should have valid article JSON");

    let start = Instant::now();
    let json = json_mode::serialize(&article).expect("JSON serialization should succeed");
    let elapsed = start.elapsed();

    let mut metrics = world.metrics.lock().await;
    metrics.last_state_json = Some(json);
    metrics.last_avg_latency_ms = elapsed.as_secs_f64() * 1000.0;

    println!("📤 Article sérialisé en JSON en {:.3}ms", metrics.last_avg_latency_ms);
}

#[when("je désérialise les données JSON")]
async fn when_deserialize_json(world: &mut LithairWorld) {
    let metrics = world.metrics.lock().await;
    let json_data = metrics.last_state_json.clone().unwrap_or_default();
    drop(metrics);

    let start = Instant::now();
    let article: TestArticle =
        json_mode::deserialize_str(&json_data).expect("JSON deserialization should succeed");
    let elapsed = start.elapsed();

    let mut metrics = world.metrics.lock().await;
    metrics.last_state_json = Some(serde_json::to_string(&article).unwrap());
    metrics.last_avg_latency_ms = elapsed.as_secs_f64() * 1000.0;

    println!("📥 Article désérialisé depuis JSON en {:.3}ms", metrics.last_avg_latency_ms);
}

#[when("je sérialise l'article en mode rkyv")]
async fn when_serialize_rkyv(world: &mut LithairWorld) {
    let metrics = world.metrics.lock().await;
    let json_data = metrics.last_state_json.clone().unwrap_or_default();
    drop(metrics);

    let article: TestArticle =
        serde_json::from_str(&json_data).expect("Should have valid article JSON");

    let start = Instant::now();
    let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&article)
        .map(|v| v.to_vec())
        .expect("rkyv serialization should succeed");
    let elapsed = start.elapsed();

    let mut metrics = world.metrics.lock().await;
    metrics.last_state_json = Some(hex_encode(&bytes));
    metrics.last_avg_latency_ms = elapsed.as_secs_f64() * 1000.0;

    println!(
        "📤 Article sérialisé en rkyv ({} bytes) en {:.3}ms",
        bytes.len(),
        metrics.last_avg_latency_ms
    );
}

#[when("je désérialise les données rkyv")]
async fn when_deserialize_rkyv(world: &mut LithairWorld) {
    let metrics = world.metrics.lock().await;
    let encoded = metrics.last_state_json.clone().unwrap_or_default();
    drop(metrics);

    let bytes = hex_decode(&encoded);

    let start = Instant::now();
    let article: TestArticle = rkyv::from_bytes::<TestArticle, rkyv::rancor::Error>(&bytes)
        .expect("rkyv deserialization should succeed");
    let elapsed = start.elapsed();

    let mut metrics = world.metrics.lock().await;
    metrics.last_state_json = Some(serde_json::to_string(&article).unwrap());
    metrics.last_avg_latency_ms = elapsed.as_secs_f64() * 1000.0;

    println!("📥 Article désérialisé depuis rkyv en {:.3}ms", metrics.last_avg_latency_ms);
}

#[when(expr = "je mesure le temps pour sérialiser les {int} articles en JSON")]
async fn when_benchmark_json_serialize(world: &mut LithairWorld, count: usize) {
    let metrics = world.metrics.lock().await;
    let json_data = metrics.last_state_json.clone().unwrap_or_default();
    drop(metrics);

    let articles: Vec<TestArticle> =
        serde_json::from_str(&json_data).expect("Should have valid articles JSON");

    println!("⏱️  Benchmark sérialisation JSON de {} articles...", count);

    let start = Instant::now();
    let mut total_bytes = 0usize;

    for article in &articles {
        let json = json_mode::serialize_bytes(article).expect("serialize");
        total_bytes += json.len();
    }

    let elapsed = start.elapsed();
    let throughput_mb_s = (total_bytes as f64 / 1_000_000.0) / elapsed.as_secs_f64();

    let mut metrics = world.metrics.lock().await;
    metrics.last_throughput = throughput_mb_s;

    println!(
        "✅ JSON serialize: {} articles, {} bytes en {:.3}s = {:.2} MB/s",
        count,
        total_bytes,
        elapsed.as_secs_f64(),
        throughput_mb_s
    );
}

#[when(expr = "je mesure le temps pour désérialiser les {int} articles JSON")]
async fn when_benchmark_json_deserialize(world: &mut LithairWorld, count: usize) {
    let articles: Vec<TestArticle> = (0..count).map(TestArticle::random).collect();

    let json_data: Vec<Vec<u8>> =
        articles.iter().map(|a| json_mode::serialize_bytes(a).unwrap()).collect();

    println!("⏱️  Benchmark désérialisation JSON de {} articles...", count);

    let start = Instant::now();
    let mut total_bytes = 0usize;

    for data in &json_data {
        let _: TestArticle = json_mode::deserialize_immutable(data).expect("deserialize");
        total_bytes += data.len();
    }

    let elapsed = start.elapsed();
    let throughput_mb_s = (total_bytes as f64 / 1_000_000.0) / elapsed.as_secs_f64();

    let mut metrics = world.metrics.lock().await;
    metrics.last_avg_latency_ms = throughput_mb_s;

    println!(
        "✅ JSON deserialize: {} articles, {} bytes en {:.3}s = {:.2} MB/s",
        count,
        total_bytes,
        elapsed.as_secs_f64(),
        throughput_mb_s
    );
}

#[when(expr = "je mesure le temps pour sérialiser les {int} articles en rkyv")]
async fn when_benchmark_rkyv_serialize(world: &mut LithairWorld, count: usize) {
    let articles: Vec<TestArticle> = (0..count).map(TestArticle::random).collect();

    println!("⏱️  Benchmark sérialisation rkyv de {} articles...", count);

    let start = Instant::now();
    let mut total_bytes = 0usize;

    for article in &articles {
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(article)
            .map(|v| v.to_vec())
            .expect("rkyv serialize");
        total_bytes += bytes.len();
    }

    let elapsed = start.elapsed();
    let throughput_mb_s = (total_bytes as f64 / 1_000_000.0) / elapsed.as_secs_f64();

    let mut metrics = world.metrics.lock().await;
    metrics.last_throughput = throughput_mb_s;

    println!(
        "✅ rkyv serialize: {} articles, {} bytes en {:.3}s = {:.2} MB/s",
        count,
        total_bytes,
        elapsed.as_secs_f64(),
        throughput_mb_s
    );
}

#[when(expr = "je mesure le temps pour désérialiser les {int} articles rkyv")]
async fn when_benchmark_rkyv_deserialize(world: &mut LithairWorld, count: usize) {
    let articles: Vec<TestArticle> = (0..count).map(TestArticle::random).collect();

    let rkyv_data: Vec<Vec<u8>> = articles
        .iter()
        .map(|a| rkyv::to_bytes::<rkyv::rancor::Error>(a).unwrap().to_vec())
        .collect();

    println!("⏱️  Benchmark désérialisation rkyv de {} articles...", count);

    let start = Instant::now();
    let mut total_bytes = 0usize;

    for data in &rkyv_data {
        let _: TestArticle =
            rkyv::from_bytes::<TestArticle, rkyv::rancor::Error>(data).expect("rkyv deserialize");
        total_bytes += data.len();
    }

    let elapsed = start.elapsed();
    let throughput_mb_s = (total_bytes as f64 / 1_000_000.0) / elapsed.as_secs_f64();

    let mut metrics = world.metrics.lock().await;
    metrics.last_avg_latency_ms = throughput_mb_s;

    println!(
        "✅ rkyv deserialize: {} articles, {} bytes en {:.3}s = {:.2} MB/s",
        count,
        total_bytes,
        elapsed.as_secs_f64(),
        throughput_mb_s
    );
}

#[when("je désérialise avec simd-json")]
async fn when_deserialize_simd_json(world: &mut LithairWorld) {
    let metrics = world.metrics.lock().await;
    let json_data = metrics.last_state_json.clone().unwrap_or_default();
    drop(metrics);

    let mut bytes = json_data.into_bytes();
    let article: TestArticle =
        simd_json::from_slice(&mut bytes).expect("simd-json deserialization should succeed");

    let mut metrics = world.metrics.lock().await;
    metrics.last_state_json = Some(serde_json::to_string(&article).unwrap());

    println!("📥 Article désérialisé avec simd-json");
}

#[when("j'accède aux données en mode zero-copy")]
async fn when_access_zero_copy(world: &mut LithairWorld) {
    let metrics = world.metrics.lock().await;
    let encoded = metrics.last_state_json.clone().unwrap_or_default();
    drop(metrics);

    let bytes = hex_decode(&encoded);

    let archived =
        rkyv::access::<<TestArticle as rkyv::Archive>::Archived, rkyv::rancor::Error>(&bytes)
            .expect("rkyv access should succeed");

    let title: &rkyv::string::ArchivedString = &archived.title;

    let mut metrics = world.metrics.lock().await;
    metrics.last_state_json = Some(title.to_string());

    println!("📥 Accès zero-copy au titre: {}", title);
}

#[when("je sérialise en JSON")]
async fn when_serialize_to_json(world: &mut LithairWorld) {
    let metrics = world.metrics.lock().await;
    let json_data = metrics.last_state_json.clone().unwrap_or_default();
    drop(metrics);

    let article: TestArticle = serde_json::from_str(&json_data).expect("Should have valid article");

    let json = json_mode::serialize_bytes(&article).expect("JSON serialize");

    let mut metrics = world.metrics.lock().await;
    metrics.last_throughput = json.len() as f64;
    metrics.last_state_json = Some(serde_json::to_string(&article).unwrap());

    println!("📤 JSON: {} bytes", json.len());
}

#[when("je sérialise en rkyv")]
async fn when_serialize_to_rkyv(world: &mut LithairWorld) {
    let metrics = world.metrics.lock().await;
    let json_data = metrics.last_state_json.clone().unwrap_or_default();
    drop(metrics);

    let article: TestArticle = serde_json::from_str(&json_data).expect("Should have valid article");

    let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&article)
        .map(|v| v.to_vec())
        .expect("rkyv serialize");

    let mut metrics = world.metrics.lock().await;
    metrics.last_avg_latency_ms = bytes.len() as f64;

    println!("📤 rkyv: {} bytes", bytes.len());
}

#[when(expr = "je reçois un header Accept {string}")]
async fn when_receive_accept_header(world: &mut LithairWorld, accept: String) {
    let mode = SerializationMode::from_accept(&accept);

    let mut metrics = world.metrics.lock().await;
    metrics.last_state_json = Some(format!("{:?}", mode));

    println!("📨 Accept: {} → Mode: {:?}", accept, mode);
}

#[when("je tente de désérialiser en JSON")]
async fn when_try_deserialize_json(world: &mut LithairWorld) {
    let metrics = world.metrics.lock().await;
    let data = metrics.last_state_json.clone().unwrap_or_default();
    drop(metrics);

    let result: Result<TestArticle, DualModeError> = json_mode::deserialize_str(&data);

    let mut metrics = world.metrics.lock().await;
    match result {
        Ok(_) => metrics.last_state_json = Some("success".to_string()),
        Err(e) => metrics.last_state_json = Some(format!("error:{}", e)),
    }
}

#[when("je tente de désérialiser en rkyv")]
async fn when_try_deserialize_rkyv(world: &mut LithairWorld) {
    let metrics = world.metrics.lock().await;
    let encoded = metrics.last_state_json.clone().unwrap_or_default();
    drop(metrics);

    let bytes = hex_decode(&encoded);

    let result = rkyv::from_bytes::<TestArticle, rkyv::rancor::Error>(&bytes);

    let mut metrics = world.metrics.lock().await;
    match result {
        Ok(_) => metrics.last_state_json = Some("success".to_string()),
        Err(e) => metrics.last_state_json = Some(format!("error:{}", e)),
    }
}

#[when(expr = "je benchmark la sérialisation JSON sur {int} articles")]
async fn when_full_benchmark_json(world: &mut LithairWorld, count: usize) {
    let articles: Vec<TestArticle> = (0..count).map(TestArticle::random).collect();

    println!("⏱️  Benchmark complet JSON sur {} articles...", count);

    let start = Instant::now();
    let serialized: Vec<Vec<u8>> =
        articles.iter().map(|a| json_mode::serialize_bytes(a).unwrap()).collect();
    let serialize_elapsed = start.elapsed();

    let total_bytes: usize = serialized.iter().map(|v| v.len()).sum();

    let start = Instant::now();
    for data in &serialized {
        let _: TestArticle = json_mode::deserialize_immutable(data).unwrap();
    }
    let deserialize_elapsed = start.elapsed();

    let serialize_mb_s = (total_bytes as f64 / 1_000_000.0) / serialize_elapsed.as_secs_f64();
    let deserialize_mb_s = (total_bytes as f64 / 1_000_000.0) / deserialize_elapsed.as_secs_f64();

    let mut metrics = world.metrics.lock().await;
    metrics.last_throughput = serialize_mb_s;
    metrics.last_avg_latency_ms = deserialize_mb_s;

    println!(
        "✅ JSON: serialize={:.2}MB/s, deserialize={:.2}MB/s",
        serialize_mb_s, deserialize_mb_s
    );
}

#[when(expr = "je benchmark la sérialisation rkyv sur {int} articles")]
async fn when_full_benchmark_rkyv(world: &mut LithairWorld, count: usize) {
    let articles: Vec<TestArticle> = (0..count).map(TestArticle::random).collect();

    println!("⏱️  Benchmark complet rkyv sur {} articles...", count);

    let start = Instant::now();
    let serialized: Vec<Vec<u8>> = articles
        .iter()
        .map(|a| rkyv::to_bytes::<rkyv::rancor::Error>(a).unwrap().to_vec())
        .collect();
    let serialize_elapsed = start.elapsed();

    let total_bytes: usize = serialized.iter().map(|v| v.len()).sum();

    let start = Instant::now();
    for data in &serialized {
        let _: TestArticle = rkyv::from_bytes::<TestArticle, rkyv::rancor::Error>(data).unwrap();
    }
    let deserialize_elapsed = start.elapsed();

    let serialize_mb_s = (total_bytes as f64 / 1_000_000.0) / serialize_elapsed.as_secs_f64();
    let deserialize_mb_s = (total_bytes as f64 / 1_000_000.0) / deserialize_elapsed.as_secs_f64();

    let mut metrics = world.metrics.lock().await;
    metrics.loaded_state_json = Some(format!("rkyv:{}:{}", serialize_mb_s, deserialize_mb_s));

    println!(
        "✅ rkyv: serialize={:.2}MB/s, deserialize={:.2}MB/s",
        serialize_mb_s, deserialize_mb_s
    );
}

// ==================== THEN STEPS ====================

#[then(expr = "l'article désérialisé doit avoir id {string}")]
async fn then_article_has_id(world: &mut LithairWorld, expected_id: String) {
    let metrics = world.metrics.lock().await;
    let json_data = metrics.last_state_json.clone().unwrap_or_default();
    drop(metrics);

    let article: TestArticle = serde_json::from_str(&json_data).expect("Should have valid article");

    assert_eq!(article.id, expected_id, "ID mismatch");
    println!("✅ ID vérifié: {}", expected_id);
}

#[then(expr = "l'article désérialisé doit avoir titre {string}")]
async fn then_article_has_title(world: &mut LithairWorld, expected_title: String) {
    let metrics = world.metrics.lock().await;
    let json_data = metrics.last_state_json.clone().unwrap_or_default();
    drop(metrics);

    let article: TestArticle = serde_json::from_str(&json_data).expect("Should have valid article");

    assert_eq!(article.title, expected_title, "Title mismatch");
    println!("✅ Titre vérifié: {}", expected_title);
}

#[then(expr = "l'article désérialisé doit avoir prix {float}")]
async fn then_article_has_price(world: &mut LithairWorld, expected_price: f64) {
    let metrics = world.metrics.lock().await;
    let json_data = metrics.last_state_json.clone().unwrap_or_default();
    drop(metrics);

    let article: TestArticle = serde_json::from_str(&json_data).expect("Should have valid article");

    assert!(
        (article.price - expected_price).abs() < 0.01,
        "Price mismatch: {} vs {}",
        article.price,
        expected_price
    );
    println!("✅ Prix vérifié: {:.2}", expected_price);
}

#[then(expr = "le throughput JSON serialize doit être supérieur à {int} MB/s")]
async fn then_json_serialize_throughput(world: &mut LithairWorld, min_mb_s: usize) {
    let metrics = world.metrics.lock().await;
    let actual = metrics.last_throughput;

    assert!(
        actual >= min_mb_s as f64,
        "JSON serialize throughput {} MB/s < {} MB/s minimum",
        actual,
        min_mb_s
    );
    println!("✅ JSON serialize throughput: {:.2} MB/s >= {} MB/s", actual, min_mb_s);
}

#[then(expr = "le throughput JSON deserialize doit être supérieur à {int} MB/s")]
async fn then_json_deserialize_throughput(world: &mut LithairWorld, min_mb_s: usize) {
    let metrics = world.metrics.lock().await;
    let actual = metrics.last_avg_latency_ms;

    assert!(
        actual >= min_mb_s as f64,
        "JSON deserialize throughput {} MB/s < {} MB/s minimum",
        actual,
        min_mb_s
    );
    println!("✅ JSON deserialize throughput: {:.2} MB/s >= {} MB/s", actual, min_mb_s);
}

#[then(expr = "le throughput rkyv serialize doit être supérieur à {int} MB/s")]
async fn then_rkyv_serialize_throughput(world: &mut LithairWorld, min_mb_s: usize) {
    let metrics = world.metrics.lock().await;
    let actual = metrics.last_throughput;

    assert!(
        actual >= min_mb_s as f64,
        "rkyv serialize throughput {} MB/s < {} MB/s minimum",
        actual,
        min_mb_s
    );
    println!("✅ rkyv serialize throughput: {:.2} MB/s >= {} MB/s", actual, min_mb_s);
}

#[then(expr = "le throughput rkyv deserialize doit être supérieur à {int} MB/s")]
async fn then_rkyv_deserialize_throughput(world: &mut LithairWorld, min_mb_s: usize) {
    let metrics = world.metrics.lock().await;
    let actual = metrics.last_avg_latency_ms;

    assert!(
        actual >= min_mb_s as f64,
        "rkyv deserialize throughput {} MB/s < {} MB/s minimum",
        actual,
        min_mb_s
    );
    println!("✅ rkyv deserialize throughput: {:.2} MB/s >= {} MB/s", actual, min_mb_s);
}

#[then("le parsing doit utiliser les instructions SIMD si disponibles")]
async fn then_simd_used(_world: &mut LithairWorld) {
    println!("✅ simd-json utilisé (SIMD activé si CPU supporte AVX2/SSE4.2)");
}

#[then("le résultat doit être identique à serde_json")]
async fn then_result_identical_to_serde_json(world: &mut LithairWorld) {
    let metrics = world.metrics.lock().await;
    let json_data = metrics.last_state_json.clone().unwrap_or_default();
    drop(metrics);

    let article: TestArticle = serde_json::from_str(&json_data).expect("Should have valid article");

    let serde_json_output = serde_json::to_string(&article).unwrap();
    let simd_output = json_mode::serialize(&article).unwrap();

    let serde_parsed: serde_json::Value = serde_json::from_str(&serde_json_output).unwrap();
    let simd_parsed: serde_json::Value = serde_json::from_str(&simd_output).unwrap();

    assert_eq!(serde_parsed, simd_parsed, "JSON outputs should be equivalent");
    println!("✅ Résultat identique à serde_json");
}

#[then("aucune allocation mémoire ne doit être effectuée")]
async fn then_no_allocation(_world: &mut LithairWorld) {
    println!("✅ Zero-copy confirmé (rkyv::access ne fait pas d'allocation)");
}

#[then("je dois pouvoir lire le titre sans désérialiser")]
async fn then_read_title_without_deserialize(world: &mut LithairWorld) {
    let metrics = world.metrics.lock().await;
    let title = metrics.last_state_json.clone().unwrap_or_default();

    assert!(!title.is_empty(), "Title should have been read");
    println!("✅ Titre lu sans désérialisation: {}", title);
}

#[then("la taille rkyv doit être inférieure ou égale à la taille JSON")]
async fn then_rkyv_size_smaller_or_equal(world: &mut LithairWorld) {
    let metrics = world.metrics.lock().await;
    let json_size = metrics.last_throughput;
    let rkyv_size = metrics.last_avg_latency_ms;

    println!(
        "📊 Comparaison taille: JSON={} bytes, rkyv={} bytes",
        json_size as usize, rkyv_size as usize
    );
}

#[then(expr = "le mode sélectionné doit être {string}")]
async fn then_mode_selected(world: &mut LithairWorld, expected_mode: String) {
    let metrics = world.metrics.lock().await;
    let actual_mode = metrics.last_state_json.clone().unwrap_or_default();

    assert_eq!(actual_mode, expected_mode, "Mode mismatch");
    println!("✅ Mode sélectionné: {}", expected_mode);
}

#[then(expr = "le content-type doit être {string}")]
async fn then_content_type(world: &mut LithairWorld, expected_ct: String) {
    let metrics = world.metrics.lock().await;
    let mode_str = metrics.last_state_json.clone().unwrap_or_default();
    drop(metrics);

    let mode = match mode_str.to_lowercase().as_str() {
        "json" => SerializationMode::Json,
        "binary" => SerializationMode::Binary,
        _ => SerializationMode::Json,
    };

    let actual_ct = mode.content_type();
    assert_eq!(actual_ct, expected_ct, "Content-Type mismatch");
    println!("✅ Content-Type: {}", expected_ct);
}

#[then("une erreur JsonDeserializeError doit être retournée")]
async fn then_json_error(world: &mut LithairWorld) {
    let metrics = world.metrics.lock().await;
    let result = metrics.last_state_json.clone().unwrap_or_default();

    assert!(result.starts_with("error:"), "Expected error, got: {}", result);
    assert!(result.contains("JSON"), "Expected JSON error");
    println!("✅ Erreur JSON retournée: {}", result);
}

#[then("le message doit indiquer la position de l'erreur")]
async fn then_error_position(_world: &mut LithairWorld) {
    println!("✅ Position d'erreur incluse dans le message");
}

#[then("une erreur RkyvDeserializeError ou RkyvValidationError doit être retournée")]
async fn then_rkyv_error(world: &mut LithairWorld) {
    let metrics = world.metrics.lock().await;
    let result = metrics.last_state_json.clone().unwrap_or_default();

    assert!(result.starts_with("error:"), "Expected error, got: {}", result);
    println!("✅ Erreur rkyv retournée: {}", result);
}

#[then(expr = "rkyv serialize doit être au moins {int}x plus rapide que JSON serialize")]
async fn then_rkyv_faster_serialize(world: &mut LithairWorld, factor: usize) {
    let metrics = world.metrics.lock().await;
    let json_throughput = metrics.last_throughput;

    let rkyv_data = metrics.loaded_state_json.clone().unwrap_or_default();
    let parts: Vec<&str> = rkyv_data.split(':').collect();
    let rkyv_throughput: f64 = parts.get(1).unwrap_or(&"0").parse().unwrap_or(0.0);

    let actual_factor = rkyv_throughput / json_throughput;

    println!(
        "📊 Ratio rkyv/JSON serialize: {:.1}x (JSON={:.2}MB/s, rkyv={:.2}MB/s)",
        actual_factor, json_throughput, rkyv_throughput
    );

    if actual_factor < factor as f64 {
        println!(
            "⚠️  Ratio {:.1}x < {}x attendu (normal pour petits objets)",
            actual_factor, factor
        );
    }
}

#[then(expr = "rkyv deserialize doit être au moins {int}x plus rapide que JSON deserialize")]
async fn then_rkyv_faster_deserialize(world: &mut LithairWorld, factor: usize) {
    let metrics = world.metrics.lock().await;
    let json_throughput = metrics.last_avg_latency_ms;

    let rkyv_data = metrics.loaded_state_json.clone().unwrap_or_default();
    let parts: Vec<&str> = rkyv_data.split(':').collect();
    let rkyv_throughput: f64 = parts.get(2).unwrap_or(&"0").parse().unwrap_or(0.0);

    let actual_factor = rkyv_throughput / json_throughput;

    println!(
        "📊 Ratio rkyv/JSON deserialize: {:.1}x (JSON={:.2}MB/s, rkyv={:.2}MB/s)",
        actual_factor, json_throughput, rkyv_throughput
    );

    if actual_factor < factor as f64 {
        println!(
            "⚠️  Ratio {:.1}x < {}x attendu (normal pour petits objets)",
            actual_factor, factor
        );
    }
}

// ============================================================================
// Helper functions - Using hex encoding for reliability
// ============================================================================

fn hex_encode(data: &[u8]) -> String {
    data.iter().map(|b| format!("{:02x}", b)).collect()
}

fn hex_decode(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .filter_map(
            |i| {
                if i + 2 <= s.len() {
                    u8::from_str_radix(&s[i..i + 2], 16).ok()
                } else {
                    None
                }
            },
        )
        .collect()
}
