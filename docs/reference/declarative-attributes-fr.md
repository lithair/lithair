# 📚 Guide de Référence des Attributs Déclaratifs Lithair

## 🎯 **Vue d'Ensemble**

Lithair utilise des **attributs déclaratifs** pour définir le comportement des données directement dans leur structure. Chaque attribut encapsule une dimension du comportement (base de données, cycle de vie, HTTP, permissions, persistance).

```rust
#[derive(DeclarativeModel)]
pub struct Product {
    #[db(primary_key)]                    // 🗄️ Contraintes DB
    #[lifecycle(immutable)]               // 🔄 Cycle de vie
    #[http(expose)]                       // 🌐 API HTTP
    #[persistence(replicate)]             // 💾 Distribution
    #[permission(read = "ProductRead")]   // 🔒 Sécurité
    pub id: Uuid,
}
```

---

## 🗄️ **Attributs Database (`#[db(...)]`)**

Définit les contraintes et propriétés au niveau base de données.

### 📋 **Syntaxe**
```rust
#[db(constraint1, constraint2, constraint3 = "value")]
```

### 🔑 **Contraintes Disponibles**

| Attribut | Description | Exemple | Impact |
|----------|-------------|---------|--------|
| `primary_key` | Clé primaire | `#[db(primary_key)]` | ✅ Index unique, immutable par défaut |
| `unique` | Contrainte d'unicité | `#[db(unique)]` | ✅ Index unique, validation automatique |
| `indexed` | Index pour performance | `#[db(indexed)]` | ⚡ Recherche rapide |
| `nullable` | Autorise les valeurs null | `#[db(nullable)]` | 🔄 Type `Option<T>` requis |
| `fk = "Model"` | Clé étrangère | `#[db(fk = "User")]` | 🔗 Référence vers autre modèle |

### 📝 **Exemples Détaillés**

```rust
#[derive(DeclarativeModel)]
pub struct User {
    #[db(primary_key)]
    pub id: Uuid,                          // ✅ PK auto-générée
    
    #[db(unique, indexed)]  
    pub email: String,                     // ✅ Email unique + index
    
    #[db(indexed)]
    pub username: String,                  // ⚡ Recherche rapide
    
    #[db(nullable)]
    pub phone: Option<String>,             // 🔄 Optionnel
}

#[derive(DeclarativeModel)]
pub struct Order {
    #[db(primary_key)]
    pub id: Uuid,
    
    #[db(fk = "User", indexed)]           // 🔗 FK + index
    pub customer_id: Uuid,
    
    #[db(indexed)]                        // ⚡ Recherche par status
    pub status: OrderStatus,
}
```

### ⚙️ **Comportements Automatiques**

```rust
// Lithair génère automatiquement :

// 1. Contraintes DB
CREATE TABLE users (
    id UUID PRIMARY KEY,
    email VARCHAR UNIQUE,
    username VARCHAR,
    phone VARCHAR NULL
);
CREATE INDEX idx_users_email ON users(email);
CREATE INDEX idx_users_username ON users(username);

// 2. Validation à l'insertion
if existing_user_with_email.is_some() {
    return Err("Email already exists");
}

// 3. Requêtes optimisées  
fn find_by_email(email: &str) -> Option<User> {
    // Utilise automatiquement l'index
}
```

---

## 🔄 **Attributs Lifecycle (`#[lifecycle(...)]`)**

Définit la gestion du cycle de vie et de l'évolution des données dans le temps.

### 📋 **Syntaxe**
```rust
#[lifecycle(policy1, policy2, retention = 365)]
```

### 🕰️ **Politiques Disponibles**

| Attribut | Description | Exemple | Impact |
|----------|-------------|---------|--------|
| `immutable` | Ne change jamais | `#[lifecycle(immutable)]` | 🔒 Erreur si modification tentée |
| `audited` | Historique complet | `#[lifecycle(audited)]` | 📝 Toutes modifications trackées |
| `versioned = N` | N versions max | `#[lifecycle(versioned = 5)]` | 🔄 Garde les 5 dernières versions |
| `snapshot_only` | Pas d'événements intermédiaires | `#[lifecycle(snapshot_only)]` | 📸 Seul l'état final compte |
| `retention = N` | Rétention N jours | `#[lifecycle(retention = 365)]` | 🗑️ Auto-suppression après 1 an |

### 📝 **Exemples Détaillés**

```rust
#[derive(DeclarativeModel)]
pub struct Article {
    #[lifecycle(immutable)]
    pub id: Uuid,                          // 🔒 Ne change jamais
    
    #[lifecycle(audited)]
    pub title: String,                     // 📝 Historique complet
    
    #[lifecycle(audited, retention = 90)]
    pub content: String,                   // 📝 + 🗑️ Supprimé après 90j
    
    #[lifecycle(versioned = 3)]
    pub metadata: serde_json::Value,       // 🔄 3 dernières versions
    
    #[lifecycle(snapshot_only)]
    pub view_count: u32,                   // 📸 Seule valeur actuelle
}

#[derive(DeclarativeModel)]  
pub struct UserProfile {
    #[lifecycle(audited, versioned = 10, retention = 1095)]
    pub sensitive_data: String,            // 📝 + 🔄 + 🗑️ Combiné
}
```

### ⚙️ **Comportements Automatiques**

```rust
// Pour #[lifecycle(audited)]:
GET /articles/{id}/history
// Retourne automatiquement:
[
    {
        "field": "title",
        "old_value": "Ancien titre", 
        "new_value": "Nouveau titre",
        "changed_at": "2024-01-15T10:30:00Z",
        "changed_by": "user-uuid"
    }
]

// Pour #[lifecycle(versioned = 3)]:
let versions = article.get_field_versions("metadata"); 
// Retourne les 3 dernières versions automatiquement

// Pour #[lifecycle(retention = 90)]:
// Tâche background auto-générée:
DELETE FROM article_history 
WHERE field = 'content' 
AND changed_at < NOW() - INTERVAL '90 days';
```

---

## 🌐 **Attributs HTTP (`#[http(...)]`)**

Contrôle l'exposition et la validation des champs dans l'API REST.

### 📋 **Syntaxe**
```rust
#[http(expose, validate = "rule", serialize = "format")]
```

### 🌍 **Options Disponibles**

| Attribut | Description | Exemple | Impact |
|----------|-------------|---------|--------|
| `expose` | Exposé dans l'API | `#[http(expose)]` | 🌐 Inclus dans JSON réponses |
| `expose = false` | Masqué de l'API | `#[http(expose = false)]` | 🚫 Jamais dans les réponses |
| `validate = "rule"` | Règle de validation | `#[http(validate = "email")]` | ✅ Validation avant persistance |
| `serialize = "format"` | Format sérialisation | `#[http(serialize = "base64")]` | 🔄 Transformation avant JSON |

### ✅ **Règles de Validation**

| Règle | Description | Exemple | Validation |
|-------|-------------|---------|------------|
| `email` | Format email | `validate = "email"` | `user@domain.com` |
| `length(min, max)` | Longueur | `validate = "length(5, 50)"` | Entre 5 et 50 chars |
| `min_length(n)` | Longueur minimum | `validate = "min_length(8)"` | Au moins 8 chars |
| `range(min, max)` | Valeur numérique | `validate = "range(1, 100)"` | Entre 1 et 100 |
| `regex("pattern")` | Expression régulière | `validate = "regex(\"^[A-Z]+$\")"` | Lettres majuscules |
| `non_empty` | Non vide | `validate = "non_empty"` | String non vide |
| `url` | URL valide | `validate = "url"` | `https://example.com` |
| `uuid` | UUID valide | `validate = "uuid"` | Format UUID correct |

### 📝 **Exemples Détaillés**

```rust
#[derive(DeclarativeModel)]
pub struct User {
    #[http(expose)]
    pub id: Uuid,                          // 🌐 Toujours visible
    
    #[http(expose, validate = "email")]
    pub email: String,                     // 🌐 + ✅ Validation email
    
    #[http(expose, validate = "length(3, 50)")]  
    pub username: String,                  // 🌐 + ✅ 3-50 caractères
    
    #[http(expose = false)]
    pub password_hash: String,             // 🚫 Jamais exposé
    
    #[http(expose, validate = "url")]
    pub website: Option<String>,           // 🌐 + ✅ URL valide si présent
}

#[derive(DeclarativeModel)]
pub struct Product {
    #[http(expose)]
    pub id: Uuid,
    
    #[http(expose, validate = "length(1, 200)")]
    pub name: String,                      // 🌐 + ✅ Nom obligatoire
    
    #[http(expose, validate = "range(0.01, 999999.99)")]
    pub price: f64,                        // 🌐 + ✅ Prix positif
    
    #[http(expose, serialize = "base64")]
    pub image_data: Vec<u8>,               // 🌐 + 🔄 Base64 dans JSON
    
    #[http(expose, validate = "regex(\"^[A-Z]{3}-[0-9]{4}$\")")]
    pub sku: String,                       // 🌐 + ✅ Format ABC-1234
}
```

### ⚙️ **API Générée Automatiquement**

```rust
// Lithair génère automatiquement:

// 1. Routes CRUD avec validation
POST   /users              // Avec validation email, username
PUT    /users/{id}          // Avec validation des champs modifiés
GET    /users/{id}          // password_hash jamais inclus

// 2. Réponses JSON cohérentes
{
    "id": "uuid-here",
    "email": "user@example.com", 
    "username": "john_doe",
    "website": "https://john.dev"
    // password_hash omis automatiquement
}

// 3. Erreurs de validation structurées  
{
    "error": "validation_failed",
    "details": {
        "email": "Invalid email format",
        "username": "Must be between 3 and 50 characters"
    }
}
```

---

## 💾 **Attributs Persistence (`#[persistence(...)]`)**

**NOUVEAU**: Contrôle fin de la persistance et distribution des données.

### 📋 **Syntaxe**
```rust
#[persistence(strategy1, strategy2)]
```

### 🌐 **Stratégies Disponibles**

| Attribut | Description | Exemple | Impact |
|----------|-------------|---------|--------|
| `memory_only` | En mémoire uniquement | `#[persistence(memory_only)]` | ⚡ Rapide, perdu au reboot |
| `persist` | Persistance disque | `#[persistence(persist)]` | 💾 Sauvegardé sur disque |
| `auto_persist` | Persistance automatique | `#[persistence(auto_persist)]` | 💾 Sauvegarde à chaque écriture |
| `replicate` | Réplication distribuée | `#[persistence(replicate)]` | 🌐 Répliqué sur tous les nœuds |
| `track_history` | Historique événements | `#[persistence(track_history)]` | 📝 Journal des modifications |
| `no_replication` | Exclu de la réplication | `#[persistence(no_replication)]` | 🏠 Local au nœud uniquement |

### 📝 **Exemples par Cas d'Usage**

```rust
#[derive(DeclarativeModel)]
pub struct UserSession {
    #[persistence(memory_only)]           // ⚡ Cache rapide
    pub session_token: String,
    
    #[persistence(persist)]               // 💾 Survit aux redémarrages
    pub user_id: Uuid,
    
    #[persistence(replicate, track_history)] // 🌐 + 📝 Critique
    pub login_time: DateTime<Utc>,
}

#[derive(DeclarativeModel)]
pub struct Order {
    #[persistence(replicate, track_history)] // 🌐 + 📝 Données critiques
    pub total_amount: f64,
    
    #[persistence(replicate, track_history)]
    pub status: OrderStatus,
    
    #[persistence(auto_persist)]          // 💾 Sauvegarde auto 
    pub customer_notes: String,
    
    #[persistence(memory_only)]           // ⚡ Calcul temporaire
    pub processing_metadata: serde_json::Value,
}

#[derive(DeclarativeModel)]
pub struct AnalyticsEvent {
    #[persistence(persist, no_replication)] // 💾 Local, pas répliqué
    pub user_agent: String,
    
    #[persistence(memory_only)]           // ⚡ Agrégation temps réel
    pub temp_counters: HashMap<String, u64>,
    
    #[persistence(replicate)]             // 🌐 Métriques partagées
    pub event_type: String,
}
```

### ⚙️ **Comportements Automatiques**

```rust
// Configuration moteur basée sur les attributs:

// memory_only -> Cache L1 rapide
let cache_engine = MemoryEngine::new();

// persist -> SCC2 avec FileStorage
let persistent_engine = Scc2Engine::new(event_store, config);

// replicate -> Distribution Raft automatique
let distributed_engine = RaftEngine::new(cluster_config);

// track_history -> Event sourcing complet
let events = get_field_history("total_amount");
// [
//     {"old": 100.0, "new": 120.0, "at": "2024-01-15T10:00:00Z"},
//     {"old": 120.0, "new": 99.99, "at": "2024-01-15T11:00:00Z"}
// ]
```

---

## 🔒 **Attributs Permission (`#[permission(...)]`)**

Définit les politiques de sécurité et d'accès au niveau des champs.

### 📋 **Syntaxe**
```rust
#[permission(read = "Permission", write = "Permission")]
#[rbac(owner_field, role_based)]
```

### 🛡️ **Permissions Disponibles**

| Attribut | Description | Exemple | Impact |
|----------|-------------|---------|--------|
| `read = "Perm"` | Permission lecture | `#[permission(read = "UserRead")]` | 🔍 Vérification avant lecture |
| `write = "Perm"` | Permission écriture | `#[permission(write = "UserWrite")]` | ✏️ Vérification avant écriture |
| `owner_field` | Basé sur propriété | `#[rbac(owner_field)]` | 👤 Seul le propriétaire accède |
| `role_based` | Basé sur les rôles | `#[rbac(role_based)]` | 🎭 Selon le rôle utilisateur |

### 📝 **Exemples Détaillés**

```rust
#[derive(DeclarativeModel)]
pub struct Article {
    #[permission(read = "ArticleReadAny")]
    pub id: Uuid,                          // 🔍 Tous avec permission
    
    #[permission(read = "ArticleReadAny", write = "ArticleWriteAny")]
    pub title: String,                     // 🔍 + ✏️ Permissions différentes
    
    #[rbac(owner_field)]                  // 👤 Seul l'auteur
    #[permission(write = "ArticleEditOwn")]
    pub content: String,
    
    #[permission(write = "AdminOnly")]    // ✏️ Admins seulement
    pub featured: bool,
    
    // Champs sans attributs = accès libre selon modèle parent
    pub created_at: DateTime<Utc>,
}

#[derive(DeclarativeModel)]
pub struct User {
    #[permission(read = "UserReadAny")]
    pub username: String,                  // 🔍 Lecture libre
    
    #[rbac(owner_field)]                  // 👤 Utilisateur ou admin
    #[permission(read = "UserReadOwn", write = "UserWriteOwn")]
    pub email: String,
    
    #[rbac(owner_field)]
    #[permission(read = "UserReadOwn", write = "UserWriteOwn")]
    pub private_notes: String,            // 👤 Strictement personnel
    
    #[permission(read = "AdminOnly", write = "AdminOnly")]
    pub admin_flags: AdminFlags,          // 🔐 Admin uniquement
}
```

### ⚙️ **Vérifications Automatiques**

```rust
// Lithair génère automatiquement:

// 1. Middleware de permission sur chaque route
GET /articles/{id}
// → Vérifie "ArticleReadAny" pour title
// → Vérifie ownership pour content si owner_field

// 2. Filtrage des champs selon permissions
{
    "id": "uuid-here",
    "title": "Visible Title",
    // content omis car l'utilisateur n'est pas propriétaire
    "created_at": "2024-01-15T10:00:00Z"
}

// 3. Erreurs de permission explicites
PUT /articles/{id}
{
    "admin_flags": {"featured": true}  // ← Utilisateur normal
}
// Retourne: 403 Forbidden - Insufficient permissions for field 'admin_flags'

// 4. Requêtes optimisées avec filtres automatiques
SELECT * FROM articles 
WHERE id = $1 
AND (author_id = $2 OR $3 IN (SELECT permission FROM user_permissions WHERE user_id = $2))
```

---

## 🎭 **Attributs RBAC (`#[rbac(...)]`)**

Contrôle d'accès basé sur les rôles et la propriété.

### 📋 **Options RBAC**

| Attribut | Description | Exemple | Comportement |
|----------|-------------|---------|-------------|
| `owner_field` | Propriété données | `#[rbac(owner_field)]` | Seul le propriétaire + admins |
| `role_based` | Basé rôles | `#[rbac(role_based)]` | Selon hiérarchie des rôles |

### 📝 **Exemples Complexes**

```rust
#[derive(DeclarativeModel)]
pub struct Document {
    #[rbac(owner_field)]
    pub author_id: Uuid,                  // 👤 Champ propriétaire
    
    #[rbac(owner_field)]                  // 👤 Seul auteur peut modifier
    #[permission(write = "DocumentEdit")]
    pub content: String,
    
    #[rbac(role_based)]                   // 🎭 Selon rôle hiérarchique  
    #[permission(read = "DocumentModerate")]
    pub moderation_notes: String,
    
    // Accès public (pas d'attribut RBAC)
    pub title: String,
}

// Configuration des rôles (dans l'application)
impl RbacConfig for MyApp {
    fn role_hierarchy() -> Vec<(Role, Vec<Role>)> {
        vec![
            (Role::Admin, vec![Role::Moderator, Role::User]),
            (Role::Moderator, vec![Role::User]),
            (Role::User, vec![]),
        ]
    }
    
    fn owner_field_mapping() -> HashMap<&'static str, &'static str> {
        hashmap! {
            "Document" => "author_id",
            "Comment" => "user_id", 
            "Order" => "customer_id",
        }
    }
}
```

---

## 🎨 **Combinaisons Avancées**

### 🏢 **Modèle Enterprise**
```rust
#[derive(DeclarativeModel)]
pub struct FinancialRecord {
    #[db(primary_key)]
    #[lifecycle(immutable)]
    #[http(expose)]
    #[persistence(replicate, track_history)]
    #[permission(read = "FinanceRead")]
    pub id: Uuid,
    
    #[db(indexed)]
    #[lifecycle(audited, retention = 2555)]  // 7 ans légaux
    #[http(expose, validate = "range(0.01, 999999999.99)")]
    #[persistence(replicate, track_history)]
    #[permission(read = "FinanceRead", write = "FinanceWrite")]
    pub amount: f64,
    
    #[db(fk = "User")]
    #[lifecycle(immutable)]
    #[http(expose)]
    #[persistence(replicate)]
    #[rbac(owner_field)]
    pub created_by: Uuid,
    
    #[lifecycle(audited, versioned = 10)]
    #[http(expose = false)]               // Interne seulement
    #[persistence(memory_only)]           // Cache de calcul
    #[permission(read = "InternalOnly")]
    pub risk_metadata: serde_json::Value,
}
```

### 🚀 **Modèle Performant**
```rust
#[derive(DeclarativeModel)]  
pub struct HighFrequencyEvent {
    #[db(primary_key)]
    #[persistence(memory_only)]           // ⚡ Ultra rapide
    pub id: Uuid,
    
    #[db(indexed)]
    #[lifecycle(snapshot_only)]           // 📸 Pas d'historique
    #[persistence(memory_only)]
    pub event_type: String,
    
    #[lifecycle(retention = 1)]           // 🗑️ 1 jour seulement
    #[persistence(auto_persist)]          // 💾 Batch writes
    pub metrics: HashMap<String, f64>,
    
    // Réplication différée pour performance
    #[persistence(replicate)]             // 🌐 Réplication async
    pub summary_data: EventSummary,
}
```

### 🔐 **Modèle Sécurisé**
```rust
#[derive(DeclarativeModel)]
pub struct SecureUserData {
    #[db(primary_key)]
    #[lifecycle(immutable)]
    #[permission(read = "UserReadOwn")]
    #[rbac(owner_field)]
    pub user_id: Uuid,
    
    #[lifecycle(audited, retention = 365)]
    #[http(expose, validate = "email")]
    #[persistence(replicate, track_history)]
    #[permission(read = "UserReadOwn", write = "UserWriteOwn")]
    #[rbac(owner_field)]
    pub email: String,
    
    #[http(expose = false)]               // 🚫 Jamais exposé
    #[lifecycle(audited, retention = 90)] 
    #[persistence(replicate, track_history)]
    #[permission(read = "SecurityAudit", write = "SecurityAdmin")]
    pub encrypted_personal_data: Vec<u8>,
    
    #[persistence(memory_only)]           // ⚡ Session uniquement
    #[permission(read = "UserReadOwn")]
    #[rbac(owner_field)]
    pub temp_preferences: serde_json::Value,
}
```

---

## 🔧 **Génération Automatique**

### 📊 **Ce que Lithair génère pour vous :**

```rust
// À partir de vos attributs déclaratifs, Lithair génère:

// 1. 🗄️ Schema de base de données optimisé
CREATE TABLE products (
    id UUID PRIMARY KEY,
    name VARCHAR(200) NOT NULL,
    price DECIMAL(10,2) CHECK (price >= 0.01 AND price <= 999999.99),
    sku VARCHAR(10) UNIQUE CHECK (sku ~ '^[A-Z]{3}-[0-9]{4}$')
);
CREATE INDEX idx_products_sku ON products(sku);

// 2. 🌐 API REST complète avec validation
POST   /products          // Validation automatique
GET    /products          // Pagination, filtres
GET    /products/{id}     // Permissions vérifiées 
PUT    /products/{id}     // Validation + permissions
DELETE /products/{id}     // Soft delete si audited
GET    /products/{id}/history  // Historique si lifecycle(audited)

// 3. 🔒 Middleware de sécurité
fn check_permissions(user: &User, operation: Operation, resource: &Resource) {
    // Vérifie automatiquement tous les attributs #[permission] et #[rbac]
}

// 4. 💾 Moteurs de persistance optimisés  
let engines = create_engines_from_attributes(&model_spec);
// Choisit automatiquement Memory/SCC2/Raft selon attributs #[persistence]

// 5. 📝 Audit trail automatique
fn track_changes<T: DeclarativeModel>(old: &T, new: &T, user: UserId) {
    // Compare automatiquement champs #[lifecycle(audited)]
    // Génère événements d'historique
}

// 6. ✅ Validation intégrée
fn validate_product(product: &Product) -> Result<(), ValidationErrors> {
    // Applique automatiquement toutes les règles #[http(validate)]
}
```

## 🎯 **Résumé : Le Pouvoir Déclaratif**

### **Une ligne d'attribut = Des centaines de lignes générées**

```rust
#[lifecycle(audited, retention = 365)]
pub title: String,
```

**Génère automatiquement :**
- 📄 Table d'audit avec colonnes appropriées
- 🔧 Triggers pour capturer les changements  
- 🌐 API `/resource/{id}/history` pour consulter
- 🗑️ Tâche de nettoyage après 365 jours
- ✅ Validation des règles de rétention
- 🔒 Vérifications de permissions d'accès à l'historique
- ⚡ Optimisations de requêtes avec index appropriés

**Impact :** **1 attribut** → **~200 lignes de code équivalent** dans une approche traditionnelle

### **Mental Shift Complet**

❌ **Avant :** "Comment implémenter l'historique ?"
✅ **Maintenant :** "Cette donnée a-t-elle besoin d'historique ?"

❌ **Avant :** "Quelles permissions pour cette API ?"  
✅ **Maintenant :** "Qui peut lire/modifier cette donnée ?"

❌ **Avant :** "Comment optimiser cette requête ?"
✅ **Maintenant :** "Cette donnée est-elle souvent recherchée ?"

**Lithair transforme les questions d'implémentation en questions de modélisation métier.** 🚀