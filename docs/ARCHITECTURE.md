# Architecture — BezotCorp AI Platform

## 1. Objectif

BezotCorp AI Platform est une plateforme locale d'agents de développement dont le backend est écrit en Rust. Elle utilise actuellement Ollama et doit pouvoir accueillir d'autres fournisseurs de modèles.

Elle permet à un ou plusieurs agents de travailler sur un projet logiciel avec :

- un accès contrôlé aux fichiers ;
- des outils de lecture et d'écriture ;
- une gestion centralisée du contexte ;
- une mémoire conversationnelle persistante ;
- des sessions sauvegardées ;
- une orchestration mono-agent, MoA, supervisée ou populationnelle ;
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

Les différents modes d'exécution utilisent les mêmes services de contexte, de mémoire, d'exécution et d'autorisation.
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
| `agents/orchestration/` | Modes d'exécution, supervision et populations                 |
| `agents/context/`       | Récupération, sélection, assemblage et compaction du contexte |
| `agents/memory/`        | Mémoire conversationnelle persistante                         |
| `tools/`                | Outils natifs, registre et autorisations                      |
| `sessions/`             | Messages, historiques et sessions persistantes                |
| `sqlite/`               | Infrastructure SQLite générique                               |
| `websocket/`            | Commandes, connexions et serveur WebSocket                    |

Les fichiers `event.rs` et `file_manager.rs` sont situés directement dans `backend/src/`.
L'infrastructure SQLite est indépendante des schémas métier. Les opérations propres à la mémoire et aux sessions restent dans leurs modules respectifs.

## 4. Agents et orchestration

### 4.1. Configuration canonique

`AgentConfig` décrit l'identité, le rôle, les instructions,
le fournisseur et le modèle d'un agent.

`ExecutionMode` constitue l'entrée commune du moteur.
Il distingue un agent unique d'une architecture multi-agents.

Les configurations métier prennent en charge Serde.
Elles ne dépendent pas du protocole WebSocket.

### 4.2. Stratégies multi-agents

`MultiAgentStrategy` comporte actuellement trois stratégies :

- `LayeredMoa` : plusieurs couches d'agents, suivies
  d'un agrégateur final ;
- `Supervised` : un superviseur choisit les travailleurs,
  leur délègue des tâches et décide quand conclure ;
- `Population` : plusieurs agents collaborent pendant
  plusieurs tours, puis un facilitateur produit la synthèse.

Les identifiants des participants sont validés pour
éviter les doublons dans une même architecture.

### 4.3. Population collaborative

Une population possède entre deux et quatre agents,
un facilitateur distinct et entre un et quatre tours.

Sa politique de participation lui appartient :

- `Fixed` : tous les agents participent à chaque tour ;
- `Adaptive` : le facilitateur sélectionne les agents
  de chaque tour entre `min_agents` et `max_agents`.

Chaque agent conserve son propre historique pendant
l'exécution. Les contributions du tour précédent
sont transmises aux participants du tour suivant
comme informations non vérifiées.

Les contributions échangées sont limitées à
240 caractères par agent. Le facilitateur reçoit
les contributions du dernier tour pour sa synthèse.

Les historiques individuels ne sont pas encore
persistés entre plusieurs exécutions.

La sélection adaptative utilise les rôles disponibles
et des rapports récents limités en taille.
Elle ne constitue pas un mécanisme évolutionnaire.

### 4.4. Supervision autonome

Le superviseur peut déléguer successivement plusieurs
tâches aux travailleurs configurés, examiner leurs
rapports et produire une réponse finale.

Chaque travailleur conserve son historique pendant
l'exécution. Les délégations et les décisions
restent limitées par la configuration.

Il n'existe pas encore de délégation hiérarchique
permettant à un superviseur de lancer une population
ou un autre superviseur comme sous-architecture.

### 4.5. Exécution et ressources

Les générations sont séquentielles. Un sémaphore
global limite à une le nombre d'exécutions utilisant
le GPU simultanément dans un processus backend.

`AgentExecution` choisit le moteur adapté au mode.
`AgentTurn` réalise la génération et les cycles
d'utilisation des outils.

Le contexte, les outils, les approbations,
l'annulation et la mémoire sont mutualisés.

Les contributions des autres agents et les souvenirs
restent des données non fiables. Ils ne remplacent
jamais une vérification des fichiers réels.

### 4.6. Capacités non implémentées

Les populations évolutionnaires, les mutations,
la sélection intergénérationnelle, les échanges
directs entre agents hors des tours collaboratifs
et la persistance des historiques individuels
restent à développer.

## 5. Fournisseurs de modèles

Ollama est le fournisseur actuellement intégré.
Le client HTTP est asynchrone et transmet les réponses en streaming au backend.
La plateforme peut découvrir les modèles disponibles et configurer les paramètres de contexte et de génération.
Les valeurs configurées ne prouvent pas que le modèle sélectionné accepte réellement la fenêtre de contexte demandée. La découverte et la validation systématique de ces capacités restent à développer.
L'architecture prévoit l'ajout d'autres fournisseurs sans réimplémenter les services d'orchestration, de contexte et d'autorisation.
Un modèle ne prenant pas en charge les appels d'outils ne doit pas provoquer l'exécution silencieuse d'une action non reconnue.

## 6. Gestion du contexte

Le gestionnaire de contexte est commun à toutes les stratégies d'exécution.
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
La commande run.start reçoit les messages de la conversation. Leur enregistrement reste une opération distincte, effectuée par session.save.
Le futur frontend devra gérer explicitement cette sauvegarde.
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
- `orchestration.deciding` ;
- `orchestration.delegated` ;
- `orchestration.reported` ;
- `population.round.started` ;
- `population.round.completed` ;
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
Il devra également prendre en charge les modes mono-agent, MoA, supervisé et populationnel, les conversations persistantes, le streaming des réponses, les événements d'exécution et les demandes d'autorisation.
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

Les fonctionnalités suivantes restent à développer :

- le frontend graphique et son lanceur ;
- les clients et adaptateurs MCP ;
- les configurations permanentes des agents,
  superviseurs, MoA et populations ;
- la gestion des comptes utilisateurs ;
- la récupération sémantique ;
- le comptage exact des tokens selon les modèles ;
- la découverte des capacités réelles des modèles ;
- la reprise des événements après reconnexion ;
- la reprise d'une exécution après redémarrage ;
- la sauvegarde automatique des sessions ;
- les historiques d'agents persistants entre exécutions ;
- les populations évolutionnaires ;
- la composition hiérarchique des architectures ;
- la politique de rétention et de sauvegarde ;
- la gestion avancée de la résidence GPU.

Les stratégies actuelles compilent, mais leur comportement
avec les différents modèles Ollama reste à valider
en conditions réelles.

## 15. Communication et persistance

### 15.1. Structures Rust

Les configurations d'agents, les modes d'exécution
et les populations sont des types métier Rust.

Les modules internes échangent directement
ces structures en mémoire. Aucune sérialisation
n'est nécessaire pour ces échanges.

Serde permet leur sérialisation et leur
désérialisation sans imposer un format unique.

La compatibilité effective avec RON ou un format binaire devra être vérifiée avant leur adoption. La prise en charge de Serde ne garantit pas que toutes les représentations des types métier soient compatibles avec tous les formats.

### 15.2. Communication

Le transport WebSocket actuel utilise JSON.
Il transmet directement `ExecutionMode` dans
les demandes `run.start`, sans structures `Spec`
spécifiques au transport.

Le fournisseur Ollama utilise également
son protocole HTTP JSON.

Le format du transport peut évoluer
indépendamment des structures métier.

### 15.3. SQLite

La mémoire conversationnelle et les sessions
utilisent une infrastructure SQLite commune.

`rusqlite` est compilé avec la fonctionnalité
`bundled`. Le déploiement du backend n'exige
donc pas l'installation séparée de SQLite.

La base reste facultative. Elle est ouverte
lorsque `AI_PLATFORM_MEMORY_DB` est configurée.
Les sessions utilisent cette même base.

La mémoire et les sessions sont actuellement
cloisonnées par projet. Aucune gestion de
comptes utilisateurs n'est implémentée.

Les messages des sessions sont enregistrés
sous forme de JSON dans une colonne SQLite.
La mémoire possède ses propres colonnes métier.

### 15.4. Configurations persistantes

Les configurations des agents, des superviseurs,
des MoA et des populations ne possèdent pas encore
de mécanisme de sauvegarde permanent.

Le prochain chantier doit permettre de créer,
charger, modifier, lister et supprimer ces
configurations, avec contrôle des révisions
et cloisonnement adapté.

Le choix entre stockage SQLite et fichiers
portables RON reste distinct du format
des communications WebSocket.

Aucun stockage binaire de ces configurations
n'est encore implémenté.

### 15.5. Frontières du runtime

Les configurations persistantes ne doivent
pas être confondues avec les données temporaires
d'une exécution : historiques individuels,
rapports, contributions et décisions.

Un éventuel mécanisme de reprise devra
préciser quelles données temporaires conserver,
comment les versionner et quand les supprimer.
