# language: fr
Fonctionnalité: Distribution et Consensus
  En tant qu'architecte de systèmes distribués
  Je veux que Lithair supporte le clustering multi-nœuds
  Afin d'assurer la haute disponibilité et la cohérence

  Contexte:
    Soit un cluster Raft de 3 nœuds
    Et que le nœud 1 soit le leader
    Et que les nœuds 2 et 3 soient des followers

  Scénario: Élection du leader
    Quand le leader actuel tombe en panne
    Alors un nouveau leader doit être élu en moins de 5 secondes
    Et le cluster doit continuer à fonctionner

  Scénario: Réplication des données
    Quand j'écris une donnée sur le leader
    Alors cette donnée doit être répliquée sur tous les followers
    Et la cohérence doit être garantie
    Et l'opération doit être confirmée seulement après réplication majoritaire

  Scénario: Partition réseau et split-brain
    Quand le cluster est partitionné en 2 groupes
    Alors seul le groupe majoritaire doit accepter les écritures
    Et le groupe minoritaire doit refuser les écritures
    Et la cohérence doit être préservée

  Scénario: Rejoindre un cluster existant
    Quand un nouveau nœud rejoint le cluster
    Alors il doit synchroniser toutes les données existantes
    Et participer au consensus
    Et ne pas perturber le service

  Scénario: Scalabilité horizontale
    Quand j'ajoute des nœuds au cluster
    Alors la capacité de traitement doit augmenter
    Et la latence doit rester stable
    Et la disponibilité doit être maintenue
