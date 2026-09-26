# Architecture — BezotCorp AI Platform

## 1. Objectif

BezotCorp AI Platform est une plateforme locale d'agents de développement dont le backend est écrit en Rust. Elle utilise actuellement Ollama et doit pouvoir accueillir d'autres fournisseurs de modèles.

Elle permet à un ou plusieurs agents de travailler sur un projet logiciel avec :

- un accès contrôlé aux fichiers ;
- des outils de lecture et d'écriture ;
- une gestion centralisée du contexte ;
- une mémoire conversationnelle persistante ;
- des sessions sauvegardées ;
- une orchestration mono-agent ou MoA ;
- une interface de communication WebSocket.

L'architecture vise notamment les machines disposant d'au plus 16 Go de VRAM. L'exécution séquentielle, la sélection du contexte et la persistance limitent les besoins matériels sans remplacer les capacités d'un modèle plus grand.

## 2. Principes fondamentaux

### 2.1. Source de vérité

Le contenu réel des fichiers et leur état Git constituent les sources de vérité du projet.
Les réponses des modèles, les propositions d'autres agents et les souvenirs sont des informations susceptibles d'être incomplètes ou obsolètes.
Les données mémorisées doivent conserver leur provenance et, lorsque cela est pertinent, une version ou une révision. Les informations qui nécessitent une vérification doivent être confrontées à leur source originale.
Un agent ne doit pas considérer un résumé comme une preuve de l'état actuel d'un fichier.

### 2.2. Responsabilités du backend

Le backend Rust applique de manière déterministe :

- la validation des appels d'outils ;
- les permissions ;
- les limites de ressources ;
- les budgets de contexte ;
- les contrôles de version ;
- la coordination des écritures ;
- l'annulation des exécutions ;
- l'orchestration des agents.

Les modèles demandent des actions, mais ne déterminent pas leurs propres autorisations.
Les données provenant des fichiers, de la mémoire, des outils et des futurs serveurs MCP sont considérées comme non fiables. Elles ne doivent jamais être promues en instructions système.

### 2.3. Services communs

Les modes mono-agent et MoA utilisent les mêmes services de contexte, de mémoire, d'exécution et d'autorisation.
Le gestionnaire de contexte appartient au backend et reste indépendant du fournisseur IA, du transport WebSocket, du stockage et de MCP.
Les futurs adaptateurs MCP devront réutiliser les services internes du backend sans dupliquer les règles métier.

## 3. Organisation du projet

Le projet comprend trois parties :

- `backend/` : moteur Rust et services internes ;
- `frontend/` : future application graphique ;
- `mcp/` : futures intégrations MCP.

Le backend est un exécutable Rust dont le point d'entrée est `backend/src/main.rs`.

Ses principaux modules sont :

| Module                  | Responsabilité                                                |
| ----------------------- | ------------------------------------------------------------- |
| `providers/`            | Fournisseurs de modèles et Ollama                             |
| `agents/`               | Configuration et exécution des agents                         |
| `agents/orchestration/` | Modes mono-agent et MoA, couches et agrégation                |
| `agents/context/`       | Récupération, sélection, assemblage et compaction du contexte |
| `agents/memory/`        | Mémoire conversationnelle persistante                         |
| `tools/`                | Outils natifs, registre et autorisations                      |
| `sessions/`             | Messages, historiques et sessions persistantes                |
| `sqlite/`               | Infrastructure SQLite générique                               |
| `websocket/`            | Commandes, connexions et serveur WebSocket                    |

Les fichiers `event.rs` et `file_manager.rs` sont situés directement dans `backend/src/`.
L'infrastructure SQLite est indépendante des schémas métier. Les opérations propres à la mémoire et aux sessions restent dans leurs modules respectifs.

## 4. Agents et orchestration

### 4.1. Configuration

`AgentConfig` constitue la configuration canonique d'un agent.
Chaque agent possède une identité, un rôle, des instructions, un fournisseur et un modèle.
Plusieurs agents peuvent utiliser le même modèle avec des rôles et des instructions différents.
Les identifiants des participants d'une même exécution doivent être uniques.

### 4.2. Modes d'exécution

Deux modes partagent le même moteur :

- `ExecutionMode::Single` : exécution d'un agent ;
- `ExecutionMode::Mixture` : exécution de plusieurs couches d'agents suivies d'un agrégateur final.

Le MoA (_Mixture of Agents_) orchestre des agents complets. Il ne s'agit pas d'un MoE : le backend ne contrôle pas les experts neuronaux internes des modèles.
Les agents d'une couche peuvent produire des contributions destinées à la couche suivante. Ces contributions restent non vérifiées tant qu'elles ne sont pas confrontées aux sources originales.

### 4.3. Exécution séquentielle

Le planificateur détermine l'ordre des couches.
Les agents sont actuellement exécutés séquentiellement. Un sémaphore global limite les générations à une exécution simultanée par processus backend.
Cette organisation permet de réutiliser un modèle entre plusieurs agents et évite d'imposer le chargement simultané de plusieurs modèles en VRAM.

### 4.4. Répartition des responsabilités

`AgentExecution` orchestre les couches, transmet leurs contributions et publie le résultat final. Il sollicite également l'enregistrement de la réponse finale dans la mémoire conversationnelle, lorsque celle-ci est activée.
`AgentTurn` prend en charge l'exécution d'un agent : préparation du contexte, communication avec le fournisseur, streaming et cycles de génération avec outils.
`ToolInvocation` centralise les appels d'outils, leurs aperçus, les demandes d'autorisation et la publication de leurs résultats.
Les règles d'exécution des outils sont communes aux modes mono-agent et MoA.

## 5. Fournisseurs de modèles

Ollama est le fournisseur actuellement intégré.
Le client HTTP est asynchrone et transmet les réponses en streaming au backend.
La plateforme peut découvrir les modèles disponibles et configurer les paramètres de contexte et de génération.
Les valeurs configurées ne prouvent pas que le modèle sélectionné accepte réellement la fenêtre de contexte demandée. La découverte et la validation systématique de ces capacités restent à développer.
L'architecture prévoit l'ajout d'autres fournisseurs sans réimplémenter les services d'orchestration, de contexte et d'autorisation.
Un modèle ne prenant pas en charge les appels d'outils ne doit pas provoquer l'exécution silencieuse d'une action non reconnue.

## 6. Gestion du contexte

Le gestionnaire de contexte est commun aux modes mono-agent et MoA.
Il assemble les instructions de l'agent, la demande utilisateur, les messages historiques pertinents, les contributions des couches précédentes, les souvenirs récupérés et les échanges avec les outils.

### 6.1. Sélection et provenance

Le dernier message utilisateur, les instructions du rôle et les contributions MoA obligatoires sont conservés.
Les messages historiques facultatifs sont sélectionnés selon leur pertinence lexicale et leur récence, puis replacés dans leur ordre chronologique d'origine.
Les souvenirs persistants peuvent compléter ce contexte. Ils restent des informations historiques non vérifiées.
Les événements `context.prepared` exposent les informations de sélection et de provenance utiles au frontend.
La sélection actuelle est lexicale. Elle ne constitue pas une recherche sémantique et ne garantit pas la découverte de toutes les dépendances d'un projet.

### 6.2. Budget

Le budget de contexte réserve de l'espace pour les instructions, les définitions d'outils et la génération.
Si les éléments obligatoires dépassent le budget disponible, la génération est refusée sans troncature silencieuse.
Le comptage actuel repose sur une estimation liée aux octets UTF-8. Il ne constitue pas un comptage exact des tokens du modèle.

### 6.3. Compaction

`ToolContext` conserve les échanges complets avec les outils et prépare le contexte avant chaque nouvelle génération.
Lorsque le budget est saturé, il peut condenser d'anciens tours de lecture terminés et retirer des échanges historiques facultatifs.
Les échanges contenant une écriture ne sont pas compactés.
Les registres compacts conservent les métadonnées utiles, mais ne remplacent pas le contenu original. Un agent doit récupérer à nouveau les données absentes avant toute opération qui nécessite leur vérification.
L'événement `context.compacted` expose les compactions et omissions réalisées.
Si les données obligatoires ou les échanges d'écriture excèdent encore la fenêtre disponible, l'exécution échoue explicitement.

## 7. Outils et accès au projet

### 7.1. Outils natifs

Les outils de lecture actuellement disponibles sont :

- `project.list_files` ;
- `project.read_file` ;
- `project.search_text`.

Les outils d'écriture sont :

- `project.replace_text` ;
- `project.create_file`.

Le nombre de tours de génération et le nombre total d'appels d'outils sont bornés.
Aucun outil de suppression de fichier ni aucune exécution arbitraire de commandes système ne sont actuellement exposés aux modèles.

### 7.2. Accès aux fichiers

Le répertoire autorisé est fourni au backend par `AI_PLATFORM_PROJECT_ROOT`.
Les outils utilisent `FileManager` et `cap-std` pour accéder aux fichiers relativement à ce répertoire.
Les traversées interdites, certains répertoires sensibles et les liens symboliques rencontrés pendant l'exploration sont refusés.
Les lectures et recherches sont soumises à des limites de taille et de nombre de résultats.
Ces protections applicatives ne constituent pas un sandbox complet du processus et ne suppriment pas toutes les courses possibles avec des processus extérieurs.

### 7.3. Autorisations

Les lectures sont autorisées automatiquement par défaut. `AI_PLATFORM_APPROVE_READS=1` permet d'exiger une autorisation explicite pour chacune d'elles.
Toute écriture exige une approbation explicite.
Le backend prépare la modification, publie son aperçu avec `tool.preview` puis émet `approval.required`.
Le frontend transmet sa décision avec `approval.resolve`. Lorsqu'une écriture est concernée, l'autorisation est liée à l'empreinte de l'aperçu présenté.
L'approbation porte exclusivement sur la proposition préparée.

### 7.4. Publication des écritures

`project.replace_text` exige l'empreinte SHA-256 du fichier original et une occurrence unique du texte à remplacer.
`project.create_file` refuse de remplacer une destination existante.
Un verrou partagé coordonne les écritures réalisées par le backend.
La version du fichier est recontrôlée avant publication. Les fichiers temporaires sont créés dans le répertoire de destination et synchronisés avant leur publication atomique.
Ces contrôles ne garantissent pas l'absence absolue de courses avec un processus extérieur.

## 8. Infrastructure SQLite

Le module `sqlite/` fournit une infrastructure générique indépendante des domaines métier.

Il comprend :

- une connexion d'écriture ;
- des connexions de lecture dédiées ;
- des opérations exécutées hors des threads asynchrones de Tokio ;
- des transactions ;
- le mode WAL ;
- une vérification d'intégrité à l'ouverture ;
- un état de santé partagé ;
- une coordination de l'arrêt.

Les modules métier transmettent leurs opérations à cette infrastructure.
La mémoire conversationnelle et les sessions réutilisent le même gestionnaire lorsqu'elles partagent une base.
Le gestionnaire doit tenir compte des opérations engagées et des références actives lors de sa fermeture.

## 9. Mémoire conversationnelle

La mémoire persistante est facultative. Elle est activée en fournissant `AI_PLATFORM_MEMORY_DB`.
Le chemin doit être absolu et situé hors du répertoire du projet autorisé. Son répertoire parent doit déjà exister.
La base n'est actuellement ni chiffrée ni synchronisée avec un service distant.
Les souvenirs sont isolés par projet. Chaque entrée contient notamment une question, un contenu, une source, une empreinte, une révision et des horodatages.
Après une exécution réussie, la mémoire peut enregistrer la dernière question utilisateur et la réponse finale.
Avant la génération de chaque agent, elle recherche des souvenirs pertinents pour la demande et les instructions du rôle.
La récupération actuelle est lexicale. Elle n'utilise ni embeddings ni index vectoriel.
Les souvenirs sont présentés comme des données historiques non vérifiées. Ils ne constituent jamais une preuve de l'état actuel du code.
Une erreur de récupération ou d'enregistrement est signalée par `memory.failed`, sans transformer automatiquement une réponse déjà produite en échec.
La politique de rétention, la sauvegarde et la synchronisation restent à définir.

## 10. Sessions et conversations

Les sessions permettent de conserver les conversations entre plusieurs utilisations de la plateforme.
`Message` représente un message conversationnel.
`History` regroupe les messages d'une session et centralise leur validation.
`Session` contient l'identifiant, la révision et les horodatages.
`SessionStore` assure l'enregistrement, la récupération, la liste et la suppression des sessions.
Les sessions utilisent l'infrastructure SQLite commune, avec leur propre schéma métier. Elles sont isolées par projet.

### 10.1. Contrôle des révisions

La création d'une session attend la révision `0`.
La modification d'une session existante exige sa révision actuelle. Une révision obsolète provoque un conflit explicite plutôt qu'un écrasement silencieux.
La suppression exige également la révision attendue.
Les opérations de liste sont bornées.

### 10.2. Intégration aux exécutions

Les sessions et la mémoire conversationnelle remplissent des fonctions distinctes.
Une session conserve un historique explicite. La mémoire conserve des informations réutilisables entre les exécutions.
Le frontend fournit actuellement les messages à `run.start` et demande explicitement leur enregistrement.
La sauvegarde automatique des exécutions interrompues et leur reprise après redémarrage ne sont pas implémentées.
La persistance des sessions dépend actuellement de l'activation de la base SQLite facultative.

## 11. Transport WebSocket

Le frontend doit lancer le backend comme processus enfant.
Le backend écoute exclusivement sur `127.0.0.1`, sur un port attribué par le système. Il publie son URL WebSocket sur stdout sous forme d'une ligne JSON, sans exposer le jeton secret.
Il vérifie l'origine des connexions et exige une authentification avant toute commande applicative.

### 11.1. Configuration

Les variables de lancement comprennent :

- `AI_PLATFORM_TOKEN` : jeton secret ;
- `AI_PLATFORM_ORIGIN` : origine autorisée ;
- `AI_PLATFORM_PROJECT_ROOT` : répertoire du projet ;
- `OLLAMA_HOST` : adresse facultative du serveur Ollama ;
- `AI_PLATFORM_MEMORY_DB` : base SQLite facultative ;
- `AI_PLATFORM_APPROVE_READS` : approbation facultative des lectures ;
- `AI_PLATFORM_NUM_CTX` : fenêtre de contexte demandée ;
- `AI_PLATFORM_NUM_PREDICT` : budget de génération demandé.

### 11.2. Commandes

Le protocole JSON comprend :

- `authenticate` ;
- `models.list` ;
- `run.start` ;
- `run.cancel` ;
- `approval.resolve` ;
- `session.save` ;
- `session.load` ;
- `session.list` ;
- `session.delete`.

### 11.3. Événements

Les principaux événements sont :

- `authenticated` ;
- `models.list` ;
- `run.queued` ;
- `run.started` ;
- `agent.started` ;
- `agent.delta` ;
- `agent.completed` ;
- `tool.requested` ;
- `tool.preview` ;
- `approval.required` ;
- `approval.resolved` ;
- `tool.completed` ;
- `tool.failed` ;
- `context.prepared` ;
- `context.compacted` ;
- `memory.failed` ;
- `run.completed` ;
- `run.failed` ;
- `run.cancelled` ;
- `session.saved` ;
- `session.loaded` ;
- `session.list` ;
- `session.deleted` ;
- `session.failed` ;
- `error`.

Les identifiants de corrélation permettent de rattacher les événements aux demandes concernées.
Les autorisations des outils sont isolées par connexion WebSocket.
Le protocole ne garantit pas encore la reprise des événements après une déconnexion.

## 12. Frontend

Le frontend graphique reste à développer.
Il devra permettre de lancer et arrêter le backend, configurer son environnement, s'authentifier, afficher les modèles disponibles et configurer les agents.
Il devra également prendre en charge les modes mono-agent et MoA, les conversations persistantes, le streaming des réponses, les événements d'exécution et les demandes d'autorisation.
Les aperçus d'écriture devront être présentés avant toute décision d'approbation.
Les déconnexions, les annulations et la fermeture du backend devront être gérées explicitement.
Le frontend ne doit pas contourner les validations du backend.

## 13. MCP

Les clients MCP ne sont pas encore raccordés au moteur d'agents.
Leur intégration devra permettre de configurer plusieurs serveurs, découvrir leurs outils, valider leurs schémas et exécuter leurs opérations de manière contrôlée.
Un serveur MCP ne pourra pas augmenter les permissions de l'agent appelant.
Les contenus qu'il renvoie resteront des données non fiables.
Un adaptateur exposant certains services internes à des clients MCP externes pourra être ajouté si nécessaire.

## 14. Limites techniques

Les fonctionnalités suivantes restent à développer ou à évaluer :

- le frontend graphique et son lanceur ;
- les clients MCP ;
- la récupération sémantique et l'indexation vectorielle ;
- le comptage exact des tokens selon les modèles ;
- la découverte systématique des capacités réelles des modèles ;
- la reprise des événements après reconnexion ;
- la reprise d'une exécution après redémarrage ;
- la sauvegarde automatique des sessions ;
- la politique de rétention et de sauvegarde ;
- la gestion avancée de la résidence GPU ;
- les formes d'orchestration plus élaborées que le MoA séquentiel.

Ces fonctionnalités ne sont pas considérées comme implémentées tant qu'elles ne sont pas effectivement intégrées au backend.

## Diversité des stratégies multi-agents — contrat initial (26 septembre 2026)

Les sections précédentes sont des jalons historiques ; les anciennes listes de fonctionnalités manquantes ne constituent pas l'état actuel. Le nouveau contrat distingue `Single` et `MultiAgent`, puis la stratégie de coordination et la politique de population. Cette évolution conserve le MoA en couches et ajoute une orchestration supervisée dynamique. Elle introduit également des types explicites pour les politiques de population futures, sans prétendre implémenter la collaboration décentralisée ni l'évolution des populations.

- `MultiAgentStrategy::LayeredMoa` : couches prédéfinies suivies d'un agrégateur, toujours exécutées séquentiellement afin de limiter la VRAM.
- `PopulationPolicy::Fixed` : seule politique exécutable actuellement.
- `PopulationPolicy::Adaptive` et `PopulationPolicy::Evolutionary` : choix distincts représentés dans le contrat mais explicitement refusés en attendant une implémentation réelle.
- La stratégie `MultiAgentStrategy::Supervised` réutilise le moteur d'agent commun pour déléguer dynamiquement des tâches à des travailleurs indépendants pendant une exécution. La collaboration décentralisée reste à développer.

Contrat WebSocket : `mode: { kind: "multi_agent", coordination: { kind: "layered_moa", layers: [...], aggregator: {...} }, population: { kind: "fixed" } }`. Le champ `population` est facultatif et vaut `fixed` par défaut. L'ancien `kind: "mixture"` est refusé sur cette nouvelle branche, avant le développement du frontend.

L'orchestration supervisée et ses limites réelles sont décrites dans `docs/SUPERVISED_ORCHESTRATION.md`.
