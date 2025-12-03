// RaftStone RBAC Frontend Demo
// Features: Session management, RBAC, Frontend caching, Activity logging

// ============================================================================
// STATE MANAGEMENT
// ============================================================================

const State = {
    token: null,
    role: null,
    username: null,
    products: [],
    permissions: {
        canRead: false,
        canWrite: false,
        canDelete: false
    },
    cache: {
        products: null,
        timestamp: null,
        ttl: 30000 // 30 seconds cache
    }
};

// ============================================================================
// API CLIENT WITH CACHING
// ============================================================================

const API = {
    baseURL: window.location.origin,

    async request(endpoint, options = {}) {
        const headers = {
            'Content-Type': 'application/json',
            ...options.headers
        };

        if (State.token) {
            headers['Authorization'] = `Bearer ${State.token}`;
        }

        const response = await fetch(`${this.baseURL}${endpoint}`, {
            ...options,
            headers
        });

        const data = await response.text();

        return {
            ok: response.ok,
            status: response.status,
            data: data ? JSON.parse(data) : null
        };
    },

    // Authentication
    async login(username, password) {
        return this.request('/auth/login', {
            method: 'POST',
            body: JSON.stringify({ username, password })
        });
    },

    async logout() {
        return this.request('/auth/logout', { method: 'POST' });
    },

    // Products with caching
    async getProducts(useCache = true) {
        // Check cache first
        if (useCache && State.cache.products) {
            const age = Date.now() - State.cache.timestamp;
            if (age < State.cache.ttl) {
                logActivity('📦 Products loaded from cache', 'success');
                updateCacheStatus(`Cached (${Math.round(age / 1000)}s ago)`);
                return { ok: true, data: State.cache.products, cached: true };
            }
        }

        // Fetch from server
        const response = await this.request('/api/products');

        if (response.ok) {
            // Update cache
            State.cache.products = response.data;
            State.cache.timestamp = Date.now();
            updateCacheStatus('Fresh from server');
            logActivity('📦 Products loaded from server', 'success');
        }

        return response;
    },

    async createProduct(product) {
        const response = await this.request('/api/products', {
            method: 'POST',
            body: JSON.stringify(product)
        });

        if (response.ok) {
            // Invalidate cache
            State.cache.products = null;
        }

        return response;
    },

    async deleteProduct(id) {
        const response = await this.request(`/api/products/${id}`, {
            method: 'DELETE'
        });

        if (response.ok) {
            // Invalidate cache
            State.cache.products = null;
        }

        return response;
    }
};

// ============================================================================
// UI MANAGEMENT
// ============================================================================

function showPage(pageId) {
    document.querySelectorAll('.page').forEach(page => {
        page.classList.add('hidden');
    });
    document.getElementById(pageId).classList.remove('hidden');
}

function showError(message) {
    const errorEl = document.getElementById('login-error');
    errorEl.textContent = message;
    errorEl.classList.add('show');
    setTimeout(() => errorEl.classList.remove('show'), 5000);
}

function updateCacheStatus(status) {
    document.getElementById('cache-info').textContent = status;
}

function logActivity(message, type = 'info') {
    const logEl = document.getElementById('activity-log');
    const timestamp = new Date().toLocaleTimeString();

    const entry = document.createElement('div');
    entry.className = `log-entry ${type}`;
    entry.innerHTML = `
        <div class="log-timestamp">${timestamp}</div>
        <div>${message}</div>
    `;

    logEl.insertBefore(entry, logEl.firstChild);

    // Keep only last 20 entries
    while (logEl.children.length > 20) {
        logEl.removeChild(logEl.lastChild);
    }
}

// ============================================================================
// PERMISSION MANAGEMENT
// ============================================================================

function updatePermissions(role) {
    // Define permission matrix
    const permissionMatrix = {
        'Customer': {
            canRead: true,
            canWrite: false,
            canDelete: false
        },
        'Employee': {
            canRead: true,
            canWrite: true,
            canDelete: false
        },
        'Administrator': {
            canRead: true,
            canWrite: true,
            canDelete: true
        }
    };

    State.permissions = permissionMatrix[role] || {
        canRead: false,
        canWrite: false,
        canDelete: false
    };

    // Update UI
    const permissionsEl = document.getElementById('permissions-list');
    permissionsEl.innerHTML = `
        <div class="permission-item ${State.permissions.canRead ? 'allowed' : 'denied'}">
            <strong>${State.permissions.canRead ? '✅' : '❌'} Read Products</strong>
            <div>GET /api/products</div>
        </div>
        <div class="permission-item ${State.permissions.canWrite ? 'allowed' : 'denied'}">
            <strong>${State.permissions.canWrite ? '✅' : '❌'} Create/Update</strong>
            <div>POST/PUT /api/products</div>
        </div>
        <div class="permission-item ${State.permissions.canDelete ? 'allowed' : 'denied'}">
            <strong>${State.permissions.canDelete ? '✅' : '❌'} Delete Products</strong>
            <div>DELETE /api/products/:id</div>
        </div>
    `;

    // Show/hide create section based on write permission
    const createSection = document.getElementById('create-section');
    if (State.permissions.canWrite) {
        createSection.classList.remove('hidden');
    } else {
        createSection.classList.add('hidden');
    }
}

// ============================================================================
// PRODUCT MANAGEMENT
// ============================================================================

async function refreshProducts(useCache = true) {
    const productsEl = document.getElementById('products-list');
    productsEl.innerHTML = '<div class="loading">Loading products...</div>';

    const response = await API.getProducts(useCache);

    if (response.ok) {
        State.products = response.data || [];
        renderProducts();

        if (response.cached) {
            logActivity(`📦 Loaded ${State.products.length} products from cache`, 'success');
        } else {
            logActivity(`📦 Loaded ${State.products.length} products from server`, 'success');
        }
    } else {
        productsEl.innerHTML = `<div class="error show">Failed to load products: ${response.status}</div>`;
        logActivity(`❌ Failed to load products: ${response.status}`, 'error');
    }
}

function renderProducts() {
    const productsEl = document.getElementById('products-list');

    if (State.products.length === 0) {
        productsEl.innerHTML = '<div class="loading">No products found. Create your first product!</div>';
        return;
    }

    productsEl.innerHTML = State.products.map(product => `
        <div class="product-card">
            <div class="product-id">ID: ${product.id}</div>
            <div class="product-name">${product.name}</div>
            <div class="product-price">$${parseFloat(product.price).toFixed(2)}</div>
            <div class="product-actions">
                <button onclick="deleteProduct('${product.id}')" class="btn btn-danger btn-small">
                    🗑️ Delete
                </button>
            </div>
        </div>
    `).join('');
}

async function deleteProduct(id) {
    if (!confirm(`Are you sure you want to delete product ${id}?`)) {
        return;
    }

    logActivity(`🗑️ Deleting product ${id}...`);

    const response = await API.deleteProduct(id);

    if (response.ok) {
        logActivity(`✅ Product ${id} deleted successfully`, 'success');
        await refreshProducts(false); // Force refresh from server
    } else {
        if (response.status === 403) {
            logActivity(`❌ Delete failed: Insufficient permissions`, 'error');
            alert('You do not have permission to delete products!');
        } else {
            logActivity(`❌ Delete failed: ${response.status}`, 'error');
        }
    }
}

// ============================================================================
// AUTHENTICATION
// ============================================================================

async function handleLogin(event) {
    event.preventDefault();

    const username = document.getElementById('username').value;
    const password = document.getElementById('password').value;

    logActivity(`🔐 Attempting login as ${username}...`);

    const response = await API.login(username, password);

    if (response.ok) {
        const data = response.data;

        // Store session
        State.token = data.session_token;
        State.role = data.role;
        State.username = username;

        // Update permissions
        updatePermissions(data.role);

        // Update UI
        document.getElementById('user-name').textContent = username;
        document.getElementById('user-role').textContent = data.role;
        document.getElementById('user-role').className = `role-badge ${data.role.toLowerCase()}`;

        // Show dashboard
        showPage('dashboard-page');

        // Load products with cache
        await refreshProducts(true);

        logActivity(`✅ Logged in as ${username} (${data.role})`, 'success');
        logActivity(`🔑 Session token: ${data.session_token.substring(0, 16)}...`, 'success');
    } else {
        showError('Login failed. Please check your credentials.');
        logActivity(`❌ Login failed for ${username}`, 'error');
    }
}

function quickLogin(username, password) {
    document.getElementById('username').value = username;
    document.getElementById('password').value = password;
    document.getElementById('login-form').requestSubmit();
}

async function logout() {
    logActivity(`👋 Logging out ${State.username}...`);

    await API.logout();

    // Clear state
    State.token = null;
    State.role = null;
    State.username = null;
    State.products = [];
    State.cache.products = null;

    // Reset form
    document.getElementById('login-form').reset();

    // Show login page
    showPage('login-page');

    logActivity(`✅ Logged out successfully`, 'success');
}

// ============================================================================
// PRODUCT CREATION
// ============================================================================

async function handleCreateProduct(event) {
    event.preventDefault();

    const product = {
        id: document.getElementById('product-id').value,
        name: document.getElementById('product-name').value,
        price: parseFloat(document.getElementById('product-price').value)
    };

    logActivity(`➕ Creating product ${product.id}...`);

    const response = await API.createProduct(product);

    if (response.ok) {
        logActivity(`✅ Product ${product.id} created successfully`, 'success');
        document.getElementById('create-form').reset();
        await refreshProducts(false); // Force refresh from server
    } else {
        if (response.status === 403) {
            logActivity(`❌ Create failed: Insufficient permissions`, 'error');
            alert('You do not have permission to create products!');
        } else {
            logActivity(`❌ Create failed: ${response.status}`, 'error');
        }
    }
}

// ============================================================================
// INITIALIZATION
// ============================================================================

document.addEventListener('DOMContentLoaded', () => {
    // Setup event listeners
    document.getElementById('login-form').addEventListener('submit', handleLogin);
    document.getElementById('create-form').addEventListener('submit', handleCreateProduct);

    // Initial activity log entry
    logActivity('🚀 RaftStone RBAC Demo initialized', 'success');
    logActivity('💡 Click on a demo user to quick login', 'success');

    // Show login page
    showPage('login-page');
});

// Auto-refresh cache every 30 seconds when on dashboard
setInterval(() => {
    if (!document.getElementById('dashboard-page').classList.contains('hidden')) {
        const age = Date.now() - (State.cache.timestamp || 0);
        if (age >= State.cache.ttl && State.cache.products) {
            logActivity('⏰ Cache expired, auto-refreshing...', 'success');
            refreshProducts(false);
        }
    }
}, 30000);
