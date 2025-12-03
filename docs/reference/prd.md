# Lithair Framework - Product Requirements Document

## 1. Introduction & Vision

**Problème :** Le développement d'applications modernes est freiné par la complexité inhérente à l'architecture 3-tiers classique. La gestion séparée d'un backend, d'une base de données et de leur communication réseau engendre une latence inévitable, des risques de désynchronisation de schémas, et un déploiement complexe qui ralentit l'innovation.

**Solution :** **Raftstone** est un framework Rust "tout-en-un" qui fusionne le backend et la base de données en une seule unité logique et un unique binaire. En traitant l'application elle-même comme une base de données distribuée en mémoire, Raftstone élimine la complexité accidentelle et offre des performances brutes.

**Mission :** "Permettre aux développeurs de construire des applications web distribuées, ultra-performantes et résilientes, avec la simplicité d'un seul langage, d'un seul binaire et d'un seul framework."

## 2. Le Concept Fondamental : Un Framework "Coquille"

**Raftstone n'est pas un produit fini, c'est un framework.** Il fournit la "coquille" et le moteur, et le développeur y injecte sa logique métier.

**Le Développeur Définit le "Quoi" :** L'utilisateur du framework se concentre uniquement sur la logique de son application :
- **Les Données :** Il définit ses modèles de données en écrivant de simples `struct`s Rust.
- **La Logique :** Il implémente ses endpoints d'API en écrivant des fonctions Rust.
- **L'Interface :** Il crée ses pages web (React, Svelte, etc.) comme il le ferait normalement.

**Raftstone Gère le "Comment" :** Le framework s'occupe de toute la plomberie complexe, de manière totalement transparente :
- **Persistance :** Journalisation automatique des changements d'état.
- **Concurrence :** Gestion sécurisée de l'accès à l'état en mémoire.
- **Réseau :** Exposition des APIs et service des fichiers du frontend.
- **Scalabilité :** Réplication de l'état entre les nœuds du cluster via le protocole Raft.
- **Le développeur écrit du code comme pour une application simple, et obtient une application distribuée de classe mondiale.**

## 3. Principes d'Architecture

- **Source de Vérité Unique :** Le journal d'événements immuable (Event Sourcing).
- **Séparation des Chemins :** Dissociation complète écriture/lecture (CQRS).
- **État en Mémoire Vive :** Lectures quasi-instantanées depuis des vues matérialisées.
- **Déploiement Monolithique, Exécution Distribuée :** Un seul binaire, conçu pour le clustering natif (Raft).
- **Rust Natif :** Le schéma de données et les requêtes sont définis par le code Rust, garantissant la sécurité des types à la compilation.

## 4. Public Cible

- Développeurs et équipes (de toute taille) qui valorisent la performance, la simplicité opérationnelle et une expérience de développement supérieure.
- Concepteurs de systèmes nécessitant une faible latence, une haute disponibilité et une scalabilité horizontale (temps-réel, IoT, plateformes SaaS).

## 5. Fonctionnalités Clés (MVP)

**Le Moteur Raftstone :**
- Un seul binaire exécutable.
- Serveur HTTP intégré pour l'API et le service du frontend.
- Moteur de persistance basé sur un journal d'événements.
- Gestion de l'état en mémoire avec `RwLock` pour la concurrence.

**Le Framework "Coquille" :**
- Des macros ou des traits (`#[RaftstoneModel]`, `#[RaftstoneApi]`) pour permettre au développeur de déclarer facilement ses modèles de données et ses routes API.
- Un système de "hooks" ou de "listeners" d'événements pour injecter de la logique métier (ex: "quand un `UserRegistered` se produit, lancer cette fonction").
- Configuration simple pour spécifier le dossier du frontend à servir.

**Dashboard d'Administration Intégré :**
- Visualisation de l'état du nœud.
- Explorateur de données pour les "vues" en mémoire.

## 6. Feuille de Route Post-MVP

- **Clustering & Haute Disponibilité :** Activation de la réplication multi-nœuds via Raft.
- **Accès Secondaire Avancé :** "Dynamic Query Endpoint" pour l'exploration de données.
- **Temps Réel :** Exposition des flux d'événements via WebSockets pour les "Live Queries".
- **Gestion des Migrations :** Outils pour le versionnage et la transformation des événements pour une évolution du schéma sans temps d'arrêt.

## 7. Analyse Concurrentielle

**PocketBase :** Un produit, pas un framework. Simple, mais non extensible et non scalable horizontalement.

**Frameworks Web (Next.js, Rails) :** Gèrent la partie "coquille" du code mais requièrent une base de données externe, recréant le problème que nous résolvons.

**Bases de données distribuées (CockroachDB, TiDB) :** Résolvent le problème de la scalabilité de la base, mais pas celui de la complexité de l'architecture 3-tiers.

## 8. Différenciateurs Clés

- **Zéro Dépendance Externe :** Tout est construit from scratch pour un contrôle total.
- **Framework, pas Produit :** Extensibilité maximale pour les besoins spécifiques.
- **Performances Brutes :** Élimination de toutes les couches d'abstraction inutiles.
- **Expérience Développeur :** Simplicité d'un monolithe, puissance du distribué.

Cette version du PRD met l'accent sur **l'expérience du développeur**. Raftstone n'est pas juste un "nouveau type de base de données", c'est une **"nouvelle manière de construire des applications"**.