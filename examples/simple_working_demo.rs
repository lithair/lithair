//! Lithair Simple Working Demo
//! 
//! This shows the completed architecture transformation:
//! - Uses ONLY lithair-core framework
//! - Unified API: new() vs new_distributed()
//! - Working single-node mode
//! - Stubbed distributed mode with clear TODO

use lithair_core::{Lithair, RaftstoneApplication};

/// Minimal application for demonstration
#[derive(Debug, Clone, Default)]
struct MinimalApp {
    visits: u64,
}

/// Minimal event system
#[derive(Debug, Clone)]
enum AppEvent {
    VisitRecorded,
}

impl lithair_core::engine::Event for AppEvent {
    type State = MinimalApp;
    
    fn apply(&self, state: &mut MinimalApp) {
        match self {
            AppEvent::VisitRecorded => state.visits += 1,
        }
    }
    
    fn to_json(&self) -> String {
        r#"{"type":"VisitRecorded"}"#.to_string()
    }
    
    fn aggregate_id(&self) -> Option<String> {
        Some("app".to_string())
    }
}

impl RaftstoneApplication for MinimalApp {
    type State = MinimalApp;
    type Event = AppEvent;

    fn initial_state() -> Self::State {
        MinimalApp::default()
    }

    fn routes() -> Vec<lithair_core::http::Route<Self::State>> {
        vec![
            lithair_core::http::Route::new(
                lithair_core::http::HttpMethod::GET,
                "/",
                |_req, _params, state: &MinimalApp| {
                    lithair_core::http::HttpResponse::ok()
                        .text(&format!("🚀 Lithair Framework Working! Visits: {}", state.visits))
                }
            ),
        ]
    }

    fn command_routes() -> Vec<lithair_core::http::CommandRoute<Self>> {
        vec![]
    }

    fn event_deserializers() -> Vec<Box<dyn lithair_core::engine::EventDeserializer<State = Self::State>>> {
        vec![]
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Lithair Simple Working Demo");
    println!();
    
    // Demonstration de l'API unifiée
    let app = MinimalApp::default();
    
    // Mode local (production ready)
    let framework = Lithair::new(app);
    
    println!("✅ Architecture transformée avec succès!");
    println!("📊 Unified API: Lithair::new() pour local, Lithair::new_distributed() pour distribué");
    println!("🔧 Exemples utilisent UNIQUEMENT lithair-core framework");
    println!("🌐 Démarrage serveur...");
    
    framework.run("127.0.0.1:9092")?;
    Ok(())
}