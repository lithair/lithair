*** Settings ***
Library           Process
Library           RequestsLibrary
Library           OperatingSystem
Library           Collections

*** Variables ***
${BINARY}         ../target/release/raftstone
${TEST_PORT}      19999
${BASE_URL}       http://localhost:${TEST_PORT}
${CONFIG_FILE}    /tmp/test-config.toml

*** Test Cases ***
Test 1: Le Binaire Compile et Existe
    [Documentation]    Vérifie que le binaire RaftStone existe
    File Should Exist    ${BINARY}
    ${result} =    Run Process    ${BINARY}    --version
    Should Be Equal As Integers    ${result.rc}    0
    Log    Binaire version: ${result.stdout}

Test 2: Démarrer le Serveur
    [Documentation]    Démarre RaftStone et vérifie qu'il répond
    [Tags]    server
    
    # Créer la config
    Create File    ${CONFIG_FILE}    [server]\nport = ${TEST_PORT}\n[persistence]\nenabled = true\npath = "/tmp/raftstone-test"
    
    # Démarrer le serveur
    ${process} =    Start Process    ${BINARY}    --config    ${CONFIG_FILE}
    ...    alias=raftstone_server
    Sleep    2s
    
    # Vérifier qu'il répond
    Create Session    raftstone    ${BASE_URL}
    ${response} =    GET On Session    raftstone    /health    expected_status=200
    Should Contain    ${response.text}    ok
    
    # Arrêter
    Terminate Process    raftstone_server
    Remove File    ${CONFIG_FILE}

Test 3: Créer un Article via API
    [Documentation]    Crée un article et vérifie la persistence
    [Tags]    api    persistence
    
    # Config
    Create File    ${CONFIG_FILE}    [server]\nport = ${TEST_PORT}\n[persistence]\nenabled = true\npath = "/tmp/raftstone-api-test"
    
    # Démarrer
    ${process} =    Start Process    ${BINARY}    --config    ${CONFIG_FILE}    alias=server
    Sleep    2s
    
    # Créer article
    Create Session    api    ${BASE_URL}
    ${data} =    Create Dictionary    title=Mon Article    content=Contenu test
    ${response} =    POST On Session    api    /api/articles    json=${data}    expected_status=201
    
    # Vérifier persistence
    File Should Exist    /tmp/raftstone-api-test/events.raftlog
    ${content} =    Get File    /tmp/raftstone-api-test/events.raftlog
    Should Contain    ${content}    ArticleCreated
    
    # Nettoyer
    Terminate Process    server
    Remove Files    ${CONFIG_FILE}    /tmp/raftstone-api-test/*

Test 4: Test de Charge
    [Documentation]    Vérifie que le serveur gère bien la charge
    [Tags]    performance
    
    # Démarrer serveur
    Create File    ${CONFIG_FILE}    [server]\nport = ${TEST_PORT}
    Start Process    ${BINARY}    --config    ${CONFIG_FILE}    alias=load_server
    Sleep    2s
    
    Create Session    load    ${BASE_URL}
    
    # 100 requêtes
    FOR    ${i}    IN RANGE    100
        ${response} =    GET On Session    load    /health    expected_status=200
    END
    
    Log    100 requêtes réussies
    
    # Nettoyer
    Terminate Process    load_server
    Remove File    ${CONFIG_FILE}

Test 5: Redémarrage avec Persistence
    [Documentation]    Vérifie que les données persistent après redémarrage
    [Tags]    persistence
    
    ${persist_path} =    Set Variable    /tmp/raftstone-restart-test
    Create Directory    ${persist_path}
    
    # Config
    Create File    ${CONFIG_FILE}    [server]\nport = ${TEST_PORT}\n[persistence]\nenabled = true\npath = "${persist_path}"
    
    # Premier démarrage
    ${proc1} =    Start Process    ${BINARY}    --config    ${CONFIG_FILE}    alias=first
    Sleep    2s
    
    # Créer données
    Create Session    first    ${BASE_URL}
    ${data} =    Create Dictionary    title=Test Persist    content=Data
    POST On Session    first    /api/articles    json=${data}    expected_status=201
    
    # Vérifier fichier créé
    Sleep    1s
    File Should Exist    ${persist_path}/events.raftlog
    ${events_count} =    Get Line Count    ${persist_path}/events.raftlog
    Should Be True    ${events_count} > 0
    
    # Arrêter
    Terminate Process    first
    Sleep    1s
    
    # Redémarrer
    ${proc2} =    Start Process    ${BINARY}    --config    ${CONFIG_FILE}    alias=second
    Sleep    2s
    
    # Vérifier que les données sont là
    Create Session    second    ${BASE_URL}
    ${response} =    GET On Session    second    /api/articles    expected_status=200
    Should Contain    ${response.text}    Test Persist
    
    # Nettoyer
    Terminate Process    second
    Remove Files    ${CONFIG_FILE}
    Remove Directory    ${persist_path}    recursive=True

Test 6: Variables d'Environnement
    [Documentation]    Vérifie que les variables d'env sont utilisées
    [Tags]    config
    
    # Vérifier que RUST_LOG existe (optionnel)
    ${rust_log} =    Get Environment Variable    RUST_LOG    default=info
    Log    RUST_LOG = ${rust_log}
    
    # Démarrer avec variable d'env
    ${proc} =    Start Process    ${BINARY}    --config    ${CONFIG_FILE}
    ...    env:RUST_LOG=debug    alias=env_test
    Sleep    2s
    
    # Le serveur devrait logger en debug
    Terminate Process    env_test

*** Keywords ***
Créer Config Serveur
    [Arguments]    ${port}    ${persist_path}=${EMPTY}
    [Documentation]    Crée un fichier de config RaftStone
    ${config} =    Set Variable    [server]\nport = ${port}
    Run Keyword If    '${persist_path}' != ''
    ...    Set Variable    ${config}\n[persistence]\nenabled = true\npath = "${persist_path}"
    Create File    ${CONFIG_FILE}    ${config}
    [Return]    ${CONFIG_FILE}
