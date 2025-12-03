*** Settings ***
Library           OperatingSystem
Library           Collections
Library           String
Library           Process

*** Variables ***
${TEST_FILE}      /tmp/robot-demo-test.txt
${TEST_CONTENT}   Hello RaftStone from Robot Framework!

*** Test Cases ***
Demo 1: Manipulation de Fichiers (Keywords Prédéfinis)
    [Documentation]    Montre les keywords prédéfinis pour fichiers
    [Tags]    demo    files
    
    Log    🚀 Test 1: Créer un fichier
    Create File    ${TEST_FILE}    ${TEST_CONTENT}
    
    Log    ✅ Test 2: Vérifier que le fichier existe
    File Should Exist    ${TEST_FILE}
    
    Log    📖 Test 3: Lire le fichier
    ${content} =    Get File    ${TEST_FILE}
    Log    Contenu lu: ${content}
    
    Log    🔍 Test 4: Vérifier le contenu
    Should Contain    ${content}    RaftStone
    Should Contain    ${content}    Robot Framework
    
    Log    🗑️ Test 5: Supprimer le fichier
    Remove File    ${TEST_FILE}
    File Should Not Exist    ${TEST_FILE}
    
    Log    ✅ Tous les keywords ont fonctionné sans écrire de code!

Demo 2: Assertions et Variables (Keywords Prédéfinis)
    [Documentation]    Montre les assertions et manipulations
    [Tags]    demo    assertions
    
    Log    🔢 Test avec nombres
    ${nombre} =    Set Variable    42
    Should Be Equal As Integers    ${nombre}    42
    Should Be True    ${nombre} > 10
    
    Log    📝 Test avec strings
    ${texte} =    Set Variable    RaftStone est génial!
    Should Contain    ${texte}    RaftStone
    Should Start With    ${texte}    RaftStone
    ${longueur} =    Get Length    ${texte}
    Should Be True    ${longueur} > 10
    
    Log    📋 Test avec listes
    ${liste} =    Create List    item1    item2    item3
    Length Should Be    ${liste}    3
    List Should Contain Value    ${liste}    item2
    Append To List    ${liste}    item4
    Length Should Be    ${liste}    4
    
    Log    📦 Test avec dictionnaires
    ${dict} =    Create Dictionary    name=RaftStone    version=1.0    status=awesome
    Dictionary Should Contain Key    ${dict}    name
    ${value} =    Get From Dictionary    ${dict}    status
    Should Be Equal    ${value}    awesome
    
    Log    ✅ Toutes les assertions ont fonctionné!

Demo 3: Process et Commandes (Keywords Prédéfinis)
    [Documentation]    Montre l'exécution de commandes système
    [Tags]    demo    process
    
    Log    💻 Test 1: Exécuter une commande simple
    ${result} =    Run Process    echo    Hello from Robot!
    Should Be Equal As Integers    ${result.rc}    0
    Should Contain    ${result.stdout}    Hello from Robot
    
    Log    📂 Test 2: Lister des fichiers
    ${result} =    Run Process    ls    -la    /tmp
    Should Be Equal As Integers    ${result.rc}    0
    Log    Résultat ls: ${result.stdout}
    
    Log    🔍 Test 3: Vérifier Rust est installé
    ${result} =    Run Process    rustc    --version
    Should Be Equal As Integers    ${result.rc}    0
    Should Contain    ${result.stdout}    rustc
    Log    Version Rust: ${result.stdout}
    
    Log    ✅ Toutes les commandes ont fonctionné!

Demo 4: Workflow Complet Simulé
    [Documentation]    Simule un workflow de test complet
    [Tags]    demo    workflow
    
    Log    📝 Étape 1: Préparer l'environnement
    ${work_dir} =    Set Variable    /tmp/robot-workflow-test
    Create Directory    ${work_dir}
    Directory Should Exist    ${work_dir}
    
    Log    📄 Étape 2: Créer un fichier de config
    ${config} =    Set Variable    [server]\nport = 8080\nenabled = true
    Create File    ${work_dir}/config.toml    ${config}
    File Should Exist    ${work_dir}/config.toml
    
    Log    📄 Étape 3: Créer un fichier de données
    Create File    ${work_dir}/data.json    {"articles": [{"title": "Test"}]}
    ${json_content} =    Get File    ${work_dir}/data.json
    Should Contain    ${json_content}    Test
    
    Log    🔍 Étape 4: Vérifier le contenu
    ${config_content} =    Get File    ${work_dir}/config.toml
    Should Contain    ${config_content}    port = 8080
    
    ${data_content} =    Get File    ${work_dir}/data.json
    Should Contain    ${data_content}    articles
    
    Log    📊 Étape 5: Compter les fichiers
    @{files} =    List Files In Directory    ${work_dir}
    ${count} =    Get Length    ${files}
    Should Be Equal As Integers    ${count}    2
    Log    Fichiers trouvés: ${files}
    
    Log    🗑️ Étape 6: Nettoyer
    Remove Directory    ${work_dir}    recursive=True
    Directory Should Not Exist    ${work_dir}
    
    Log    ✅ Workflow complet exécuté avec succès!

*** Keywords ***
# Tu peux aussi définir tes propres keywords réutilisables
Mon Keyword Custom
    [Documentation]    Exemple de keyword custom (optionnel)
    Log    🎯 Ceci est un keyword custom réutilisable
    ${timestamp} =    Get Time    epoch
    RETURN    ${timestamp}
