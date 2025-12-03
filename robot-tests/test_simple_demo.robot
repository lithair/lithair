*** Settings ***
Library           OperatingSystem
Library           Collections

*** Variables ***
${TEST_DIR}       /tmp/robot-test-demo

*** Test Cases ***
Test Simple - Créer et Vérifier Fichier
    [Documentation]    Test basique pour montrer les logs détaillés
    [Tags]    demo    simple
    
    Log    🚀 Début du test    console=yes
    Log    Step 1: Créer le répertoire    console=yes
    Create Directory    ${TEST_DIR}
    Directory Should Exist    ${TEST_DIR}
    
    Log    Step 2: Créer 10 fichiers    console=yes
    FOR    ${i}    IN RANGE    10
        ${filename} =    Set Variable    ${TEST_DIR}/file-${i}.txt
        Create File    ${filename}    Content for file ${i}
        Log    ✅ Fichier ${i} créé    console=yes
    END
    
    Log    Step 3: Vérifier qu'on a 10 fichiers    console=yes
    @{files} =    List Files In Directory    ${TEST_DIR}
    ${count} =    Get Length    ${files}
    Log    Nombre de fichiers trouvés: ${count}    console=yes
    Should Be Equal As Integers    ${count}    10
    
    Log    Step 4: Nettoyer    console=yes
    Remove Directory    ${TEST_DIR}    recursive=True
    
    Log    ✅ Test terminé avec succès!    console=yes

Test Avec Assertions
    [Documentation]    Test avec plusieurs assertions
    [Tags]    demo    assertions
    
    Log    📊 Test des assertions    console=yes
    
    ${value} =    Set Variable    42
    Log    Valeur testée: ${value}    console=yes
    Should Be Equal As Integers    ${value}    42
    Log    ✅ Assertion 1 OK    console=yes
    
    Should Be True    ${value} > 10
    Log    ✅ Assertion 2 OK    console=yes
    
    Should Be True    ${value} < 100
    Log    ✅ Assertion 3 OK    console=yes
    
    Log    ✅ Toutes les assertions passent!    console=yes

Test Avec Variables
    [Documentation]    Test manipulation de variables
    [Tags]    demo    variables
    
    Log    📝 Test des variables    console=yes
    
    @{liste} =    Create List    item1    item2    item3
    Log    Liste créée: ${liste}    console=yes
    
    ${longueur} =    Get Length    ${liste}
    Log    Longueur: ${longueur}    console=yes
    Should Be Equal As Integers    ${longueur}    3
    
    Append To List    ${liste}    item4
    ${nouvelle_longueur} =    Get Length    ${liste}
    Log    Nouvelle longueur: ${nouvelle_longueur}    console=yes
    Should Be Equal As Integers    ${nouvelle_longueur}    4
    
    Log    ✅ Variables OK!    console=yes
