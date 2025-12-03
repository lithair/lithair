*** Settings ***
Library           OperatingSystem
Library           Collections
Library           Process
Suite Setup       Create Directory    ${BASE_DIR}
Suite Teardown    Remove Directory    ${BASE_DIR}    recursive=True

*** Variables ***
# Variables globales réutilisables
${BASE_DIR}       /tmp/robot-templates
${BINARY}         ../target/release/raftstone
${DEFAULT_PORT}   19999

*** Test Cases ***
# ==================== 1. TEST TEMPLATE (Data-Driven) ====================
Test Création Fichiers avec Template
    [Documentation]    Un seul test, plusieurs données
    [Template]    Créer et Vérifier Fichier
    
    # Format: nom_fichier              contenu
    config.toml                        [server]\\nport = 8080
    data.json                          {"articles": []}
    README.md                          Documentation
    test.txt                           Test content

# ==================== 2. FOR LOOP (Itération) ====================
Test Multiple Ports
    [Documentation]    Tester plusieurs ports en boucle
    
    @{ports} =    Create List    19100    19101    19102    19103
    
    FOR    ${port}    IN    @{ports}
        Log    🚀 Test du port ${port}
        ${config} =    Set Variable    [server]\nport = ${port}
        Create File    /tmp/config-${port}.toml    ${config}
        File Should Exist    /tmp/config-${port}.toml
        ${content} =    Get File    /tmp/config-${port}.toml
        Should Contain    ${content}    ${port}
        Remove File    /tmp/config-${port}.toml
    END

# ==================== 3. TABLE DE DONNÉES ====================
Test Différentes Configurations
    [Documentation]    Tester plusieurs configs avec une table
    
    # Table de test data
    @{configs} =    Create List
    ...    19200|true|/tmp/data1
    ...    19201|false|/tmp/data2
    ...    19202|true|/tmp/data3
    
    FOR    ${config}    IN    @{configs}
        @{parts} =    Split String    ${config}    |
        ${port} =    Set Variable    ${parts}[0]
        ${persist} =    Set Variable    ${parts}[1]
        ${path} =    Set Variable    ${parts}[2]
        
        Log    Testing: port=${port}, persist=${persist}, path=${path}
        ${cfg_content} =    Catenate    SEPARATOR=\n
        ...    [server]
        ...    port = ${port}
        ...    [persistence]
        ...    enabled = ${persist}
        ...    path = "${path}"
        
        Create File    /tmp/test-${port}.toml    ${cfg_content}
        ${saved} =    Get File    /tmp/test-${port}.toml
        Should Contain    ${saved}    port = ${port}
        Remove File    /tmp/test-${port}.toml
    END

# ==================== 4. KEYWORD PARAMÉTRÉ RÉUTILISABLE ====================
Test avec Keyword Réutilisable
    [Documentation]    Keyword custom qui prend des paramètres
    
    # Utiliser le même keyword avec différents paramètres
    Tester Config Serveur    port=19300    persist=true    path=/tmp/test1
    Tester Config Serveur    port=19301    persist=false   path=/tmp/test2
    Tester Config Serveur    port=19302    persist=true    path=/tmp/test3

# ==================== 5. DATA-DRIVEN COMPLET ====================
Test API Endpoints
    [Documentation]    Tester plusieurs endpoints avec template
    [Template]    Vérifier Endpoint Existe
    
    # Format: method    path              expected_keyword
    GET         /health               ok
    GET         /status               status
    POST        /api/articles         articles
    GET         /api/users            users

# ==================== 6. NESTED LOOPS ====================
Test Matrix de Configurations
    [Documentation]    Test combinatoire (ports × persistence)
    
    @{ports} =    Create List    19400    19401
    @{persist_modes} =    Create List    true    false
    
    FOR    ${port}    IN    @{ports}
        FOR    ${persist}    IN    @{persist_modes}
            Log    🧪 Testing port=${port} with persistence=${persist}
            ${config_name} =    Set Variable    test-${port}-${persist}.toml
            ${config} =    Catenate    SEPARATOR=\n
            ...    [server]
            ...    port = ${port}
            ...    [persistence]
            ...    enabled = ${persist}
            
            Create File    /tmp/${config_name}    ${config}
            File Should Exist    /tmp/${config_name}
            Remove File    /tmp/${config_name}
        END
    END

# ==================== 7. CONDITIONAL TESTING ====================
Test Conditionnel
    [Documentation]    Exécuter selon conditions
    
    ${env} =    Get Environment Variable    TEST_ENV    default=dev
    
    Run Keyword If    '${env}' == 'prod'
    ...    Log    🏭 Mode PRODUCTION
    ...    ELSE IF    '${env}' == 'staging'
    ...    Log    🎭 Mode STAGING
    ...    ELSE
    ...    Log    🔧 Mode DEV

# ==================== 8. VARIABLES DYNAMIQUES ====================
Test Variables Dynamiques
    [Documentation]    Créer variables à la volée
    
    ${timestamp} =    Get Time    epoch
    ${unique_id} =    Evaluate    str(${timestamp})[-6:]
    ${test_dir} =    Set Variable    /tmp/test-${unique_id}
    
    Create Directory    ${test_dir}
    Directory Should Exist    ${test_dir}
    
    # Créer N fichiers dynamiquement
    FOR    ${i}    IN RANGE    5
        ${filename} =    Set Variable    ${test_dir}/file-${i}.txt
        Create File    ${filename}    Content ${i}
        File Should Exist    ${filename}
    END
    
    # Vérifier qu'on a bien 5 fichiers
    @{files} =    List Files In Directory    ${test_dir}
    Length Should Be    ${files}    5
    
    Remove Directory    ${test_dir}    recursive=True

# ==================== 9. TAGS ET FILTRAGE ====================
Test avec Tags pour Filtrage
    [Documentation]    Utiliser tags pour sélectionner tests
    [Tags]    api    smoke    fast
    
    Log    Ce test a les tags: api, smoke, fast
    # Lancer avec: robot --include smoke demo_templating.robot

Test Lent
    [Documentation]    Test long
    [Tags]    slow    integration
    
    Log    Ce test est tagué 'slow'
    # Exclure avec: robot --exclude slow demo_templating.robot

*** Keywords ***
# ==================== KEYWORDS RÉUTILISABLES ====================

Créer et Vérifier Fichier
    [Documentation]    Keyword template pour créer/vérifier fichiers
    [Arguments]    ${filename}    ${content}
    
    ${filepath} =    Set Variable    ${BASE_DIR}/${filename}
    Create File    ${filepath}    ${content}
    File Should Exist    ${filepath}
    ${saved_content} =    Get File    ${filepath}
    Should Be Equal    ${saved_content}    ${content}
    Log    ✅ Fichier ${filename} créé et vérifié

Tester Config Serveur
    [Documentation]    Teste une configuration serveur complète
    [Arguments]    ${port}    ${persist}    ${path}
    
    Log    🧪 Test: port=${port}, persist=${persist}, path=${path}
    
    ${config} =    Catenate    SEPARATOR=\n
    ...    [server]
    ...    port = ${port}
    ...    [persistence]
    ...    enabled = ${persist}
    ...    path = "${path}"
    
    ${config_file} =    Set Variable    /tmp/server-${port}.toml
    Create File    ${config_file}    ${config}
    
    # Vérifications
    File Should Exist    ${config_file}
    ${content} =    Get File    ${config_file}
    Should Contain    ${content}    port = ${port}
    Should Contain    ${content}    enabled = ${persist}
    
    # Cleanup
    Remove File    ${config_file}
    
    Log    ✅ Config ${port} testée avec succès

Vérifier Endpoint Existe
    [Documentation]    Template pour vérifier endpoints (simulé)
    [Arguments]    ${method}    ${path}    ${expected}
    
    Log    Testing ${method} ${path} should contain ${expected}
    # En vrai, tu ferais:
    # ${response} =    Run Keyword    ${method} On Session    api    ${path}
    # Should Contain    ${response.text}    ${expected}

*** Test Cases ***
# ==================== 10. SETUP/TEARDOWN AVANCÉ ====================
Test avec Setup et Teardown
    [Documentation]    Préparer l'environnement avant, nettoyer après
    [Setup]    Préparer Environnement Test
    [Teardown]    Nettoyer Environnement Test
    
    Log    🎯 Le test s'exécute dans un environnement préparé
    Directory Should Exist    ${BASE_DIR}
    File Should Exist    ${BASE_DIR}/setup-done.txt

*** Keywords ***
Préparer Environnement Test
    [Documentation]    Setup avant chaque test
    Create Directory    ${BASE_DIR}
    Create File    ${BASE_DIR}/setup-done.txt    Setup completed
    Log    ✅ Environnement préparé

Nettoyer Environnement Test
    [Documentation]    Cleanup après chaque test
    Remove Directory    ${BASE_DIR}    recursive=True
    Log    🧹 Environnement nettoyé
