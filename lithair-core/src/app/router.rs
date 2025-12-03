//! Router for LithairServer


/// Route matcher
pub struct Router {
    routes: Vec<Route>,
}

/// Single route
pub struct Route {
    pub method: http::Method,
    pub path: String,
}

impl Router {
    /// Create a new router
    pub fn new() -> Self {
        Self {
            routes: Vec::new(),
        }
    }
    
    /// Add a route
    pub fn add_route(&mut self, method: http::Method, path: impl Into<String>) {
        self.routes.push(Route {
            method,
            path: path.into(),
        });
    }
    
    /// Match a request to a route
    pub fn match_route(&self, method: &http::Method, path: &str) -> Option<&Route> {
        self.routes.iter().find(|route| {
            route.method == *method && Self::path_matches(&route.path, path)
        })
    }
    
    /// Check if a path matches a route pattern
    fn path_matches(pattern: &str, path: &str) -> bool {
        // Simple exact match for now
        // TODO: Add parameter matching (/api/products/:id)
        pattern == path
    }
}

impl Default for Router {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_router_exact_match() {
        let mut router = Router::new();
        router.add_route(http::Method::GET, "/api/products");
        
        assert!(router.match_route(&http::Method::GET, "/api/products").is_some());
        assert!(router.match_route(&http::Method::POST, "/api/products").is_none());
        assert!(router.match_route(&http::Method::GET, "/api/users").is_none());
    }
}
