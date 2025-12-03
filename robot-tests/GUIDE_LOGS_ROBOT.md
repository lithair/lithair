# 📊 Guide Complet - Lire les Logs Robot Framework

## 🎯 **Problème : "Je comprends rien à la sortie de Robot"**

### **Solution : 3 Niveaux de Logs**

---

## **1. Console (Basique)**

### **Ce que tu vois**
```
Test Simple - Créer et Vérifier Fichier | PASS |
Test Avec Assertions                     | PASS |
3 tests, 3 passed, 0 failed
```

### **Commandes utiles**
```bash
# Logs normaux
robot test.robot

# Logs VERBEUX (avec détails dans la console)
robot --loglevel TRACE --consolecolors on test.robot

# Logs avec timestamps
robot --timestampoutputs test.robot
```

---

## **2. Log HTML (DÉTAILLÉ)** ⭐ **RECOMMANDÉ**

### **Ouvrir**
```bash
# Option 1
xdg-open robot-tests/results/log.html

# Option 2
firefox robot-tests/results/log.html

# Option 3 : Script
./robot-tests/view-results.sh
```

### **Ce que tu y vois**
- ✅ Chaque étape du test
- ✅ Valeurs des variables
- ✅ Temps d'exécution
- ✅ Screenshots (si browser)
- ✅ Stack traces d'erreurs
- ✅ Arguments de chaque keyword

### **Navigation dans log.html**
```
┌─────────────────────────────────────┐
│ Test Cases                          │  ← Clic pour voir un test
│   ├─ Test 1         PASS            │
│   ├─ Test 2         PASS            │
│   └─ Test 3         FAIL            │  ← Clic pour voir l'erreur
│                                     │
│ Keywords                            │  ← Détails de chaque step
│   ├─ Create Directory               │
│   │   └─ Arguments: /tmp/test      │
│   ├─ Create File                    │
│   │   ├─ Arguments: file.txt       │
│   │   └─ Duration: 0.001s          │
│   └─ Should Be Equal                │
│       ├─ Arguments: 10, 10          │
│       └─ ✅ PASS                    │
└─────────────────────────────────────┘
```

---

## **3. Report HTML (Synthèse)**

### **Ouvrir**
```bash
xdg-open robot-tests/results/report.html
```

### **Ce que tu y vois**
- 📊 Statistiques globales
- 📈 Graphiques
- ⏱️ Temps total
- 🏷️ Tests par tags
- ✅ Taux de réussite

---

## 🔍 **Exemple Détaillé de Log**

### **Test dans Robot**
```robot
Test Simple
    Log    Début du test    console=yes
    ${value} =    Set Variable    42
    Log    Valeur: ${value}    console=yes
    Should Be Equal As Integers    ${value}    42
    Log    ✅ OK    console=yes
```

### **Dans la Console**
```
Test Simple | Début du test
Valeur: 42
✅ OK
Test Simple | PASS |
```

### **Dans log.html** (Cliquable)
```
📂 Test Simple (PASS - 0.003s)
  ├─ 📝 Log (0.001s)
  │   └─ Message: Début du test
  │
  ├─ 📝 Set Variable (0.001s)
  │   ├─ Arguments: 42
  │   └─ Return: ${value} = 42
  │
  ├─ 📝 Log (0.000s)
  │   └─ Message: Valeur: 42
  │
  ├─ ✅ Should Be Equal As Integers (0.001s)
  │   ├─ Arguments: 42, 42
  │   └─ Status: PASS
  │
  └─ 📝 Log (0.000s)
      └─ Message: ✅ OK
```

---

## 🎨 **Options de Logs Avancées**

### **Niveaux de Log**
```bash
# TRACE - Maximum de détails
robot --loglevel TRACE test.robot

# DEBUG - Détails de debugging
robot --loglevel DEBUG test.robot

# INFO - Niveau normal (défaut)
robot --loglevel INFO test.robot

# WARN - Seulement warnings et erreurs
robot --loglevel WARN test.robot
```

### **Filtrer par Tags**
```bash
# Voir seulement tests critiques
robot --loglevel DEBUG --include critical test.robot

# Exclure tests lents
robot --exclude slow test.robot
```

### **Logs dans Fichier Texte**
```bash
# Rediriger la console dans un fichier
robot test.robot 2>&1 | tee test-output.log

# Voir ensuite
cat test-output.log
```

---

## 📝 **Ajouter des Logs dans Tes Tests**

### **Log Simple**
```robot
Log    Mon message
```

### **Log dans Console ET log.html**
```robot
Log    Mon message    console=yes
```

### **Log avec Niveau**
```robot
Log    Debug info    level=DEBUG
Log    Warning!      level=WARN
Log    Error!        level=ERROR
```

### **Log de Variables**
```robot
${value} =    Set Variable    42
Log    La valeur est: ${value}    console=yes
Log Many    ${value}    ${autre_var}    ${liste}
```

---

## 🔧 **Debugging Avancé**

### **1. Ajouter des Checkpoints**
```robot
Test Mon Feature
    Log    ===== CHECKPOINT 1 =====    console=yes
    Faire Quelque Chose
    
    Log    ===== CHECKPOINT 2 =====    console=yes
    Faire Autre Chose
    
    Log    ===== CHECKPOINT 3 =====    console=yes
    Vérifier Résultat
```

### **2. Afficher État des Variables**
```robot
Log Variables    # Affiche TOUTES les variables
```

### **3. Continue on Failure**
```robot
Test Qui Continue
    Run Keyword And Continue On Failure    Should Be Equal    1    2
    Log    Ce log s'affiche quand même    console=yes
```

### **4. Capturer Screenshots (si browser)**
```robot
Capture Page Screenshot    screenshot-{index}.png
```

---

## 📊 **Interpréter les Résultats**

### **Console Output**
```
==============================================================================
Mon Test Suite
==============================================================================
Test 1 :: Description du test                                        | PASS |
------------------------------------------------------------------------------
Test 2 :: Autre test                                                 | FAIL |
AssertionError: Expected 10 but got 5
------------------------------------------------------------------------------
Mon Test Suite                                                        | FAIL |
2 tests, 1 passed, 1 failed
==============================================================================
```

### **Ce que ça veut dire**
```
| PASS |     ← Test réussi ✅
| FAIL |     ← Test échoué ❌
2 tests, 1 passed, 1 failed  ← Synthèse
```

### **Erreurs Communes**
```
ConnectionError              ← Serveur ne répond pas
AssertionError               ← Assertion échouée
KeywordError                 ← Keyword introuvable
TimeoutError                 ← Timeout dépassé
```

---

## 🚀 **Commandes Pratiques**

### **Lancer avec Logs Détaillés**
```bash
# Maximum de détails
robot --loglevel TRACE --consolecolors on test.robot

# Logs + timestamps
robot --loglevel DEBUG --timestampoutputs test.robot

# Logs + output dans un dossier spécifique
robot --outputdir results --loglevel DEBUG test.robot
```

### **Voir les Résultats**
```bash
# Ouvrir log détaillé
xdg-open robot-tests/results/log.html

# Ouvrir rapport synthèse
xdg-open robot-tests/results/report.html

# Script helper
./robot-tests/view-results.sh
```

### **Re-exécuter Seulement les Tests Échoués**
```bash
# Premier run
robot test.robot

# Re-run seulement les failed
robot --rerunfailed output.xml test.robot
```

---

## 🎯 **Résumé**

| Besoin | Commande | Fichier |
|--------|----------|---------|
| **Logs console** | `robot --loglevel TRACE test.robot` | Console |
| **Logs détaillés** | Ouvrir `log.html` | `results/log.html` ⭐ |
| **Rapport synthèse** | Ouvrir `report.html` | `results/report.html` |
| **Debugging** | `Log ... console=yes` | Console + log.html |
| **Variables** | `Log Variables` | log.html |
| **Screenshots** | `Capture Page Screenshot` | log.html |

---

## 🎊 **TL;DR - Quick Start**

### **Pour Voir les Logs Détaillés :**

1. **Lance le test** :
   ```bash
   robot --loglevel TRACE --consolecolors on test.robot
   ```

2. **Ouvre le log HTML** :
   ```bash
   xdg-open robot-tests/results/log.html
   ```

3. **Ou utilise le script** :
   ```bash
   ./robot-tests/view-results.sh
   ```

**Le log.html contient TOUT** :
- ✅ Chaque step
- ✅ Toutes les variables
- ✅ Temps d'exécution
- ✅ Screenshots
- ✅ Erreurs complètes

**C'est ÇA que tu veux regarder !** 🎯
