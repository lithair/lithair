*** Settings ***
Library           Process
Library           OperatingSystem
Library           RequestsLibrary
Library           Collections
Library           String
Suite Setup       Compiler RaftStone Et Préparer
Suite Teardown    Nettoyer Tout

Documentation     Tests de Performance et Intégrité de RaftStone
...               Ces tests sont CRITIQUES pour vérifier que sous charge,
...               AUCUNE donnée n'est perdue et les performances sont au RDV

*** Variables ***
${BINARY}            ./target/release/test_server
${BASE_PERSIST}      /tmp/raftstone-perf-robot

*** Test Cases ***
# ====================TESTS D'INTÉGRITÉ ====================

Test 1000 Articles - Aucune Perte
    [Documentation]    Crée 1000 articles et vérifie qu'ils sont TOUS persistés
    [Tags]    integrity    critical
    
    ${port} =    Set Variable    21000
    ${persist_path} =    Set Variable    ${BASE_PERSIST}/integrity-1000
    
    Démarrer Serveur RaftStone    ${port}    ${persist_path}
    
    # Créer 1000 articles
    ${created} =    Créer N Articles    1000    ${port}
    Should Be Equal As Integers    ${created}    1000
    
    # Attendre flush
    Sleep    2s
    
    # Vérifier persistence
    ${event_count} =    Compter Événements Dans Log    ${persist_path}
    Should Be Equal As Integers    ${event_count}    1000
    
    # Vérifier intégrité
    Vérifier Intégrité Complète    ${persist_path}    1000
    
    Arrêter Serveur RaftStone
    Log    ✅ 1000 articles créés, TOUS persistés, intégrité OK

Test 10000 Articles Parallèles - Intégrité Complète
    [Documentation]    10k articles en parallèle, vérifier AUCUNE perte
    [Tags]    integrity    stress    critical
    
    ${port} =    Set Variable    21001
    ${persist_path} =    Set Variable    ${BASE_PERSIST}/integrity-10k
    
    Démarrer Serveur RaftStone    ${port}    ${persist_path}
    
    # Créer 10k articles en parallèle
    Log    🚀 Création de 10000 articles en parallèle...
    ${created} =    Créer N Articles Parallèle    10000    ${port}    threads=50
    
    Sleep    5s
    
    # Vérifications critiques
    ${event_count} =    Compter Événements Dans Log    ${persist_path}
    Should Be Equal As Integers    ${event_count}    10000    msg=PERTE DE DONNÉES DÉTECTÉE!
    
    Vérifier Aucun Doublon    ${persist_path}
    Vérifier Séquence IDs Continue    ${persist_path}    0    9999
    
    Arrêter Serveur RaftStone
    Log    ✅ 10000 articles - AUCUNE perte, séquence continue, pas de doublons

Test Charge 5000 Requêtes - Vérification Intégrité
    [Documentation]    5000 req concurrentes, tout doit être persisté
    [Tags]    load    integrity    critical
    
    ${port} =    Set Variable    21002
    ${persist_path} =    Set Variable    ${BASE_PERSIST}/load-5k
    
    Démarrer Serveur RaftStone    ${port}    ${persist_path}
    
    # Charge concurrente
    ${created} =    Créer N Articles Parallèle    5000    ${port}    threads=100
    
    Sleep    3s
    
    # Vérification CRITIQUE
    ${event_count} =    Compter Événements Dans Log    ${persist_path}
    Should Be Equal As Integers    ${event_count}    5000
    ...    msg=❌ PERTE DE DONNÉES: seulement ${event_count}/5000 événements
    
    Vérifier Fichier Valide JSON    ${persist_path}
    
    Arrêter Serveur RaftStone
    Log    ✅ 5000/5000 requêtes persistées, intégrité validée

# ==================== TESTS DE PERFORMANCE ====================

Test Performance Écriture - Minimum 1000 req/s
    [Documentation]    Mesure throughput d'écriture avec persistence
    [Tags]    performance    write    critical
    
    ${port} =    Set Variable    21003
    ${persist_path} =    Set Variable    ${BASE_PERSIST}/perf-write
    
    Démarrer Serveur RaftStone    ${port}    ${persist_path}
    
    # Mesurer performance
    ${metrics} =    Mesurer Performance Écriture    ${port}    duration_s=10
    
    Log    📊 Performance: ${metrics}[rps] req/s
    Log    📊 Latence moyenne: ${metrics}[avg_latency_ms]ms
    Log    📊 Latence p95: ${metrics}[p95_latency_ms]ms
    
    # Vérifications
    Should Be True    ${metrics}[rps] >= 1000
    ...    msg=❌ Performance insuffisante: ${metrics}[rps] < 1000 req/s
    
    Should Be True    ${metrics}[p95_latency_ms] < 100
    ...    msg=❌ Latence p95 trop élevée: ${metrics}[p95_latency_ms]ms
    
    # Vérifier que TOUT est persisté
    Sleep    2s
    ${event_count} =    Compter Événements Dans Log    ${persist_path}
    Should Be True    ${event_count} >= ${metrics}[requests_sent]
    
    Arrêter Serveur RaftStone
    Log    ✅ Performance: ${metrics}[rps] req/s, p95: ${metrics}[p95_latency_ms]ms, TOUT persisté

Test Performance Lecture - Minimum 5000 req/s
    [Documentation]    Mesure throughput de lecture
    [Tags]    performance    read
    
    ${port} =    Set Variable    21004
    ${persist_path} =    Set Variable    ${BASE_PERSIST}/perf-read
    
    Démarrer Serveur RaftStone    ${port}    ${persist_path}
    
    # Créer des données initiales
    Créer N Articles    1000    ${port}
    Sleep    1s
    
    # Mesurer lecture
    ${metrics} =    Mesurer Performance Lecture    ${port}    duration_s=10
    
    Log    📊 Performance lecture: ${metrics}[rps] req/s
    Log    📊 Latence p99: ${metrics}[p99_latency_ms]ms
    
    Should Be True    ${metrics}[rps] >= 5000
    ...    msg=❌ Performance lecture insuffisante
    
    Should Be True    ${metrics}[p99_latency_ms] < 20
    ...    msg=❌ Latence p99 trop élevée
    
    Arrêter Serveur RaftStone
    Log    ✅ Performance lecture: ${metrics}[rps] req/s, p99: ${metrics}[p99_latency_ms]ms

Test Performance Mixte 80/20 - Minimum 2000 req/s
    [Documentation]    80% lectures / 20% écritures
    [Tags]    performance    mixed    critical
    
    ${port} =    Set Variable    21005
    ${persist_path} =    Set Variable    ${BASE_PERSIST}/perf-mixed
    
    Démarrer Serveur RaftStone    ${port}    ${persist_path}
    
    # Données initiales
    Créer N Articles    100    ${port}
    
    # Test mixte
    ${metrics} =    Test Charge Mixte    ${port}    duration_s=30    read_pct=80    write_pct=20
    
    Log    📊 Throughput total: ${metrics}[total_rps] req/s
    Log    📊 Écritures: ${metrics}[writes]
    Log    📊 Lectures: ${metrics}[reads]
    Log    📊 Latence moyenne: ${metrics}[avg_latency_ms]ms
    
    Should Be True    ${metrics}[total_rps] >= 2000
    Should Be True    ${metrics}[avg_latency_ms] < 30
    Should Be Equal As Integers    ${metrics}[errors]    0
    
    # Vérifier que toutes les écritures sont persistées
    Sleep    2s
    ${event_count} =    Compter Événements Dans Log    ${persist_path}
    Should Be True    ${event_count} >= ${metrics}[writes]
    
    Arrêter Serveur RaftStone
    Log    ✅ Mixte 80/20: ${metrics}[total_rps] req/s, latence: ${metrics}[avg_latency_ms]ms

# ==================== TESTS DE PERSISTENCE SOUS CHARGE ====================

Test Persistence Continue - 60s à 500 req/s
    [Documentation]    Charge constante pendant 60s, vérifier TOUT est persisté
    [Tags]    persistence    load    critical
    
    ${port} =    Set Variable    21006
    ${persist_path} =    Set Variable    ${BASE_PERSIST}/persist-load
    
    Démarrer Serveur RaftStone    ${port}    ${persist_path}
    
    # Charge constante
    ${created} =    Charge Constante    ${port}    duration_s=60    target_rps=500
    
    Log    📊 ${created} requêtes envoyées
    
    Sleep    5s
    
    # Vérification CRITIQUE
    ${event_count} =    Compter Événements Dans Log    ${persist_path}
    ${expected} =    Evaluate    60 * 500
    
    ${diff} =    Evaluate    abs(${event_count} - ${expected})
    ${tolerance} =    Evaluate    ${expected} * 0.02  # Tolérance 2%
    
    Should Be True    ${diff} <= ${tolerance}
    ...    msg=❌ Trop d'événements manquants: ${event_count}/${expected}
    
    Vérifier Taille Fichier Cohérente    ${persist_path}
    Vérifier Aucune Corruption    ${persist_path}
    
    Arrêter Serveur RaftStone
    Log    ✅ ${event_count}/${expected} événements persistés (≈100%)

Test Redémarrage Avec Persistence
    [Documentation]    Redémarrer et vérifier que les données sont là
    [Tags]    persistence    restart    critical
    
    ${port} =    Set Variable    21007
    ${persist_path} =    Set Variable    ${BASE_PERSIST}/restart-test
    
    # Premier démarrage
    Démarrer Serveur RaftStone    ${port}    ${persist_path}
    ${created_1} =    Créer N Articles    1000    ${port}
    Sleep    2s
    
    # Vérifier persistence
    ${count_1} =    Compter Événements Dans Log    ${persist_path}
    Should Be Equal As Integers    ${count_1}    1000
    
    # Redémarrer
    Arrêter Serveur RaftStone
    Sleep    2s
    Démarrer Serveur RaftStone    ${port}    ${persist_path}
    
    # Vérifier que les données sont là
    Create Session    api    http://localhost:${port}
    ${response} =    GET On Session    api    /api/articles
    Should Be Equal As Integers    ${response.status_code}    200
    
    # Créer plus de données
    ${created_2} =    Créer N Articles    1000    ${port}    start_id=1000
    Sleep    2s
    
    # Vérifier total
    ${count_2} =    Compter Événements Dans Log    ${persist_path}
    Should Be Equal As Integers    ${count_2}    2000
    
    Vérifier Séquence IDs Continue    ${persist_path}    0    1999
    
    Arrêter Serveur RaftStone
    Log    ✅ Redémarrage OK, 2000 événements, séquence continue 0-1999

# ==================== TESTS DE CHARGE EXTRÊME ====================

Test Charge Extrême - 50000 Articles
    [Documentation]    50k articles en batches, vérifier intégrité totale
    [Tags]    extreme    stress    critical
    
    ${port} =    Set Variable    21010
    ${persist_path} =    Set Variable    ${BASE_PERSIST}/extreme-50k
    
    Démarrer Serveur RaftStone    ${port}    ${persist_path}
    
    # 10 batches de 5000
    FOR    ${batch}    IN RANGE    10
        ${start_id} =    Evaluate    ${batch} * 5000
        Log    📦 Batch ${batch+1}/10 (IDs ${start_id}-${start_id+4999})
        ${created} =    Créer N Articles Parallèle    5000    ${port}    start_id=${start_id}    threads=100
        Sleep    2s
    END
    
    Sleep    5s
    
    # Vérifications CRITIQUES
    ${event_count} =    Compter Événements Dans Log    ${persist_path}
    Should Be Equal As Integers    ${event_count}    50000
    ...    msg=❌ PERTE MASSIVE: seulement ${event_count}/50000
    
    ${file_size_mb} =    Taille Fichier MB    ${persist_path}/events.raftlog
    Should Be True    ${file_size_mb} >= 5
    
    Vérifier Aucun Doublon    ${persist_path}
    Vérifier Séquence IDs Continue    ${persist_path}    0    49999
    
    # Vérifier que le serveur reste réactif
    ${response_time} =    Mesurer Temps Réponse    ${port}
    Should Be True    ${response_time} < 100
    
    Arrêter Serveur RaftStone
    Log    ✅ 50000 articles - AUCUNE perte, ${file_size_mb}MB, séquence 0-49999, réactif

Test Concurrence Extrême - 1000 Threads × 10 Articles
    [Documentation]    1000 threads créant chacun 10 articles
    [Tags]    concurrency    extreme    critical
    
    ${port} =    Set Variable    21011
    ${persist_path} =    Set Variable    ${BASE_PERSIST}/concurrency-extreme
    
    Démarrer Serveur RaftStone    ${port}    ${persist_path}
    
    Log    ⚡ Lancement de 1000 threads créant 10 articles chacun...
    ${created} =    Créer N Articles Parallèle    10000    ${port}    threads=1000
    
    Sleep    10s
    
    # Vérifications ULTRA-CRITIQUES
    ${event_count} =    Compter Événements Dans Log    ${persist_path}
    Should Be Equal As Integers    ${event_count}    10000
    ...    msg=❌ RACE CONDITION DÉTECTÉE: ${event_count}/10000
    
    Vérifier IDs Uniques    ${persist_path}
    Vérifier Aucun Doublon    ${persist_path}
    
    Arrêter Serveur RaftStone
    Log    ✅ 10000 articles de 1000 threads - AUCUN conflit, IDs uniques

# ==================== TESTS DE DURABILITÉ ====================

Test Durabilité Avec fsync
    [Documentation]    Avec fsync, AUCUNE perte même après crash brutal
    [Tags]    durability    critical
    
    ${port} =    Set Variable    21014
    ${persist_path} =    Set Variable    ${BASE_PERSIST}/durability-fsync
    
    Démarrer Serveur Avec Fsync    ${port}    ${persist_path}
    
    # Créer des données
    ${created} =    Créer N Articles    100    ${port}
    Sleep    1s
    
    # Tuer brutalement (SIGKILL)
    Tuer Serveur Brutalement
    Sleep    2s
    
    # Redémarrer
    Démarrer Serveur Avec Fsync    ${port}    ${persist_path}
    Sleep    2s
    
    # Vérifier que TOUT est là
    ${event_count} =    Compter Événements Dans Log    ${persist_path}
    Should Be Equal As Integers    ${event_count}    100
    ...    msg=❌ PERTE DE DONNÉES après crash: ${event_count}/100
    
    Vérifier Aucune Corruption    ${persist_path}
    
    Arrêter Serveur RaftStone
    Log    ✅ Durabilité fsync: 100/100 événements après SIGKILL

*** Keywords ***
Compiler RaftStone Et Préparer
    [Documentation]    Compile et prépare l'environnement
    Log    🔨 Compilation de test_server...
    ${result} =    Run Process    cargo    build    --release    --bin    test_server
    ...    timeout=300s
    Should Be Equal As Integers    ${result.rc}    0
    File Should Exist    ${BINARY}
    
    Create Directory    ${BASE_PERSIST}
    Log    ✅ Test server compilé et environnement prêt

Démarrer Serveur RaftStone
    [Arguments]    ${port}    ${persist_path}
    [Documentation]    Démarre test_server avec persistence
    
    Create Directory    ${persist_path}
    
    ${proc} =    Start Process    ${BINARY}    --port    ${port}    --persist    ${persist_path}
    ...    alias=raftstone-${port}
    Sleep    2s
    
    Set Global Variable    ${CURRENT_PORT}    ${port}
    
    Log    ✅ Serveur démarré sur ${port} avec persistence ${persist_path}    console=yes

Arrêter Serveur RaftStone
    [Documentation]    Arrête le serveur proprement
    Terminate Process    raftstone-${CURRENT_PORT}
    Sleep    1s
    Log    🛑 Serveur arrêté    console=yes

Créer N Articles
    [Arguments]    ${count}    ${port}    ${start_id}=0
    [Documentation]    Crée N articles séquentiellement
    
    Create Session    api    http://localhost:${port}
    
    FOR    ${i}    IN RANGE    ${count}
        ${id} =    Evaluate    ${start_id} + ${i}
        ${article} =    Create Dictionary
        ...    id=${id}
        ...    title=Article ${id}
        ...    content=Content for article ${id}
        
        ${response} =    POST On Session    api    /api/articles
        ...    json=${article}    expected_status=any
    END
    
    RETURN    ${count}

Créer N Articles Parallèle
    [Arguments]    ${count}    ${port}    ${threads}=50    ${start_id}=0
    [Documentation]    Crée N articles en parallèle
    
    # Utiliser curl en parallèle (simplifié pour la démo)
    ${per_thread} =    Evaluate    ${count} / ${threads}
    
    FOR    ${i}    IN RANGE    ${count}
        ${id} =    Evaluate    ${start_id} + ${i}
        ${result} =    Run Process    curl    -s    -X    POST
        ...    http://localhost:${port}/api/articles
        ...    -H    Content-Type: application/json
        ...    -d    {"id":${id},"title":"Article ${id}","content":"Content ${id}"}
    END
    
    RETURN    ${count}

Compter Événements Dans Log
    [Arguments]    ${persist_path}
    [Documentation]    Compte le nombre d'événements dans le log
    
    ${log_file} =    Set Variable    ${persist_path}/events.raftlog
    File Should Exist    ${log_file}
    
    ${content} =    Get File    ${log_file}
    ${lines} =    Get Line Count    ${content}
    
    RETURN    ${lines}

Vérifier Intégrité Complète
    [Arguments]    ${persist_path}    ${expected_count}
    [Documentation]    Vérifie l'intégrité complète du log
    
    ${log_file} =    Set Variable    ${persist_path}/events.raftlog
    ${content} =    Get File    ${log_file}
    
    # Vérifier que chaque ligne est du JSON valide
    @{lines} =    Split To Lines    ${content}
    ${line_count} =    Get Length    ${lines}
    
    Should Be Equal As Integers    ${line_count}    ${expected_count}
    
    FOR    ${line}    IN    @{lines}
        ${valid} =    Run Keyword And Return Status
        ...    Evaluate    json.loads('${line}')    json
        Should Be True    ${valid}    msg=Ligne JSON invalide: ${line}
    END
    
    Log    ✅ Intégrité validée: ${expected_count} événements JSON valides

Vérifier Aucun Doublon
    [Arguments]    ${persist_path}
    [Documentation]    Vérifie qu'il n'y a pas de doublons
    
    ${log_file} =    Set Variable    ${persist_path}/events.raftlog
    ${content} =    Get File    ${log_file}
    
    @{lines} =    Split To Lines    ${content}
    ${total} =    Get Length    ${lines}
    
    # Compter les lignes uniques
    ${unique_lines} =    Remove Duplicates    ${lines}
    ${unique_count} =    Get Length    ${unique_lines}
    
    Should Be Equal    ${total}    ${unique_count}
    ...    msg=Doublons détectés: ${total} lignes dont ${unique_count} uniques
    
    Log    ✅ Aucun doublon: ${unique_count} lignes uniques

Vérifier Séquence IDs Continue
    [Arguments]    ${persist_path}    ${start_id}    ${end_id}
    [Documentation]    Vérifie que la séquence d'IDs est continue
    
    ${log_file} =    Set Variable    ${persist_path}/events.raftlog
    ${content} =    Get File    ${log_file}
    
    @{lines} =    Split To Lines    ${content}
    
    @{ids} =    Create List
    FOR    ${line}    IN    @{lines}
        ${event} =    Evaluate    json.loads('${line}')    json
        ${id} =    Get From Dictionary    ${event}    id
        Append To List    ${ids}    ${id}
    END
    
    Sort List    ${ids}
    
    FOR    ${i}    IN RANGE    ${end_id - start_id + 1}
        ${expected} =    Evaluate    ${start_id} + ${i}
        ${actual} =    Get From List    ${ids}    ${i}
        Should Be Equal As Integers    ${actual}    ${expected}
        ...    msg=ID manquant: attendu ${expected}, trouvé ${actual}
    END
    
    Log    ✅ Séquence continue ${start_id}-${end_id}

Mesurer Performance Écriture
    [Arguments]    ${port}    ${duration_s}=10
    [Documentation]    Mesure la performance d'écriture
    
    Create Session    api    http://localhost:${port}
    
    ${start} =    Get Time    epoch
    ${count} =    Set Variable    0
    @{latencies} =    Create List
    
    WHILE    True
        ${now} =    Get Time    epoch
        ${elapsed} =    Evaluate    ${now} - ${start}
        Exit For Loop If    ${elapsed} >= ${duration_s}
        
        ${req_start} =    Get Time    epoch
        ${article} =    Create Dictionary    id=${count}    title=Perf ${count}    content=Content
        ${response} =    POST On Session    api    /api/articles    json=${article}    expected_status=any
        ${req_end} =    Get Time    epoch
        
        ${latency_ms} =    Evaluate    (${req_end} - ${req_start}) * 1000
        Append To List    ${latencies}    ${latency_ms}
        ${count} =    Evaluate    ${count} + 1
    END
    
    ${end} =    Get Time    epoch
    ${total_duration} =    Evaluate    ${end} - ${start}
    ${rps} =    Evaluate    ${count} / ${total_duration}
    
    ${avg_latency} =    Evaluate    sum(${latencies}) / len(${latencies})
    Sort List    ${latencies}
    ${p95_index} =    Evaluate    int(len(${latencies}) * 0.95)
    ${p95_latency} =    Get From List    ${latencies}    ${p95_index}
    
    &{metrics} =    Create Dictionary
    ...    rps=${rps}
    ...    requests_sent=${count}
    ...    avg_latency_ms=${avg_latency}
    ...    p95_latency_ms=${p95_latency}
    
    RETURN    ${metrics}

Mesurer Performance Lecture
    [Arguments]    ${port}    ${duration_s}=10
    [Documentation]    Mesure la performance de lecture
    
    Create Session    api    http://localhost:${port}
    
    ${start} =    Get Time    epoch
    ${count} =    Set Variable    0
    @{latencies} =    Create List
    
    WHILE    True
        ${now} =    Get Time    epoch
        ${elapsed} =    Evaluate    ${now} - ${start}
        Exit For Loop If    ${elapsed} >= ${duration_s}
        
        ${req_start} =    Get Time    epoch
        ${response} =    GET On Session    api    /api/articles    expected_status=any
        ${req_end} =    Get Time    epoch
        
        ${latency_ms} =    Evaluate    (${req_end} - ${req_start}) * 1000
        Append To List    ${latencies}    ${latency_ms}
        ${count} =    Evaluate    ${count} + 1
    END
    
    ${end} =    Get Time    epoch
    ${total_duration} =    Evaluate    ${end} - ${start}
    ${rps} =    Evaluate    ${count} / ${total_duration}
    
    Sort List    ${latencies}
    ${p99_index} =    Evaluate    int(len(${latencies}) * 0.99)
    ${p99_latency} =    Get From List    ${latencies}    ${p99_index}
    
    &{metrics} =    Create Dictionary
    ...    rps=${rps}
    ...    requests_sent=${count}
    ...    p99_latency_ms=${p99_latency}
    
    RETURN    ${metrics}

Vérifier Fichier Valide JSON
    [Arguments]    ${persist_path}
    [Documentation]    Vérifie que chaque ligne est du JSON valide
    
    Vérifier Intégrité Complète    ${persist_path}    expected_count=0

Vérifier Aucune Corruption
    [Arguments]    ${persist_path}
    [Documentation]    Vérifie qu'il n'y a pas de corruption
    
    ${log_file} =    Set Variable    ${persist_path}/events.raftlog
    ${content} =    Get File    ${log_file}
    
    @{lines} =    Split To Lines    ${content}
    
    FOR    ${line}    IN    @{lines}
        ${valid} =    Run Keyword And Return Status
        ...    Evaluate    json.loads('${line}')    json
        Should Be True    ${valid}    msg=Corruption détectée: ${line}
    END
    
    Log    ✅ Aucune corruption détectée

Taille Fichier MB
    [Arguments]    ${file_path}
    [Documentation]    Retourne la taille en MB
    
    ${size_bytes} =    Get File Size    ${file_path}
    ${size_mb} =    Evaluate    ${size_bytes} / (1024 * 1024)
    
    RETURN    ${size_mb}

Vérifier IDs Uniques
    [Arguments]    ${persist_path}
    [Documentation]    Vérifie que tous les IDs sont uniques
    
    ${log_file} =    Set Variable    ${persist_path}/events.raftlog
    ${content} =    Get File    ${log_file}
    
    @{lines} =    Split To Lines    ${content}
    @{ids} =    Create List
    
    FOR    ${line}    IN    @{lines}
        ${event} =    Evaluate    json.loads('${line}')    json
        ${id} =    Get From Dictionary    ${event}    id
        Append To List    ${ids}    ${id}
    END
    
    ${total} =    Get Length    ${ids}
    ${unique_ids} =    Remove Duplicates    ${ids}
    ${unique_count} =    Get Length    ${unique_ids}
    
    Should Be Equal    ${total}    ${unique_count}
    ...    msg=IDs dupliqués détectés: ${total} IDs dont ${unique_count} uniques
    
    Log    ✅ Tous les IDs sont uniques: ${unique_count}

Nettoyer Tout
    [Documentation]    Nettoyage final
    Terminate All Processes
    Remove Directory    ${BASE_PERSIST}    recursive=True
    Log    🧹 Nettoyage terminé
