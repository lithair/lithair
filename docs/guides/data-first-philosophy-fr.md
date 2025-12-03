# 🧠 Lithair: Data-First Philosophy

## 🎯 **The Mental Model Revolution**

Lithair fundamentally changes how we think about backend applications. Instead of **separating** business logic and persistence, we **unify** everything in the data definition.

### 🏗️ **Architecture 3-Tiers Traditionnelle**

```
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│   PRESENTATION  │    │    BUSINESS     │    │   DATA LAYER    │
│                 │    │     LOGIC       │    │                 │
│ - Controllers   │───▶│ - Services      │───▶│ - Database      │
│ - Routes        │    │ - Validation    │    │ - ORM/Queries   │
│ - Serialization │    │ - Business Rules│    │ - Migrations    │
└─────────────────┘    └─────────────────┘    └─────────────────┘
```

**Problèmes:**
- 🔥 **Complexité dispersée**: La logique métier est éparpillée dans 3 couches
- 🐛 **Désynchronisation**: Modèles, migrations, validations divergent
- 🏭 **Boilerplate massif**: CRUD répétitif, mapping ORM, DTO...
- 🕳️ **Failles**: Historique, audit, permissions ajoutés après coup

### ⚡ **Lithair: Data-First Unification**

```
┌─────────────────────────────────────────────────────────────────┐
│                    DATA MODEL (Single Source of Truth)         │
│                                                                 │
│  #[derive(DeclarativeModel)]                                    │
│  pub struct User {                                              │
│      #[db(primary_key)]           ◄── Database constraints     │
│      #[lifecycle(immutable)]      ◄── Business rules           │
│      #[http(expose)]              ◄── API generation           │
│      #[persistence(replicate)]    ◄── Distribution strategy    │
│      #[permission(read="UserRead")]◄── Security policies       │
│      pub id: Uuid,                                              │
│  }                                                              │
└─────────────────────────────────────────────────────────────────┘
                                    │
                    ┌───────────────┼───────────────┐
                    ▼               ▼               ▼
        ┌─────────────────┐ ┌─────────────────┐ ┌─────────────────┐
        │   HTTP API      │ │   PERSISTENCE   │ │  DISTRIBUTION   │
        │  (Generated)    │ │   (Generated)   │ │   (Generated)   │
        └─────────────────┘ └─────────────────┘ └─────────────────┘
```

## 🎨 **Exemples Comparatifs**

### 📝 **Besoin: User avec Historique d'Email**

#### 🏭 **Approche 3-Tiers Traditionnelle**

```sql
-- Migration 1: Table principale
CREATE TABLE users (
    id UUID PRIMARY KEY,
    username VARCHAR(255) UNIQUE NOT NULL,
    current_email VARCHAR(255) NOT NULL,
    created_at TIMESTAMP DEFAULT NOW()
);

-- Migration 2: Table d'historique (ajoutée plus tard)
CREATE TABLE user_email_history (
    id UUID PRIMARY KEY,
    user_id UUID REFERENCES users(id),
    old_email VARCHAR(255),
    new_email VARCHAR(255),
    changed_at TIMESTAMP DEFAULT NOW(),
    changed_by UUID
);

-- Trigger pour l'historique (complexité supplémentaire)
CREATE OR REPLACE FUNCTION track_email_changes()
RETURNS TRIGGER AS $$
BEGIN
    INSERT INTO user_email_history (user_id, old_email, new_email, changed_by)
    VALUES (NEW.id, OLD.current_email, NEW.current_email, current_user_id());
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER email_history_trigger
    BEFORE UPDATE ON users
    FOR EACH ROW
    EXECUTE FUNCTION track_email_changes();
```

```rust
// Modèle ORM (désynchronisé des migrations)
#[derive(Serialize, Deserialize)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub current_email: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Serialize, Deserialize)]
pub struct UserEmailHistory {
    pub id: Uuid,
    pub user_id: Uuid,
    pub old_email: String,
    pub new_email: String,
    pub changed_at: DateTime<Utc>,
    pub changed_by: Uuid,
}

// Service layer (logique métier dispersée)
impl UserService {
    pub async fn update_email(&self, user_id: Uuid, new_email: String) -> Result<()> {
        // 1. Validation manuelle
        if !is_valid_email(&new_email) {
            return Err("Invalid email format");
        }
        
        // 2. Vérifier permissions (logique séparée)
        if !self.auth.can_update_user(user_id) {
            return Err("Insufficient permissions");
        }
        
        // 3. Transaction complexe
        let mut tx = self.db.begin().await?;
        
        // 4. Récupérer l'ancien email
        let old_user = sqlx::query_as::<_, User>(
            "SELECT * FROM users WHERE id = $1"
        )
        .bind(user_id)
        .fetch_one(&mut tx)
        .await?;
        
        // 5. Insérer dans l'historique (manuellement)
        sqlx::query!(
            "INSERT INTO user_email_history (user_id, old_email, new_email, changed_by) 
             VALUES ($1, $2, $3, $4)",
            user_id, old_user.current_email, new_email, self.current_user_id
        )
        .execute(&mut tx)
        .await?;
        
        // 6. Mettre à jour l'utilisateur
        sqlx::query!(
            "UPDATE users SET current_email = $1 WHERE id = $2",
            new_email, user_id
        )
        .execute(&mut tx)
        .await?;
        
        tx.commit().await?;
        
        // 7. Invalidation cache (oubliée souvent)
        self.cache.invalidate(&format!("user:{}", user_id));
        
        Ok(())
    }
}

// Controller (encore plus de boilerplate)
#[post("/users/{id}/email")]
pub async fn update_user_email(
    path: web::Path<Uuid>,
    body: web::Json<UpdateEmailRequest>,
    service: web::Data<UserService>
) -> Result<HttpResponse> {
    let user_id = path.into_inner();
    
    match service.update_email(user_id, body.new_email.clone()).await {
        Ok(_) => Ok(HttpResponse::Ok().json("Email updated")),
        Err(e) => Ok(HttpResponse::BadRequest().json(format!("Error: {}", e)))
    }
}
```

**Problèmes:**
- 📄 **50+ lignes de code** pour une simple mise à jour
- 🔗 **3 endroits à maintenir** (migration, modèle, service)
- 🐛 **Bugs fréquents**: oubli d'historique, permissions, cache
- 🔄 **Logique dupliquée** dans différents services

#### ⚡ **Approche Lithair Data-First**

```rust
#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
pub struct User {
    #[db(primary_key)]
    #[lifecycle(immutable)]
    #[http(expose)]
    pub id: Uuid,
    
    #[db(unique, indexed)]
    #[lifecycle(audited)]  // ◄── Historique automatique !
    #[http(expose, validate = "email")]  // ◄── Validation automatique !
    #[permission(write = "UserEmailUpdate")]  // ◄── Permissions déclarées !
    pub email: String,
    
    #[db(unique, indexed)]
    #[http(expose)]
    pub username: String,
    
    #[lifecycle(immutable)]
    #[http(expose)]
    pub created_at: DateTime<Utc>,
}
```

**C'est TOUT !** Lithair génère automatiquement :
- ✅ **Event sourcing** avec historique complet
- ✅ **Validation** email intégrée  
- ✅ **Permissions** RBAC
- ✅ **API HTTP** avec routes CRUD
- ✅ **Sérialisation** JSON
- ✅ **Contraintes** base de données

## 🧠 **Mental Model Shift**

### 🏭 **Pensée 3-Tiers: "Comment stocker ?"**
```
Business Logic ──► "Comment je sauvegarde ça ?" ──► Database Design
     ▲                                                      │
     └─────────── "Comment je récupère ça ?" ◄─────────────┘
```

### ⚡ **Pensée Lithair: "Qu'est-ce que c'est ?"**
```
Data Model ──► "C'est quoi cette donnée ?"
    │
    ├─► #[lifecycle(audited)]     ──► "Elle a besoin d'historique"
    ├─► #[permission(write="...")]──► "Qui peut la modifier ?"
    ├─► #[db(unique)]             ──► "Elle doit être unique"
    ├─► #[persistence(replicate)] ──► "Elle doit être répliquée"
    └─► #[http(expose)]           ──► "Elle est exposée en API"
```

## 🎯 **Avantages Disruptifs**

### 📍 **Single Source of Truth**
- **1 définition** → Tout est généré de façon cohérente
- **Pas de désync** entre modèle, DB, API
- **Refactoring sûr** : changer 1 ligne propage partout

### 🚀 **Vélocité Développement**
```rust
// Ajouter un champ avec historique et permissions
#[lifecycle(audited)]
#[permission(write = "UserPhoneUpdate")]
pub phone: Option<String>,  // ◄── 3 lignes = feature complète !
```

### 🛡️ **Sécurité by Design**
- Permissions **déclarées** dans le modèle
- Impossible d'oublier les validations
- Audit trail **automatique**

### 🔧 **Évolution Schema**
```rust
// Migration automatique avec préservation d'historique
#[lifecycle(audited, retention = 365)]  // ◄── Garde 1 an d'historique
pub email: String,
```

### 🌊 **Flow Mental Naturel**
1. 🤔 **"J'ai besoin d'un User avec email"**
2. ✍️ **Décrire la structure + attributs**
3. 🚀 **Lithair fait le reste**

Vs approche traditionnelle :
1. 🤔 "J'ai besoin d'un User"
2. 📄 Écrire le modèle
3. 🗄️ Créer la migration
4. 🔧 Implémenter le service
5. 🌐 Créer les routes
6. ✅ Ajouter les validations
7. 🔒 Gérer les permissions
8. 📚 Historique (oublié souvent)

## 🎨 **Patterns Avancés**

### 🔄 **Évolution Temporelle**
```rust
#[derive(DeclarativeModel)]
pub struct Product {
    #[lifecycle(versioned = 5)]  // ◄── Garde 5 versions
    pub price: f64,
    
    #[lifecycle(immutable)]      // ◄── Ne change jamais
    pub sku: String,
    
    #[lifecycle(snapshot_only)]  // ◄── Pas d'événements intermédiaires
    pub stock_count: u32,
}
```

### 🌐 **Distribution Intelligente**
```rust
#[derive(DeclarativeModel)]
pub struct Order {
    #[persistence(replicate, track_history)]  // ◄── Critique
    pub status: OrderStatus,
    
    #[persistence(memory_only)]               // ◄── Cache local
    pub processing_metadata: serde_json::Value,
    
    #[persistence(auto_persist)]              // ◄── Sauvegarde auto
    pub customer_notes: String,
}
```

### 🔐 **Sécurité Multi-Niveau**
```rust
#[derive(DeclarativeModel)]
pub struct User {
    #[permission(read = "UserReadAny", write = "UserWriteAny")]
    pub email: String,
    
    #[permission(read = "UserReadOwn", write = "UserWriteOwn")]
    #[rbac(owner_field)]  // ◄── Permissions basées sur propriété
    pub private_notes: String,
    
    #[permission(write = "AdminOnly")]
    pub admin_flags: AdminFlags,
}
```

## 🎭 **Impact Psychologique**

### 🧠 **Charge Cognitive Réduite**
- **Focus sur le QUOI** (la donnée) au lieu du COMMENT (l'implémentation)
- **Moins de context switching** entre couches
- **Documentation vivante** dans le code

### 🎯 **Productivité Décuplée**
- **Features en minutes** au lieu d'heures
- **Moins d'bugs** (génération cohérente)
- **Maintenance simplifiée** (1 endroit à changer)

### 🚀 **Innovation Accelerée**
- **Prototypage rapide** de nouvelles idées
- **Refactoring sans peur**
- **Expérimentation sûre**

---

## 💡 **Conclusion: Le Futur du Backend**

Lithair ne fait pas que **simplifier** le développement backend - il **révolutionne** la façon dont nous pensons les applications.

**Avant:** "Comment je code cette fonctionnalité ?"
**Maintenant:** "Comment je modélise cette donnée ?"

Cette approche **Data-First** transforme la complexité accidentelle en expressivité déclarative, permettant aux développeurs de se concentrer sur la **valeur métier** plutôt que sur la plomberie technique.

*Le code devient la documentation. La documentation devient le code. La donnée devient l'architecture.*