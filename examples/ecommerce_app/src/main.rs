use lithair_core::{
    engine::{RaftstoneApplication, AutoJoiner, RelationRegistry, DataSource},
    http::{CommandMessage, CommandRoute, HttpMethod, HttpResponse, Route},
    Lithair,
};
use lithair_core::model::{FieldPolicy, ModelSpec};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock}; // Using OnceLock for global registry access in example
use uuid::Uuid;

// --- 0. Declarative Model Specs (The "Schema") ---

struct ProductModelSpec;
impl ModelSpec for ProductModelSpec {
    fn get_policy(&self, field_name: &str) -> Option<FieldPolicy> {
        match field_name {
            "name" => Some(FieldPolicy {
                unique: true,
                indexed: true,
                ..Default::default()
            }),
            "price" => Some(FieldPolicy {
                indexed: true,
                ..Default::default()
            }),
            "category_id" => Some(FieldPolicy {
                fk: true,
                fk_collection: Some("categories".to_string()), // ✨ Link to Category
                indexed: true,
                ..Default::default()
            }),
            _ => None,
        }
    }

    fn get_all_fields(&self) -> Vec<String> {
        vec![
            "id".to_string(),
            "name".to_string(),
            "price".to_string(),
            "stock".to_string(),
            "category_id".to_string(),
        ]
    }
}

static PRODUCT_SPEC: ProductModelSpec = ProductModelSpec;

// --- 1. Domain Models (The "Tables") ---

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct Category {
    id: String,
    name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct Product {
    id: String,
    name: String,
    price: f64,
    stock: i32,
    category_id: Option<String>, // New field for FK
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct Order {
    id: String,
    product_id: String,
    quantity: i32,
    status: String,
}

// --- 2. Domain Events (The "Changes") ---

#[derive(Debug, Clone, Serialize, Deserialize)]
enum CategoryEvent {
    Created { id: String, name: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum ProductEvent {
    Created { id: String, name: String, price: f64, stock: i32, category_id: Option<String> },
    StockUpdated { id: String, new_stock: i32 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum OrderEvent {
    Placed { id: String, product_id: String, quantity: i32 },
    StatusChanged { id: String, new_status: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum AppEvent {
    Category(CategoryEvent),
    Product(ProductEvent),
    Order(OrderEvent),
    OrderPlacedWithStockReduction {
        order_id: String,
        product_id: String,
        quantity: i32,
        new_stock: i32,
    },
}

impl lithair_core::engine::Event for AppEvent {
    type State = AppState;

    fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| String::from("{}"))
    }

    fn from_json(json: &str) -> lithair_core::engine::EngineResult<Self> {
        serde_json::from_str(json).map_err(|e| {
            lithair_core::engine::EngineError::SerializationError(e.to_string())
        })
    }

    fn apply(&self, state: &mut Self::State) {
        match self {
            AppEvent::Category(e) => apply_category_event(state, e),
            AppEvent::Product(e) => apply_product_event(state, e),
            AppEvent::Order(e) => apply_order_event(state, e),
            AppEvent::OrderPlacedWithStockReduction { order_id, product_id, quantity, new_stock } => {
                apply_order_event(state, &OrderEvent::Placed {
                    id: order_id.clone(),
                    product_id: product_id.clone(),
                    quantity: *quantity,
                });
                apply_product_event(state, &ProductEvent::StockUpdated {
                    id: product_id.clone(),
                    new_stock: *new_stock,
                });
            }
        }
    }
}

fn apply_category_event(state: &mut AppState, event: &CategoryEvent) {
    match event {
        CategoryEvent::Created { id, name } => {
            state.categories.insert(id.clone(), Category { id: id.clone(), name: name.clone() });
        }
    }
}

fn apply_product_event(state: &mut AppState, event: &ProductEvent) {
    match event {
        ProductEvent::Created { id, name, price, stock, category_id } => {
            state.products.insert(id.clone(), Product {
                id: id.clone(),
                name: name.clone(),
                price: *price,
                stock: *stock,
                category_id: category_id.clone(),
            });
        }
        ProductEvent::StockUpdated { id, new_stock } => {
            if let Some(p) = state.products.get_mut(id) {
                p.stock = *new_stock;
            }
        }
    }
}

fn apply_order_event(state: &mut AppState, event: &OrderEvent) {
    match event {
        OrderEvent::Placed { id, product_id, quantity } => {
            state.orders.insert(id.clone(), Order {
                id: id.clone(),
                product_id: product_id.clone(),
                quantity: *quantity,
                status: "Pending".to_string(),
            });
        }
        OrderEvent::StatusChanged { id, new_status } => {
            if let Some(o) = state.orders.get_mut(id) {
                o.status = new_status.clone();
            }
        }
    }
}

// --- 3. Application State & Data Source ---

#[derive(Default, Clone, Serialize, Deserialize)]
struct AppState {
    categories: HashMap<String, Category>,
    products: HashMap<String, Product>,
    orders: HashMap<String, Order>,
}

impl lithair_core::model_inspect::Inspectable for AppState {
    fn get_field_value(&self, field_name: &str) -> Option<Value> {
        match field_name {
            "categories" => serde_json::to_value(&self.categories).ok(),
            "products" => serde_json::to_value(&self.products).ok(),
            "orders" => serde_json::to_value(&self.orders).ok(),
            _ => None,
        }
    }
}

impl ModelSpec for AppState {
    fn get_policy(&self, _field_name: &str) -> Option<FieldPolicy> {
        None
    }
    fn get_all_fields(&self) -> Vec<String> {
        vec![]
    }
}

// Custom DataSource to expose Categories from AppState (Simulating a separate table)
// Note: In a real app, we might use Scc2Engine which implements DataSource automatically.
// Here we use a small adapter for the HashMap.
struct CategoryDataSource {
    // We use a Weak or Shared reference to state?
    // For simplicity in this example, we'll just query the global state registry
    // (which isn't ideal for clean code but works for the demo).
    //
    // Better: Store a copy of the data or access it via a shared Arc.
    // Let's simulate it by passing the state to the expander directly,
    // or implementing DataSource on a wrapper around the HashMap.
    categories: Arc<Mutex<HashMap<String, Category>>>,
}

impl DataSource for CategoryDataSource {
    fn fetch_by_id(&self, id: &str) -> Option<Value> {
        self.categories.lock().unwrap().get(id).map(|c| serde_json::to_value(c).unwrap())
    }
}

// Global Registry for the example (to be accessible in routes)
static REGISTRY: OnceLock<RelationRegistry> = OnceLock::new();

// --- 4. Lithair Integration ---

#[derive(Default)]
struct EcommerceApp {
    // We hold a shared reference to categories to update the DataSource
    category_store: Arc<Mutex<HashMap<String, Category>>>,
}

impl RaftstoneApplication for EcommerceApp {
    type State = AppState;
    type Event = AppEvent;
    type Command = ();

    fn initial_state() -> Self::State {
        AppState::default()
    }

    fn event_deserializers() -> Vec<Box<dyn lithair_core::engine::EventDeserializer<State = Self::State>>> {
        vec![]
    }

    fn on_startup(state: &mut Self::State) -> anyhow::Result<()> {
        println!("🛒 Ecommerce App Startup");

        // Initialize Registry and Data Sources
        let registry = RelationRegistry::new();

        // Create a shared store for categories populated from initial state
        let category_store = Arc::new(Mutex::new(state.categories.clone()));

        // Register "categories" data source
        registry.register("categories", Arc::new(CategoryDataSource {
            categories: category_store.clone(),
        }));

        let _ = REGISTRY.set(registry);

        println!("   - Products: {}", state.products.len());
        println!("   - Categories: {}", state.categories.len());

        Ok(())
    }

    fn routes() -> Vec<Route<Self::State>> {
        vec![
            // Products with Auto-Join!
            Route::new(HttpMethod::GET, "/api/products", |_req, params, state: &AppState| {
                let registry = REGISTRY.get().unwrap();

                // 1. Update the data source with latest state (Sync hack for HashMap example)
                // In Scc2Engine, this is automatic as it reads from live memory.
                // Here we need to ensure our DataSource has the latest data.
                if let Some(ds) = registry.get("categories") {
                     // This cast is ugly but specific to this "HashMap Adapter" demo
                     // Real implementation with Scc2Engine avoids this.
                     // We'll skip the update for now and rely on the fact we are read-mostly.
                }

                // ⚠️ CRITICAL: For this demo to work with HashMap state,
                // we need to construct a fresh DataSource using the *current* state passed to the route.
                let local_registry = RelationRegistry::new();
                local_registry.register("categories", Arc::new(CategoryDataSource {
                    categories: Arc::new(Mutex::new(state.categories.clone())),
                }));

                let products: Vec<&Product> = state.products.values().collect();

                // ✨ MAGIC HAPPENS HERE: Auto-Join based on ModelSpec
                let json = AutoJoiner::expand_list(&products, &PRODUCT_SPEC, &local_registry).unwrap();

                HttpResponse::ok().json(&json.to_string())
            }),

            Route::new(HttpMethod::GET, "/api/categories", |_req, _, state: &AppState| {
                HttpResponse::ok().json(&serde_json::to_string(&state.categories).unwrap())
            }),
        ]
    }

    fn command_routes() -> Vec<CommandRoute<Self>> {
        vec![
            // Create Category
            CommandRoute::new(HttpMethod::POST, "/api/categories", |req, _, sender| {
                let payload: serde_json::Value = serde_json::from_str(req.body_string().unwrap_or("")).unwrap();
                let id = Uuid::new_v4().to_string();
                let name = payload["name"].as_str().unwrap().to_string();

                let event = AppEvent::Category(CategoryEvent::Created { id: id.clone(), name });

                let (tx, rx) = std::sync::mpsc::channel();
                sender.lock().unwrap().send(CommandMessage { event, response_sender: tx }).unwrap();
                let _ = rx.recv().unwrap();

                HttpResponse::created().json(&serde_json::json!({ "id": id }).to_string())
            }),

            // Create Product
            CommandRoute::new(HttpMethod::POST, "/api/products", |req, _, sender| {
                let payload: serde_json::Value = serde_json::from_str(req.body_string().unwrap_or("")).unwrap();
                let id = Uuid::new_v4().to_string();
                let name = payload["name"].as_str().unwrap().to_string();
                let price = payload["price"].as_f64().unwrap();
                let stock = payload["stock"].as_i64().unwrap() as i32;
                let category_id = payload.get("category_id").and_then(|v| v.as_str()).map(|s| s.to_string());

                let event = AppEvent::Product(ProductEvent::Created {
                    id: id.clone(),
                    name,
                    price,
                    stock,
                    category_id,
                });

                let (tx, rx) = std::sync::mpsc::channel();
                sender.lock().unwrap().send(CommandMessage { event, response_sender: tx }).unwrap();
                let _ = rx.recv().unwrap();

                HttpResponse::created().json(&serde_json::json!({ "id": id }).to_string())
            }),
        ]
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let app = EcommerceApp::default();

    // Configure Engine
    std::env::set_var("RS_STORAGE_FORMAT", "Binary");
    std::env::set_var("EXPERIMENT_DATA_BASE", "data/ecommerce_v2");

    println!("🚀 Starting Ecommerce App with Auto-Joiner");
    println!("   - Product Model Spec defines 'category_id' -> 'categories'");

    let framework = Lithair::new(app);
    framework.run("127.0.0.1:8081")?;

    Ok(())
}
