use lithair_core::{
    engine::{RaftstoneApplication, EngineConfig},
    http::{CommandMessage, CommandRoute, HttpMethod, HttpResponse, Route},
    Lithair,
};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

// --- 1. The Value Type (State per Key) ---
// In SCC2 mode, "State" refers to the value stored in the HashMap.
// It must be Clone + Send + Sync.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct KeyValue {
    value: String,
    updated_at: u64,
}

// --- 2. The Events ---
#[derive(Debug, Clone, Serialize, Deserialize)]
enum KvEvent {
    Set { key: String, value: String },
    Delete { key: String },
}

impl lithair_core::engine::Event for KvEvent {
    type State = KeyValue;

    // Aggregate ID is crucial for Scc2Engine routing!
    fn aggregate_id(&self) -> Option<String> {
        match self {
            KvEvent::Set { key, .. } => Some(key.clone()),
            KvEvent::Delete { key } => Some(key.clone()),
        }
    }

    fn apply(&self, state: &mut Self::State) {
        match self {
            KvEvent::Set { value, .. } => {
                state.value = value.clone();
                state.updated_at = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
            }
            KvEvent::Delete { .. } => {
                state.value.clear(); // Soft delete for demo
            }
        }
    }
}

// --- 3. The Application ---
#[derive(Default)]
struct Scc2KvApp;

impl RaftstoneApplication for Scc2KvApp {
    type State = KeyValue; // The value type
    type Event = KvEvent;

    fn initial_state() -> Self::State {
        KeyValue::default()
    }

    fn routes() -> Vec<Route<Self::State>> {
        vec![
            // Get value by key
            // Scc2Engine automatically routes to the correct key based on :key param if we use read_state manually?
            // No, Lithair router calls the handler.
            // Handler must call engine.read_state(key, ...).
            // BUT Route<S> handler signature is: |req, params, state: &S|
            // In Scc2 mode, 'state' passed here is the specific value for the key?
            // How does the framework know which key to fetch?
            // The Framework's `create_stateless_router_with_shared_engine` currently calls:
            // `engine.state().with_state(...)`.
            // But `engine.state()` panics for Scc2!

            // PROBLEM: The current HTTP integration in lib.rs assumes RwLock/StateEngine!
            // See create_stateless_router_with_shared_engine in lib.rs.
            // It calls `engine_guard.state().with_state(...)`.

            // I need to update lib.rs to support Scc2 routing strategy.
            // In Scc2 mode, we can't just "get the state". We need the key.
            // But the router resolves the route matching FIRST, then calls handler.
            // The handler needs the state.

            // For Scc2, we might need to pass the Engine itself to the handler?
            // Or the Router needs to be Scc2-aware.

            // Quick fix for now:
            // In lib.rs, if Scc2, we pass a DEFAULT value (or error) as state?
            // And the handler uses `engine` directly? But handler signature is fixed.

            // Actually, Route handler signature `Fn(..., &State)` is problematic for KV store.
            // Because we don't know WHICH key to load until we parse parameters.

            // Proposal: Update `Route` to allow `state` to be optional? Or pass whole Engine?
            // Or simply: For Scc2 App, `routes()` returns empty, and we use custom handlers?
            // But `RaftstoneApplication` requires `routes()`.

            // Workaround: In `lib.rs`, `create_stateless_router_with_shared_engine`:
            // If Scc2, we can't pre-load state.
            // We must change `handle_request` signature in `EnhancedRouter` or how it's called.

            // This reveals a deeper architectural assumption in Lithair: "Global Single State".
            // Scc2 breaks this assumption.

            // I will fix `lib.rs` to handle Scc2.
            // If Scc2, I will pass `&Default::default()` as state to the handler (dummy).
            // And I will expose a way for the handler to access the Engine?
            // Currently `Route` handler doesn't get Engine.

            // THIS IS A BLOCKER for the elegant "Route" API with Scc2.

            // Alternative: Scc2KvApp uses `CommandRoute` for reads too? (Commands don't get state).
            // But we want fast reads.

            // I will update `lib.rs` to fetch "global" state for Scc2?
            // No, Scc2 has no global state.

            // I will modify `lib.rs` to detect Scc2 and handle it gracefully.
            // BUT I can't easily change the `Route` callback signature.

            // Strategy:
            // 1. In `lib.rs`, catch the panic or avoid calling `state()`.
            // 2. Pass a dummy state.
            // 3. Provide a `thread_local` or `lazy_static` access to Engine? No.

            // The best way is to allow `RaftstoneApplication` to define `type State = Scc2Engine<V>`.
            // Then `RwLock<Scc2Engine<V>>`? No.

            // Let's stick to the plan: `State` is the value type.
            // I will modify `lib.rs` to use `engine.read_state("global", ...)` as default?
            // If the app is KV, maybe we use a special "root" key?

            // For this example, I'll use a dirty hack:
            // I'll define `State` as `Arc<Scc2Engine<KeyValue>>`? No, circular.

            // Let's look at `lib.rs`.
            // `create_stateless_router_with_shared_engine` calls `engine_guard.state().with_state(...)`.
            // I will change `state()` to `read_state("global", ...)`?
            // `Engine::state()` returns `&StateEngine`.
            // `Engine::read_state` uses a callback.

            // I will update `lib.rs` to use `read_state("global", ...)` for the route handler state.
            // This means for Scc2 app, we MUST have a "global" entry that acts as the entry point?
            // Or `State` should be a `HashMap` (RwLock mode) for the root?

            // If I want true sharded KV, `RaftstoneApplication` might be the wrong abstraction.
            // But I can't change everything now.

            // Compromise:
            // Scc2 KV App will use `State = ()`.
            // It will rely on `CommandRoute` for everything? (Read via Command? No).

            // I'll fix `lib.rs` to handle Scc2 by creating a fresh default state for the handler.
            // AND I'll add a way to access the engine from the handler?
            // Maybe `Route` can accept `Context` which includes Engine.

            // Better: `RaftstoneApplication` is for Global State apps.
            // `Scc2Engine` is for High Perf KV.
            // Maybe we don't mix them yet in this iteration.

            // User asked for "useful".
            // The "Useful" part of Scc2 is mostly as a backend for the existing `HashMap` state?
            // i.e. `AppState` containing `Scc2HashMap`.
            // But `Scc2HashMap` is not serializable directly.

            // Let's go with: Fix `lib.rs` to pass a DUMMY state to routes in Scc2 mode.
            // And add `Scc2Engine` access via a global static or similar in the example.
            // It's ugly but demonstrates the engine.

            // Actually, I can use `CommandRoute` for reads. It's just not RESTful (POST).
            // But wait, `CommandRoute` can be GET!
            // `CommandRoute` handler gets `sender`. It sends a message to worker.
            // Worker has access to engine.
            // Worker calls `apply_event`. `apply_event` is write.

            // We need a `QueryRoute` that gets access to `Engine`.
            // `Route` gets `&State`.

            // OK, I will modify `lib.rs` to pass `&Engine` to `Route`?
            // That changes the trait signature. Breaking change.

            // I will implement `read_state` in `lib.rs` to use "global" key.
            // The `Scc2KvApp` will use a single "global" key that contains a `HashMap`?
            // Then `Scc2Engine<HashMap>`? That defeats the purpose of sharding.

            // I will put this example on hold/simplify it.
            // The user just asked to INTEGRATE Scc2. I did it in `mod.rs` and `scc2_engine.rs`.
            // The integration in `lib.rs` (HTTP server) is the missing link.

            // I will fix `lib.rs` to avoid the panic.
            // In `create_stateless_router_with_shared_engine`:
            // Check engine mode. If Scc2, we can't give &State.
            // We can't easily change `lib.rs` flow without big refactor.

            // I will skip the example for now and tell the user:
            // "Scc2 Engine integrated in Core. HTTP integration requires further refactoring of the Router to support Key-Based routing."
            // This is honest.

            // But wait, I can create the example and manualy wire it up without `Lithair::run`?
            // I can use `Engine` directly in the example!
            // "Manual Mode".

            Route::new(HttpMethod::GET, "/", |_req, _, _state| {
                HttpResponse::ok().text("Use manual engine access")
            })
        ]
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // ...
    Ok(())
}

