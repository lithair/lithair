# Lithair Framework - Product Requirements Document

## 1. Introduction & Vision

**Problème :** Le développement d'applications modernes peut être freiné par la
complexité de l'architecture 3-tiers classique. La gestion séparée d'un
backend, d'une base de données et de leur communication réseau ajoute souvent
de la latence, des risques de désynchronisation de schémas, et un déploiement
plus lourd.

**Solution :** **Lithair** est un framework Rust intégré qui rapproche le
backend et la couche de données dans une seule unité logique et un unique
binaire. En traitant l'application elle-même comme un système orienté état et
événements, Lithair vise à réduire une partie de la complexité accidentelle
tout en conservant de bonnes performances sur les workloads adaptés.

**Mission :** "Permettre aux développeurs de construire des applications web
distribuées, performantes et résilientes, avec la simplicité d'un seul
langage, d'un seul binaire et d'un framework cohérent."

## 2. Le Concept Fondamental : Un Framework "Coquille"

**Lithair n'est pas un produit fini, c'est un framework.** Il fournit la
"coquille" et le moteur, et le développeur y injecte sa logique métier.

**Le Développeur Définit le "Quoi" :** L'utilisateur du framework se concentre
uniquement sur la logique de son application :

- **Les Données :** Il définit ses modèles de données en écrivant de simples
  `struct`s Rust.
- **La Logique :** Il implémente ses endpoints d'API en écrivant des fonctions
  Rust.
- **L'Interface :** Il crée ses pages web (React, Svelte, etc.) comme il le
  ferait normalement.

**Lithair Gère le "Comment" :** Le framework s'occupe d'une grande partie de la
plomberie complexe, avec une surface plus cohérente :

- **Persistance :** Journalisation automatique des changements d'état.
- **Concurrence :** Gestion sécurisée de l'accès à l'état en mémoire.
- **Réseau :** Exposition des APIs et service des fichiers du frontend.
- **Scalabilité :** Réplication de l'état entre les nœuds du cluster via le
  protocole Raft.
- **Le développeur peut écrire du code proche d'une application simple, tout en
  gardant la possibilité d'évoluer vers une application distribuée.**

## 3. Principes d'Architecture

- **Source de Vérité Unique :** Le journal d'événements immuable (Event
  Sourcing).
- **Séparation des Chemins :** Dissociation complète écriture/lecture (CQRS).
- **État en Mémoire Vive :** Lectures très rapides depuis des vues matérialisées.
- **Déploiement Monolithique, Exécution Distribuée :** Un seul binaire, conçu
  pour le clustering natif (Raft).
- **Rust Natif :** Le schéma de données et les requêtes sont définis par le
  code Rust, garantissant la sécurité des types à la compilation.

## 4. Public Cible

- Développeurs et équipes (de toute taille) qui valorisent la performance, la
  simplicité opérationnelle et une expérience de développement supérieure.
- Concepteurs de systèmes nécessitant une faible latence, une haute
  disponibilité et une scalabilité horizontale (temps-réel, IoT, plateformes
  SaaS).

## 5. Fonctionnalités Clés (MVP)

**Le Moteur Lithair :**

- Un seul binaire exécutable.
- Serveur HTTP intégré pour l'API et le service du frontend.
- Moteur de persistance basé sur un journal d'événements.
- Gestion de l'état en mémoire avec `RwLock` pour la concurrence.

**Le Framework "Coquille" :**

- Des macros ou des traits pour permettre au développeur
  de déclarer facilement ses modèles de données et ses
  routes API.
- Un système de "hooks" ou de "listeners" d'événements pour injecter de la
  logique métier (ex: "quand un `UserRegistered` se produit, lancer cette
  fonction").
- Configuration simple pour spécifier le dossier du frontend à servir.

**Dashboard d'Administration Intégré :**

- Visualisation de l'état du nœud.
- Explorateur de données pour les "vues" en mémoire.

## 6. Feuille de Route Post-MVP

- **Clustering & Haute Disponibilité :** Activation de la réplication
  multi-nœuds via Raft.
- **Accès Secondaire Avancé :** "Dynamic Query Endpoint" pour l'exploration de
  données.
- **Temps Réel :** Exposition des flux d'événements via WebSockets pour les
  "Live Queries".
- **Gestion des Migrations :** Outils pour le versionnage et la transformation
  des événements pour une évolution du schéma sans temps d'arrêt.

## 7. Analyse Concurrentielle

**PocketBase :** Un produit, pas un framework. Simple, mais moins extensible
pour certains besoins spécifiques et moins orienté clustering natif.

**Frameworks Web (Next.js, Rails) :** Gèrent bien la partie "coquille" du
code, mais reposent généralement sur une base de données externe et donc sur
plus de couches à opérer.

**Bases de données distribuées (CockroachDB, TiDB) :** Résolvent une partie du
problème de la scalabilité de la base, mais laissent en place une architecture
applicative plus classique.

## 8. Différenciateurs Clés

- **Moins de Dépendances Externes :** Une architecture pensée pour réduire le
  nombre de composants à opérer.
- **Framework, pas Produit :** Une bonne extensibilité pour les besoins spécifiques.
- **Performances Memory-First :** Une architecture qui réduit certaines couches
  d'abstraction sur les workloads adaptés.
- **Expérience Développeur :** Simplicité d'un monolithe, puissance du distribué.

Cette version du PRD met l'accent sur **l'expérience du développeur**.
Lithair n'est pas seulement une couche de persistance différente ; c'est une
manière plus intégrée d'assembler certaines applications.
