# 🎨 Guide Complet du Templating Robot Framework

## 🎯 **TU VOULAIS TEMPLATISER ? VOILÀ 10 TECHNIQUES !**

---

## **1. Test Template (Data-Driven)**

### **Le Plus Puissant** ⭐

Un seul test, plusieurs jeux de données !

```robot
*** Test Cases ***
Test Multiple Configs
    [Template]    Tester Configuration
    
    # port    persistence    path
    8080     true           /tmp/data1
    8081     false          ${EMPTY}
    8082     true           /tmp/data2

*** Keywords ***
Tester Configuration
    [Arguments]    ${port}    ${persist}    ${path}
    Log    Testing ${port}...
    # Ton code de test ici
```

**✅ Avantage** : 1 test → N configurations automatiquement !

---

## **2. FOR Loop (Boucles)**

### **Itérer sur des Données**

```robot
*** Test Cases ***
Test Tous Les Ports
    @{ports} =    Create List    8080    8081    8082
    
    FOR    ${port}    IN    @{ports}
        Log    Testing port ${port}
        # Ton test ici
    END
```

**Avec range** :
```robot
FOR    ${i}    IN RANGE    10
    Log    Iteration ${i}
END
```

**✅ Avantage** : Boucles classiques, très flexible !

---

## **3. Variables Globales**

### **Paramètres Réutilisables**

```robot
*** Variables ***
${BINARY}         ./target/release/lithair
${DEFAULT_PORT}   8080
${BASE_DIR}       /tmp/tests
@{PORTS}          8080    8081    8082
&{CONFIG}         port=8080    host=localhost

*** Test Cases ***
Mon Test
    Log    Binary: ${BINARY}
    Log    Port: ${DEFAULT_PORT}
    Log    Ports list: ${PORTS}
    Log    Config dict: ${CONFIG}
```

**✅ Avantage** : Centraliser la config !

---

## **4. Keywords Paramétrés**

### **Fonctions Réutilisables**

```robot
*** Keywords ***
Démarrer Serveur
    [Arguments]    ${port}    ${config_file}
    Start Process    ${BINARY}    --port    ${port}
    ...    --config    ${config_file}

*** Test Cases ***
Test 1
    Démarrer Serveur    8080    /tmp/config1.toml

Test 2
    Démarrer Serveur    8081    /tmp/config2.toml
```

**Avec valeurs par défaut** :
```robot
*** Keywords ***
Démarrer Serveur
    [Arguments]    ${port}=8080    ${persist}=true
    Log    Port=${port}, Persistence=${persist}
```

**✅ Avantage** : DRY (Don't Repeat Yourself) !

---

## **5. Nested Loops (Matrice)**

### **Combinaisons Complètes**

```robot
*** Test Cases ***
Test Matrice
    @{ports} =    Create List    8080    8081
    @{modes} =    Create List    true    false
    
    FOR    ${port}    IN    @{ports}
        FOR    ${mode}    IN    @{modes}
            Log    Testing ${port} with persist=${mode}
            # Ton test ici
        END
    END
```

**✅ Avantage** : Tester TOUTES les combinaisons !

---

## **6. Conditional (Si/Sinon)**

### **Tests Conditionnels**

```robot
*** Test Cases ***
Test Conditionnel
    ${env} =    Get Environment Variable    ENV    default=dev
    
    Run Keyword If    '${env}' == 'prod'
    ...    Tester En Production
    ...    ELSE IF    '${env}' == 'staging'
    ...    Tester En Staging
    ...    ELSE
    ...    Tester En Dev
```

**✅ Avantage** : Comportement adaptatif !

---

## **7. Setup/Teardown**

### **Préparer et Nettoyer Auto**

```robot
*** Test Cases ***
Mon Test
    [Setup]    Préparer Environnement
    [Teardown]    Nettoyer Environnement
    
    Log    Le test s'exécute

*** Keywords ***
Préparer Environnement
    Create Directory    /tmp/test
    Start Process    ${BINARY}

Nettoyer Environnement
    Terminate All Processes
    Remove Directory    /tmp/test    recursive=True
```

**Suite-level** (pour tous les tests) :
```robot
*** Settings ***
Suite Setup       Compiler Le Binaire
Suite Teardown    Nettoyer Tout

*** Keywords ***
Compiler Le Binaire
    Run Process    cargo    build    --release
```

**✅ Avantage** : Environnement propre automatiquement !

---

## **8. Tags (Filtrage)**

### **Organiser et Sélectionner**

```robot
*** Test Cases ***
Test Rapide
    [Tags]    smoke    fast    api
    Log    Test rapide

Test Lent
    [Tags]    slow    integration
    Log    Test long

Test Critique
    [Tags]    critical    smoke
    Log    Test important
```

**Lancer** :
```bash
# Seulement les tests smoke
robot --include smoke tests.robot

# Exclure les tests lents
robot --exclude slow tests.robot

# Combiner
robot --include critical --exclude slow tests.robot
```

**✅ Avantage** : Filtrer facilement !

---

## **9. Resource Files (Modules)**

### **Import de Keywords**

```robot
# common_keywords.robot
*** Keywords ***
Démarrer Serveur
    [Arguments]    ${port}
    Log    Starting on ${port}

# mon_test.robot
*** Settings ***
Resource    common_keywords.robot

*** Test Cases ***
Test 1
    Démarrer Serveur    8080
```

**✅ Avantage** : Réutiliser entre fichiers !

---

## **10. Variables Dynamiques**

### **Calculer à la Volée**

```robot
*** Test Cases ***
Test Variables Dynamiques
    ${timestamp} =    Get Time    epoch
    ${unique_id} =    Evaluate    str(${timestamp})[-6:]
    ${test_dir} =    Set Variable    /tmp/test-${unique_id}
    
    Create Directory    ${test_dir}
    
    # Générer 10 fichiers
    FOR    ${i}    IN RANGE    10
        Create File    ${test_dir}/file-${i}.txt    Data ${i}
    END
```

**Avec expressions** :
```robot
${result} =    Evaluate    5 + 3
${uppercase} =    Evaluate    "${text}".upper()
${json_data} =    Evaluate    json.loads('${json_string}')    json
```

**✅ Avantage** : Flexibilité totale !

---

## 🎯 **Exemple COMPLET Lithair**

### **Tester 12 Configurations Auto**

```robot
*** Settings ***
Library           Process
Library           OperatingSystem
Library           RequestsLibrary

*** Variables ***
${BINARY}    ../target/release/lithair

*** Test Cases ***
Test Toutes Les Configurations
    [Template]    Tester Config Lithair
    
    # port  | persist | path          | desc
    8080    true      /tmp/data1      Config 1: Full
    8081    false     ${EMPTY}        Config 2: No DB
    8082    true      /tmp/cluster1   Config 3: Cluster
    8083    true      /tmp/prod       Config 4: Production
    # ... 8 autres configs

*** Keywords ***
Tester Config Lithair
    [Arguments]    ${port}    ${persist}    ${path}    ${desc}
    
    Log    🧪 ${desc}
    
    # Générer config TOML
    ${config} =    Catenate    SEPARATOR=\n
    ...    [server]
    ...    port = ${port}
    ...    [persistence]
    ...    enabled = ${persist}
    ...    path = "${path}"
    
    Create File    /tmp/config-${port}.toml    ${config}
    
    # Démarrer serveur
    ${proc} =    Start Process    ${BINARY}
    ...    --config    /tmp/config-${port}.toml
    ...    alias=server-${port}
    Sleep    2s
    
    # Tester
    Create Session    api    http://localhost:${port}
    ${resp} =    GET On Session    api    /health
    Should Contain    ${resp.text}    ok
    
    # Nettoyer
    Terminate Process    server-${port}
    Remove File    /tmp/config-${port}.toml
    
    Log    ✅ ${desc} - OK
```

**Lancer** :
```bash
robot lithair_template_tests.robot
```

**Résultat** : 12 tests exécutés automatiquement ! 🎉

---

## 📊 **Comparaison**

| Technique | Cas d'usage | Complexité |
|-----------|-------------|------------|
| **Template** | Même test, plusieurs données | Facile ⭐ |
| **FOR Loop** | Itérations | Facile ⭐ |
| **Keywords** | Réutilisation | Moyen ⭐⭐ |
| **Nested Loops** | Combinaisons | Moyen ⭐⭐ |
| **Conditional** | Tests adaptatifs | Moyen ⭐⭐ |
| **Setup/Teardown** | Environnement propre | Facile ⭐ |
| **Tags** | Organisation | Facile ⭐ |
| **Resources** | Modules | Moyen ⭐⭐ |
| **Variables** | Configuration centralisée | Facile ⭐ |
| **Dynamic** | Génération à la volée | Avancé ⭐⭐⭐ |

---

## 🚀 **Tests Lancés**

Testons les exemples de templating :

```bash
# Lancer les démos de templating
robot demo_templating.robot

# Lancer les tests Lithair templatisés
robot lithair_template_tests.robot

# Filtrer par tags
robot --include fast demo_templating.robot

# Exclure les lents
robot --exclude slow lithair_template_tests.robot
```

---

## 🎊 **Résumé**

### **Ce Que Tu Peux Faire Maintenant**

✅ **1 test → 12 configs auto** avec `[Template]`  
✅ **Boucles** sur ports, modes, configs  
✅ **Matrice complète** de toutes les combinaisons  
✅ **Keywords réutilisables** entre tests  
✅ **Setup/Teardown auto** pour environnement propre  
✅ **Tags** pour filtrer facilement  
✅ **Variables centralisées** pour la config  
✅ **Conditions** pour tests adaptatifs  
✅ **Modules** pour partager entre fichiers  
✅ **Génération dynamique** de données  

**C'est EXACTEMENT ce que tu voulais !** 🎉

**Fichiers créés** :
- `demo_templating.robot` - 10 exemples de templating
- `lithair_template_tests.robot` - Templates pour Lithair
- `GUIDE_TEMPLATING.md` - Ce guide
