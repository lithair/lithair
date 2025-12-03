# 🤖 Robot Framework pour Lithair

## 🎯 **C'EST EXACTEMENT CE QUE TU CHERCHES !**

**Robot Framework** = Framework avec **keywords prédéfinis** !

Tu écris juste :
```robot
File Should Exist         /tmp/test.txt
File Should Contain       /tmp/test.txt    ArticleCreated
Run Process              ./my-binary    --arg1    --arg2
GET On Session           api    /health
```

**PAS besoin d'implémenter** `File Should Exist`, `Run Process`, etc. !

---

## 🚀 **Installation**

```bash
pip install robotframework
pip install robotframework-requests     # HTTP
pip install robotframework-process      # Process
pip install robotframework-sshlibrary   # SSH (optionnel)
```

---

## 📚 **Keywords Prédéfinis (Built-in)**

### **1. Fichiers (OperatingSystem Library)**

```robot
# Vérifier existence
File Should Exist                /tmp/test.txt
File Should Not Exist            /tmp/deleted.txt
Directory Should Exist           /tmp/mydir

# Lire fichiers
${content} =    Get File         /tmp/test.txt
${lines} =      Get File Lines   /tmp/test.txt

# Écrire fichiers
Create File                      /tmp/new.txt    Contenu ici
Append To File                   /tmp/log.txt    Nouvelle ligne

# Vérifier contenu
File Should Contain              /tmp/test.txt    ArticleCreated
File Should Not Contain          /tmp/test.txt    Error

# Supprimer
Remove File                      /tmp/test.txt
Remove Directory                 /tmp/mydir    recursive=True

# Copier
Copy File                        /tmp/src.txt    /tmp/dst.txt
Move File                        /tmp/old.txt    /tmp/new.txt

# Informations
${size} =       Get File Size    /tmp/test.txt
${modified} =   Get Modified Time    /tmp/test.txt
```

### **2. Process (Process Library)**

```robot
# Démarrer process
${process} =    Start Process    ./lithair    --config    test.toml    alias=server
Sleep    2s

# Vérifier process
Process Should Be Running        server

# Terminer process
Terminate Process                server
${result} =    Wait For Process    server    timeout=10s

# Exécuter et attendre
${result} =    Run Process       cargo    build    --release
Should Be Equal As Integers      ${result.rc}    0
Log                              ${result.stdout}
```

### **3. HTTP (RequestsLibrary)**

```robot
# Créer session
Create Session    api    http://localhost:8080

# GET
${response} =    GET On Session    api    /health    expected_status=200
Should Contain   ${response.text}    ok

# POST
${data} =        Create Dictionary    title=Test    content=Data
${response} =    POST On Session    api    /articles    json=${data}
Should Be Equal As Integers    ${response.status_code}    201

# PUT, DELETE
PUT On Session     api    /articles/1    json=${data}
DELETE On Session  api    /articles/1    expected_status=204

# Headers
${headers} =     Create Dictionary    Authorization=Bearer token123
GET On Session   api    /protected    headers=${headers}
```

### **4. Variables d'Environnement**

```robot
# Lire
${value} =           Get Environment Variable    RUST_LOG    default=info
Log                  RUST_LOG = ${value}

# Set
Set Environment Variable    DEPLOY_TARGET    production

# Vérifier
Environment Variable Should Be Set    API_KEY
Environment Variable Should Not Be Set    OLD_VAR
```

### **5. Strings et Collections**

```robot
# Strings
Should Contain         ${text}    ArticleCreated
Should Start With      ${text}    Article
Should End With        ${text}    Created
Should Match Regexp    ${text}    Article.*Created

# Listes
${list} =              Create List    item1    item2    item3
Length Should Be       ${list}    3
List Should Contain Value    ${list}    item2
Append To List         ${list}    item4

# Dictionnaires
${dict} =              Create Dictionary    key1=value1    key2=value2
Dictionary Should Contain Key    ${dict}    key1
${value} =             Get From Dictionary    ${dict}    key1
```

### **6. Assertions**

```robot
# Égalité
Should Be Equal                 ${actual}    ${expected}
Should Be Equal As Integers     ${count}     42
Should Be Equal As Numbers      ${price}     99.99

# Conditions
Should Be True                  ${value} > 10
Should Not Be True              ${failed}
Run Keyword If                  ${count} > 0    Log    Items found

# Pas égal
Should Not Be Equal             ${actual}    ${wrong}
```

### **7. Timing et Retry**

```robot
# Attendre
Sleep                           2s
Sleep                           500ms

# Retry
Wait Until Keyword Succeeds     10s    1s    File Should Exist    /tmp/file.txt

# Timeout
Run Keyword And Expect Error    TimeoutError    Wait For Something    timeout=5s
```

---

## 📝 **Exemple Complet Lithair**

```robot
*** Settings ***
Library           Process
Library           RequestsLibrary
Library           OperatingSystem

*** Variables ***
${BINARY}         ../target/release/lithair
${PORT}           19999
${URL}            http://localhost:${PORT}

*** Test Cases ***
Test Complet Lithair
    [Documentation]    Test E2E complet sans écrire de code
    
    # 1. Vérifier binaire
    File Should Exist    ${BINARY}
    
    # 2. Créer config
    Create File    /tmp/config.toml    [server]\nport = ${PORT}
    
    # 3. Démarrer serveur
    Start Process    ${BINARY}    --config    /tmp/config.toml    alias=server
    Sleep    2s
    
    # 4. Tester API
    Create Session    api    ${URL}
    ${resp} =    GET On Session    api    /health
    Should Contain    ${resp.text}    ok
    
    # 5. Créer article
    ${data} =    Create Dictionary    title=Test    content=Data
    POST On Session    api    /articles    json=${data}    expected_status=201
    
    # 6. Vérifier persistence
    File Should Exist    /tmp/lithair/events.raftlog
    ${log} =    Get File    /tmp/lithair/events.raftlog
    Should Contain    ${log}    ArticleCreated
    
    # 7. Nettoyer
    Terminate Process    server
    Remove Files    /tmp/config.toml    /tmp/lithair/*
```

**Lancer** :
```bash
robot lithair_tests.robot
```

**Résultat** : Rapport HTML automatique !

---

## 🎨 **Libraries Additionnelles Disponibles**

```bash
# SSH
pip install robotframework-sshlibrary

# Database
pip install robotframework-databaselibrary

# Browser (Selenium)
pip install robotframework-seleniumlibrary

# REST API avancé
pip install robotframework-jsonlibrary

# Docker
pip install robotframework-dockerlibrary
```

**Utilisation** :
```robot
*** Settings ***
Library    SSHLibrary
Library    DatabaseLibrary
Library    SeleniumLibrary

*** Test Cases ***
Test SSH
    Open Connection    server.example.com
    Login    user    password
    ${output} =    Execute Command    ls -la
    Close Connection

Test Database
    Connect To Database    psycopg2    dbname=test
    ${result} =    Query    SELECT * FROM articles
    Disconnect From Database
```

---

## 📊 **Reporting Automatique**

Après chaque run :
```
output.xml      # XML détaillé
log.html        # Log interactif
report.html     # Rapport de synthèse
```

Ouvre `report.html` pour voir :
- ✅ Tests passés/échoués
- ⏱️ Durée de chaque test
- 📸 Screenshots (si browser)
- 📋 Logs détaillés

---

## 🆚 **Comparaison**

| Feature | Cucumber Rust | Robot Framework |
|---------|---------------|-----------------|
| **Steps prédéfinis** | ❌ Tu codes tout | ✅ Tout est là ! |
| **Langage** | Rust | Aucun (keywords) |
| **Fichiers** | Tu codes | `File Should Exist` |
| **Process** | Tu codes | `Run Process` |
| **HTTP** | Tu codes | `GET On Session` |
| **Learning curve** | Moyen | Facile |
| **Extensible** | Oui (Rust) | Oui (Python) |

---

## 🎯 **Pourquoi Robot Framework ?**

✅ **Keywords prédéfinis** - Fichiers, process, HTTP, tout !  
✅ **Pas de code** - Juste des mots-clés  
✅ **Lisible** - Même pour non-devs  
✅ **Reporting** - HTML automatique  
✅ **Mature** - Utilisé depuis 2005  
✅ **Extensible** - Ajoute tes keywords en Python  

---

## 🚀 **Démarrage Rapide**

```bash
# 1. Installer
pip install robotframework robotframework-requests

# 2. Lancer les tests
robot lithair_tests.robot

# 3. Voir le rapport
open report.html
```

---

## 📚 **Documentation Officielle**

- https://robotframework.org/
- https://robotframework.org/robotframework/latest/RobotFrameworkUserGuide.html
- https://marketsquare.github.io/robotframework-requests/doc/RequestsLibrary.html

---

## 🎊 **C'EST EXACTEMENT CE QUE TU CHERCHES !**

**Avec Robot Framework** :
- ❌ PAS besoin d'implémenter `File Should Exist`
- ❌ PAS besoin d'implémenter `Run Process`
- ❌ PAS besoin d'implémenter `GET On Session`

**C'est DÉJÀ LÀ !** ✅

Tu écris juste :
```robot
File Should Exist    /tmp/test.txt
```

**Et ça marche !** 🎉
