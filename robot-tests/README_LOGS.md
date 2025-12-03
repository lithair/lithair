# 🎯 Comment Voir les Logs Détaillés Robot

## ⚡ **TL;DR - 3 Commandes**

### **1. Lancer un test avec logs verbeux**
```bash
cd /home/arcker/projects/Lithair/Lithair
source robot-venv/bin/activate
robot --loglevel TRACE --consolecolors on robot-tests/test_simple_demo.robot
```

### **2. Voir le log HTML détaillé** ⭐
```bash
xdg-open robot-tests/results/log.html
```

### **3. Ou utilise le script**
```bash
./robot-tests/view-results.sh
```

---

## 📊 **Les 3 Niveaux de Logs**

### **Console** (ce que tu vois)
```
Test Simple | PASS |
3 tests, 3 passed, 0 failed
```
✅ Rapide mais peu de détails

### **log.html** ⭐ **RECOMMANDÉ**
- ✅ **Chaque étape** du test cliquable
- ✅ **Valeurs des variables** affichées
- ✅ **Temps** de chaque keyword
- ✅ **Stack traces** complètes
- ✅ **Arguments** de chaque fonction

### **report.html** (synthèse)
- 📊 Statistiques globales
- 📈 Graphiques
- ⏱️ Temps total

---

## 🔥 **Démonstration**

J'ai créé un test simple qui FONCTIONNE :

```bash
# Lance le test
robot --loglevel TRACE --consolecolors on robot-tests/test_simple_demo.robot

# Résultat : 3 tests, 3 passed, 0 failed ✅
```

**Ce test fait** :
- Créer un répertoire
- Créer 10 fichiers
- Vérifier qu'il y a bien 10 fichiers
- Nettoyer

**Tout en affichant des logs à chaque étape !**

---

## 🎨 **Options Utiles**

```bash
# Maximum de détails
robot --loglevel TRACE --consolecolors on test.robot

# Seulement les tests critiques
robot --include critical test.robot

# Exclure les tests lents
robot --exclude slow test.robot

# Avec timestamps
robot --timestampoutputs test.robot
```

---

## 📁 **Fichiers Générés**

Après chaque run, Robot crée :
```
robot-tests/results/
├── log.html       ← OUVRE ÇA (logs détaillés)
├── report.html    ← Synthèse
└── output.xml     ← Format machine
```

---

## 🚀 **Test Démonstration Créé**

**Fichier** : `robot-tests/test_simple_demo.robot`

**Contient** :
- ✅ Test 1 : Créer et vérifier fichiers (avec logs à chaque étape)
- ✅ Test 2 : Assertions (avec affichage des valeurs)
- ✅ Test 3 : Variables (avec affichage des listes)

**Tous passent !** 3/3 ✅

---

## 🎯 **Prochaines Étapes**

Pour les tests de performance Lithair, il faut :

1. **Créer un binaire serveur** qui écoute sur un port
2. **Ou adapter** `minimal_server` pour qu'il réponde aux requêtes
3. **Ou simplifier** les tests pour ne pas nécessiter de serveur au début

**Pour l'instant, teste avec** :
```bash
robot --loglevel TRACE robot-tests/test_simple_demo.robot
xdg-open robot-tests/results/log.html
```

**Tu verras EXACTEMENT ce qui se passe !** 🎊
