# language: fr
Fonctionnalité: Test Basique
  Pour vérifier que l'infrastructure Cucumber fonctionne

  Scénario: Serveur de base
    Etant donné un serveur Lithair démarré
    Quand j'effectue une requête GET sur "/health"
    Alors la réponse doit être réussie
