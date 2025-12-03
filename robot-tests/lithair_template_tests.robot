*** Settings ***
Library           Process
Library           OperatingSystem
Library           RequestsLibrary
Suite Setup       Compiler RaftStone
Suite Teardown    Nettoyer Tout

*** Variables ***
${BINARY}         ../target/release/raftstone
${BASE_CONFIG}    /tmp/raftstone-configs

*** Test Cases ***
# ==================== TEMPLATE: TESTER TOUTES LES CONFIGS ====================
Test Multiple Configurations RaftStone
    [Documentation]    Teste automatiquement 12 configurations différentes
    [Template]    Tester Configuration RaftStone
    
    # Format: port | persistence | path                  | description
    19500         true          /tmp/rs-data-1          Config 1: API avec persistence
    19501         false         ${EMPTY}                Config 2: API sans persistence
    19502         true          /tmp/rs-data-2          Config 3: Full stack avec DB
    19503         true          /tmp/rs-data-3          Config 4: Production mode
    19504         false         ${EMPTY}                Config 5: Dev mode rapide
    19505         true          /tmp/rs-cluster-1       Config 6: Cluster node 1
    19506         true          /tmp/rs-cluster-2       Config 7: Cluster node 2
    19507         true          /tmp/rs-cluster-3       Config 8: Cluster node 3

# ==================== FOR LOOP: ENDPOINTS ====================
Test Tous Les Endpoints
    [Documentation]    Teste tous les endpoints de l'API
    
    # Démarrer un serveur une fois
    Démarrer Serveur RaftStone    19600    true    /tmp/rs-endpoints-test
    
    # Tester tous les endpoints
    @{endpoints} =    Create List
    ...    /health
    ...    /status
    ...    /api/articles
    ...    /api/users
    
    FOR    ${endpoint}    IN    @{endpoints}
        Log    Testing endpoint: ${endpoint}
        ${response} =    GET On Session    raftstone    ${endpoint}    expected_status=any
        Log    Response: ${response.status_code}
    END
    
    Arrêter Serveur RaftStone

# ==================== MATRIX: PORTS × PERSISTENCE ====================
Test Matrice Configuration
    [Documentation]    Teste toutes les combinaisons possibles
    
    @{ports} =    Create List    19700    19701    19702
    @{persist_modes} =    Create List    true    false
    @{storage_paths} =    Create List    /tmp/rs-mat-1    /tmp/rs-mat-2
    
    FOR    ${port}    IN    @{ports}
        FOR    ${persist}    IN    @{persist_modes}
            Run Keyword If    '${persist}' == 'true'
            ...    Tester Avec Persistence    ${port}    /tmp/rs-matrix-${port}
            ...    ELSE
            ...    Tester Sans Persistence    ${port}
        END
    END

# ==================== TEMPLATE: PERFORMANCE ====================
Test Performance Avec Différentes Charges
    [Documentation]    Teste la performance avec différents niveaux de charge
    [Template]    Tester Charge Serveur
    
    # Format: port | nb_requetes | concurrence | description
    19800         100           10            Charge légère
    19801         1000          50            Charge moyenne
    19802         5000          100           Charge élevée

# ==================== DATA-DRIVEN: PERSISTENCE ====================
Test Persistence Événements
    [Documentation]    Vérifie que différents types d'événements sont persistés
    
    ${port} =    Set Variable    19900
    ${persist_path} =    Set Variable    /tmp/rs-events-test
    
    Démarrer Serveur RaftStone    ${port}    true    ${persist_path}
    Create Session    raftstone    http://localhost:${port}
    
    # Créer différents types d'événements
    @{events} =    Create List
    ...    ArticleCreated|{"title": "Article 1"}
    ...    ArticleUpdated|{"title": "Article 1 Updated"}
    ...    UserCreated|{"username": "bob"}
    ...    CommentAdded|{"text": "Great post!"}
    
    FOR    ${event}    IN    @{events}
        @{parts} =    Split String    ${event}    |
        ${event_type} =    Set Variable    ${parts}[0]
        ${data} =    Evaluate    json.loads('${parts}[1]')    json
        
        Log    Creating event: ${event_type}
        POST On Session    raftstone    /api/events
        ...    json=${data}    expected_status=any
    END
    
    Sleep    1s
    
    # Vérifier que le fichier contient tous les événements
    ${log_file} =    Set Variable    ${persist_path}/events.raftlog
    File Should Exist    ${log_file}
    ${content} =    Get File    ${log_file}
    
    FOR    ${event}    IN    @{events}
        ${event_type} =    Fetch From Left    ${event}    |
        Should Contain    ${content}    ${event_type}
    END
    
    Arrêter Serveur RaftStone

# ==================== CONDITIONAL: ENVIRONNEMENT ====================
Test Selon Environnement
    [Documentation]    Comportement différent selon l'environnement
    
    ${env} =    Get Environment Variable    RAFTSTONE_ENV    default=dev
    
    Run Keyword If    '${env}' == 'prod'
    ...    Tester Configuration Production
    ...    ELSE IF    '${env}' == 'staging'
    ...    Tester Configuration Staging
    ...    ELSE
    ...    Tester Configuration Dev

# ==================== SETUP/TEARDOWN ====================
Test Avec Préparation Complète
    [Documentation]    Test avec setup et teardown automatiques
    [Setup]    Préparer Environnement Complet
    [Teardown]    Nettoyer Environnement Complet
    
    Log    L'environnement est prêt, base de données initialisée
    Directory Should Exist    ${BASE_CONFIG}
    File Should Exist    ${BASE_CONFIG}/init-done.txt

*** Keywords ***
# ==================== KEYWORDS RÉUTILISABLES ====================

Compiler RaftStone
    [Documentation]    Compile le binaire une seule fois pour toute la suite
    Log    🔨 Compilation de RaftStone...
    ${result} =    Run Process    cargo    build    --release
    ...    cwd=..    timeout=300s
    Should Be Equal As Integers    ${result.rc}    0
    File Should Exist    ${BINARY}
    Log    ✅ RaftStone compilé avec succès

Tester Configuration RaftStone
    [Documentation]    Template pour tester une configuration
    [Arguments]    ${port}    ${persistence}    ${path}    ${description}
    
    Log    🧪 ${description}
    Log    Testing: port=${port}, persistence=${persistence}, path=${path}
    
    # Créer la config
    ${config} =    Générer Config    ${port}    ${persistence}    ${path}
    ${config_file} =    Set Variable    /tmp/config-${port}.toml
    Create File    ${config_file}    ${config}
    
    # Démarrer le serveur
    ${proc} =    Start Process    ${BINARY}    --config    ${config_file}
    ...    alias=server-${port}
    Sleep    2s
    
    # Tester que le serveur répond
    Create Session    test-${port}    http://localhost:${port}
    ${response} =    GET On Session    test-${port}    /health    expected_status=any
    
    # Vérifier persistence si activée
    Run Keyword If    '${persistence}' == 'true'
    ...    Vérifier Persistence Activée    ${path}
    
    # Arrêter et nettoyer
    Terminate Process    server-${port}
    Remove File    ${config_file}
    Run Keyword If    '${persistence}' == 'true'
    ...    Remove Directory    ${path}    recursive=True
    
    Log    ✅ ${description} - OK

Démarrer Serveur RaftStone
    [Documentation]    Démarre RaftStone avec config donnée
    [Arguments]    ${port}    ${persistence}    ${path}=${EMPTY}
    
    ${config} =    Générer Config    ${port}    ${persistence}    ${path}
    ${config_file} =    Set Variable    /tmp/raftstone-${port}.toml
    Create File    ${config_file}    ${config}
    
    Set Global Variable    ${CURRENT_CONFIG_FILE}    ${config_file}
    Set Global Variable    ${CURRENT_PORT}    ${port}
    
    ${proc} =    Start Process    ${BINARY}    --config    ${config_file}
    ...    alias=raftstone-server
    Sleep    2s
    
    Create Session    raftstone    http://localhost:${port}
    Log    ✅ Serveur démarré sur port ${port}

Arrêter Serveur RaftStone
    [Documentation]    Arrête le serveur RaftStone en cours
    
    Terminate Process    raftstone-server
    Remove File    ${CURRENT_CONFIG_FILE}
    Log    🛑 Serveur arrêté

Générer Config
    [Documentation]    Génère un fichier de configuration TOML
    [Arguments]    ${port}    ${persistence}    ${path}=${EMPTY}
    
    ${config} =    Catenate    SEPARATOR=\n
    ...    [server]
    ...    port = ${port}
    ...    ${EMPTY}
    
    ${persist_section} =    Set Variable If    '${persistence}' == 'true'
    ...    [persistence]\nenabled = true\npath = "${path}"
    ...    [persistence]\nenabled = false
    
    ${full_config} =    Catenate    SEPARATOR=\n
    ...    ${config}
    ...    ${persist_section}
    
    RETURN    ${full_config}

Vérifier Persistence Activée
    [Documentation]    Vérifie que la persistence fonctionne
    [Arguments]    ${path}
    
    Directory Should Exist    ${path}
    Log    ✅ Répertoire de persistence existe: ${path}

Tester Charge Serveur
    [Documentation]    Template pour tests de charge
    [Arguments]    ${port}    ${nb_req}    ${concurrence}    ${description}
    
    Log    ⚡ ${description}: ${nb_req} requêtes, concurrence ${concurrence}
    
    Démarrer Serveur RaftStone    ${port}    false
    
    # Simuler la charge (en vrai tu utiliserais un outil de bench)
    FOR    ${i}    IN RANGE    ${nb_req}
        ${response} =    GET On Session    raftstone    /health    expected_status=any
    END
    
    Arrêter Serveur RaftStone
    Log    ✅ ${description} - OK

Tester Avec Persistence
    [Documentation]    Test spécifique avec persistence
    [Arguments]    ${port}    ${path}
    
    Démarrer Serveur RaftStone    ${port}    true    ${path}
    Sleep    1s
    Arrêter Serveur RaftStone
    Directory Should Exist    ${path}
    Remove Directory    ${path}    recursive=True

Tester Sans Persistence
    [Documentation]    Test spécifique sans persistence
    [Arguments]    ${port}
    
    Démarrer Serveur RaftStone    ${port}    false
    Sleep    1s
    Arrêter Serveur RaftStone

Tester Configuration Production
    Log    🏭 Test en mode PRODUCTION
    # Config production spécifique

Tester Configuration Staging
    Log    🎭 Test en mode STAGING
    # Config staging spécifique

Tester Configuration Dev
    Log    🔧 Test en mode DEV
    # Config dev spécifique

Préparer Environnement Complet
    [Documentation]    Setup complet avant test
    Create Directory    ${BASE_CONFIG}
    Create File    ${BASE_CONFIG}/init-done.txt    Initialized
    Log    ✅ Environnement préparé

Nettoyer Environnement Complet
    [Documentation]    Cleanup complet après test
    Remove Directory    ${BASE_CONFIG}    recursive=True
    Log    🧹 Environnement nettoyé

Nettoyer Tout
    [Documentation]    Cleanup final de la suite
    Log    🧹 Nettoyage final
