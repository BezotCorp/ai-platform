# Architecture — BezotCorp AI Platform

Statut : décisions de conception du 24 septembre 2026. Ce document distingue les invariants adoptés des options restant à valider. Il ne prétend pas que les fonctionnalités sont déjà implémentées.

## Objectif et limites

Construire un agent de développement Rust, utilisable avec des modèles locaux (notamment Ollama) et ultérieurement d'autres fournisseurs. Il doit fonctionner sur une machine dotée d'au plus 16 Go de VRAM : priorité aux modèles compatibles avec ce budget, à l'exécution séquentielle, aux recherches ciblées, aux caches et à la persistance du travail. **Ne jamais assimiler ces optimisations à 100 Go de VRAM ou à la qualité garantie d'un modèle plus grand.** Mesurer résultats et ressources.

## Principes non négociables

1. Un moteur d'agent commun aux modes mono-IA et MoA (Mixture of Agents), sélectionnables depuis le frontend. Un MoA orchestre des agents complets pouvant avoir des rôles, instructions, modèles, permissions et contextes différents. **Ce n'est pas un MoE** : aucun routage des experts neuronaux internes d'un modèle par notre orchestrateur.
2. Le modèle peut demander des outils ; seul le backend valide, autorise, exécute et retourne les résultats. Les outils proviennent du registre natif et/ou de clients MCP vers un ou plusieurs serveurs. Ne jamais supposer qu'un modèle sait appeler correctement les outils.
3. Le gestionnaire de contexte réside dans le backend Rust, indépendamment de la stratégie d'orchestration, du fournisseur IA, du stockage et de MCP. Un adaptateur MCP peut exposer ses opérations à des clients externes ; les composants internes l'appellent directement.
4. Une mémoire persistante commune est consultable à la demande. Chaque agent conserve son historique et son contexte de travail privés. L'orchestrateur conserve tâches, dépendances et références, **pas** nécessairement le contenu intégral des travaux de tous les agents.
5. Le code réel et la version Git sont les sources de vérité pour le contenu des fichiers. Une proposition ou un résumé d'agent n'est pas une vérité. Toute entrée de mémoire porte une provenance, une version/révision si pertinente, un statut (observation, hypothèse, proposition, décision ou obsolète), des permissions et un lien vers sa source originale.
6. Le prompt système est minimal mais doit toujours contenir le rôle, les contraintes essentielles de sécurité et le contrat des outils. Le contexte est sélectionné à la demande, selon la tâche et le budget de chaque modèle. **Minimal ne veut pas dire insuffisant** : possibilité de récupérer les extraits complets si nécessaire.
7. Ne jamais laisser une IA seule garantir autorisations, fraîcheur du contenu, intégrité des versions, budgets ou coordination des écritures : ces règles sont imposées par du Rust déterministe. Les modèles peuvent aider à reformuler la recherche, classer et synthétiser, mais leurs résultats restent vérifiables.
8. Les opérations destructrices, écritures sensibles et commandes nécessitent une politique d'autorisation explicite. Le contenu récupéré via fichiers ou MCP est une donnée non fiable, jamais une instruction système.

## Exécution

- `ExecutionMode::Single` : un agent, le moteur de contexte, le registre d'outils et les mêmes mécanismes de sécurité.
- `ExecutionMode::Mixture` : une ou plusieurs couches d'agents de proposition, suivies d'un agrégateur ; rôles et modèles configurables par agent. Les agents d'une couche peuvent travailler indépendamment, publier des résultats sourcés et consulter sélectivement les sorties antérieures ; l'agrégateur peut demander des vérifications.
- Une exécution MoA n'implique **pas** plusieurs modèles simultanément en VRAM. Le planificateur pourra exécuter les rôles successivement et réutiliser le même modèle ; les modèles différents pourront être chargés à tour de rôle.
- Les outils de lecture peuvent être exécutés en parallèle lorsque c'est sûr ; les modifications concurrentes d'une même ressource nécessitent coordination, validation de version et absence de pertes silencieuses.

## Context Engine

Pipeline : `requête + rôle + permissions + budget` -> récupération textuelle/symbolique (plus tard sémantique) -> filtrage d'accès et fraîcheur -> classement -> sélection sous budget -> assemblage avec provenance. Conserver une marge pour les outils et la réponse ; mesurer le nombre réel de tokens avec un tokenizer adapté au modèle quand disponible. Les heuristiques de longueur ne sont que des approximations explicites.

- Prioriser le code original et les diagnostics exacts pour toute correction ; les résumés servent à orienter la recherche et ne remplacent pas un extrait de code nécessaire.
- Une recherche qui ne trouve rien doit pouvoir retourner « aucune donnée » plutôt qu'une invention.
- Les résultats de compilation/tests sont datés et associés à une révision. Une modification ultérieure peut les rendre obsolètes.
- Rechercher par identifiants stables, chemins, symboles et références exactes ; ajouter ensuite recherche sémantique, reranking neuronal facultatif et index vectoriel via interfaces distinctes.
- Appliquer les plafonds sur résultats, octets/tokens, appels et temps. Une sélection tronquée doit être annoncée et permettre une récupération complémentaire.

## Stockage et accès IA

SQLite est le premier candidat pour la mémoire durable locale : migrations explicites, transactions courtes, WAL si adapté, intégrité et stratégie de sauvegarde. FTS5 pour la recherche textuelle, à vérifier dans le runtime retenu. Les fichiers Git restent hors de SQLite, référencés par chemin, révision et éventuellement empreinte. Une couche `MemoryStore` doit permettre de remplacer le stockage sans changer les agents.

Les IA n'exécutent pas de SQL arbitraire. Elles appellent des outils métier à schémas stricts : recherche, récupération ciblée, publication d'observation, consultation des décisions et historique. Le backend vérifie autorisations, tailles, accès et version. Les opérations MCP n'augmentent jamais les permissions de leur appelant.

## Incertitudes et décisions différées

- Fournisseur HTTP/framework, schémas JSON, client MCP Rust et versions de crates : à retenir après examen du code et des API actuelles.
- Embeddings, index vectoriel et modèle de reranking : seulement après mesure sur des tâches réelles et évaluation de leur coût VRAM.
- Modalités de coordination plus élaborées que le MoA en couches (délégation, reprises, négociation) : extensibles, non présumées implémentées.
- Politique fine des permissions, persistance des sessions, backend/frontend en streaming et reprise après crash : à spécifier avant mise en production.

## Jalons et critères de validation

1. **Contrats testés** : configuration mono/MoA, budgets de contexte, provenance, registre des outils ; tests unitaires sans modèle ni réseau.
2. **Premier chemin complet** : backend -> Ollama -> éventuelle demande d'outil -> vérification -> exécution -> retour au modèle ; prise en charge explicite des modèles sans tool calling.
3. **Mémoire persistante** : SQLite derrière `MemoryStore`, provenance, recherches et invalidation suite à modification de fichiers.
4. **MCP** : connexions configurées à plusieurs serveurs, découverte, validation des schémas, appel contrôlé ; serveur/adaptateur de contexte seulement si utile aux clients externes.
5. **MoA réel** : rôles distincts, couches, agrégateur et orchestration séquentielle, mêmes outils et moteur de contexte que le mode mono.
6. **Frontend** : choix du mode, des rôles/modèles, visibilité des appels et permissions ; traitement d'erreurs.
7. **Évaluation** : sur les mêmes tâches Rust, mesurer exactitude (tests), tokens, latence, pics de VRAM, collisions d'écriture et comportement en contexte insuffisant.

**Règle de travail :** toute modification d'architecture revoit ce document et les tests correspondants. Ne pas marquer « implémenté » ce qui n'est que prévu. Ne jamais pousser directement sur `main` ou `dev` : développement sur `feature/backend-agent`, puis PR vers `dev`.

## Arborescence du backend

Le backend est un exécutable Rust dont le point
d'entrée est `backend/src/main.rs`.

Les modules sont organisés par responsabilité :

- `providers` : fournisseurs de modèles et Ollama.
- `agents` : agents et orchestration mono-IA/MoA.
- `context` : récupération et assemblage du contexte.
- `memory` : mémoire persistante et stockage SQLite.
- `tools` : outils natifs, registre et permissions.
- `sessions` : sessions et conversations.
- `api` : interface du backend avec le frontend.

Les intégrations MCP pourront utiliser les services
communs du backend. Les dossiers racine `mcp/`
et `frontend/` restent disponibles pour leur
développement dans VS Code.

### Conventions Rust

- `main.rs` est le point d'entrée.
- Aucun `lib.rs` n'est prévu pour cet exécutable.
- Chaque fichier possède une responsabilité précise.
- Les noms des fichiers contenant une structure ou
  une énumération correspondent à leur nom Rust
  converti en snake_case.
- Les fichiers `mod.rs` déclarent leurs sous-modules.

### État

L'arborescence et les déclarations des modules sont
créées. Les fonctionnalités ne sont pas encore
implémentées.

Aucun test n'est créé.

Le code n'est pas considéré comme compilé ou validé
par cette opération.

## Configuration des agents — première implémentation

Le modèle est identifié par son fournisseur et son nom.
Un même modèle peut être partagé par plusieurs agents.

Chaque agent possède un identifiant unique dans son
exécution et un rôle configurable avec ses instructions.

Le mode mono-IA contient un agent.

Le mode MoA contient une ou plusieurs couches d'agents
et un agrégateur final. Les identifiants des participants
doivent être uniques. Plusieurs rôles peuvent utiliser
le même modèle.

Le planificateur fournit l'ordre logique des couches.
Il n'exécute pas encore les modèles.

Le budget de contexte distingue capacité, prompt
système, génération et réserve pour les outils.
Le comptage exact dépendra du fournisseur.

Cette première implémentation ne réalise pas encore
les appels Ollama, l'exécution MoA, MCP ou le stockage.

Aucun test n'a été ajouté. Compilation non vérifiée.

## Transport frontend — WebSocket

Le frontend démarre le binaire Rust. Le backend ne
propose aucune interface CLI, REST ou SSE destinée
à l'application.

Le point d'entrée `main.rs` démarre directement
le serveur WebSocket.

### Démarrage

Le frontend fournit les variables d'environnement :

- `AI_PLATFORM_TOKEN` : secret aléatoire fort.
- `AI_PLATFORM_ORIGIN` : origine exacte du frontend.
- `OLLAMA_HOST` : URL Ollama facultative.

Le backend écoute exclusivement sur `127.0.0.1`.

Le système attribue un port disponible.

Le backend écrit une ligne JSON contenant son URL
WebSocket sur stdout, pour son processus parent.

Le frontend doit s'authentifier avant toute commande.

### Protocole

Tous les messages applicatifs sont au format JSON.

Commandes :

- `authenticate`
- `models.list`
- `run.start`
- `run.cancel`

Événements :

- `authenticated`
- `models.list`
- `run.queued`
- `run.started`
- `agent.started`
- `agent.delta`
- `agent.completed`
- `run.completed`
- `run.failed`
- `run.cancelled`
- `error`

Les demandes d'exécution comprennent un identifiant
de corrélation et la configuration mono-IA ou MoA.

### Exécution

Le backend utilise le planificateur déjà défini.

En mode mono-IA, un seul agent est exécuté.

En mode MoA, les couches sont exécutées
successivement, puis l'agrégateur.

Les agents peuvent partager un modèle Ollama.

Un sémaphore global limite cette implémentation
à une génération simultanée par processus.

Le client HTTP Ollama est asynchrone.

Le streaming est retransmis au frontend par WebSocket.

Les appels d'outils non encore raccordés ne sont
jamais exécutés silencieusement.

### Limites actuelles

Cette étape ne termine pas l'application.

Ne sont pas encore raccordés :

- Le frontend graphique et son lanceur.
- Les serveurs MCP et les outils.
- Les autorisations détaillées des opérations.
- La mémoire SQLite persistante.
- La récupération intelligente du contexte.
- La reprise des événements après reconnexion.
- La gestion avancée de résidence GPU.

Les limites de taille du transport ne remplacent
pas un budget de contexte calculé en tokens.

Aucun test n'a été créé.

La compilation Rust doit être vérifiée séparément.

## Assemblage du contexte — première intégration

Le moteur d'exécution utilise ContextBudget avant
chaque génération Ollama.

Configuration facultative fournie au lancement
par le frontend :

- AI_PLATFORM_NUM_CTX : 4096 par défaut.
- AI_PLATFORM_NUM_PREDICT : 768 par défaut.

Ces paramètres sont transmis à Ollama via
num_ctx et num_predict.

L'assembleur préserve le dernier message utilisateur
et les propositions de la couche MoA précédente.

Il sélectionne les messages historiques récents
dans la limite du budget disponible.

Il refuse la génération lorsque les informations
obligatoires dépassent ce budget.

L'événement WebSocket context.prepared expose
les limites, le volume estimé et le nombre de
messages historiques retenus ou écartés.

Le comptage utilise une estimation basée sur
les octets UTF-8. Il ne s'agit pas d'un comptage
exact des tokens propre au modèle.

Les limites configurées ne prouvent pas que le
modèle sélectionné accepte réellement cette
fenêtre de contexte. La découverte de cette
capacité reste à raccorder.

La récupération depuis SQLite, les outils MCP
et l'indexation sémantique restent à implémenter.


## Outils natifs — première intégration

Trois outils de lecture sont maintenant disponibles :

- `project.list_files`
- `project.read_file`
- `project.search_text`

Le frontend doit fournir `AI_PLATFORM_PROJECT_ROOT`
au lancement du backend.

Le backend résout ce chemin et interdit aux outils
l'accès à des fichiers situés en dehors de ce projet.

Les chemins absolus, la traversée avec `..`,
les liens symboliques rencontrés pendant l'exploration
et plusieurs répertoires sensibles sont interdits.

Les recherches et lectures possèdent des limites
explicites de taille et de nombre de résultats.

Le modèle reçoit les définitions JSON des outils.
Le backend reçoit ses appels, valide leurs arguments,
exécute les opérations autorisées, lui retourne
les résultats, puis reprend la génération.

Le nombre de tours et d'appels d'outils est limité.
Les résultats sont traités comme des données
non fiables et ne sont pas promus en instructions.

### Autorisations WebSocket

La variable facultative `AI_PLATFORM_APPROVE_READS=1`
impose une autorisation avant chaque outil de lecture.

Le backend émet `approval.required` avec le
`request_id`, le `call_id`, l'agent, le nom de l'outil
et les arguments exacts.

Le frontend répond :

`{"type":"approval.resolve","request_id":"...","call_id":"...","approved":true}`

L'autorisation expire après 120 secondes.
L'annulation de l'exécution annule également
la demande en attente.

Lorsque cette variable est absente ou vaut zéro,
les trois outils de lecture sont autorisés
automatiquement, dans les limites du projet.

Aucun outil d'écriture ni aucune commande système
n'est accessible aux modèles.

### Limites

Cette intégration ne comprend pas encore :

- les serveurs MCP ;
- les outils d'écriture et leurs autorisations ;
- la mémoire SQLite ;
- le comptage exact des tokens ;
- la persistance des événements WebSocket.

Le frontend graphique reste à développer.

Aucun test n'est ajouté par cette étape.

## Outils d'écriture et consolidation

Les outils `project.replace_text` et `project.create_file`
complètent les trois outils de lecture.

Toute écriture est préparée et présentée au frontend par
`tool.preview`, avec son diff unifié, son chemin et les
empreintes SHA-256 anciennes et nouvelles.

Le frontend doit ensuite répondre à `approval.required`
par `approval.resolve`. Cette autorisation est obligatoire
pour toutes les écritures, même lorsque les lectures sont
autorisées automatiquement.

Le consentement concerne la modification déjà préparée :
les arguments ne peuvent pas être remplacés après l'accord.

`replace_text` exige le SHA-256 du fichier original et
une occurrence unique du texte à remplacer.

La version du fichier est recontrôlée immédiatement avant
l'écriture. `create_file` refuse toute destination existante.

Un verrou partagé coordonne les écritures du backend.
Les fichiers temporaires sont créés dans le répertoire
de destination, synchronisés puis publiés atomiquement.

Les autorisations sont isolées par connexion WebSocket.

Les chemins explicitement parcourus refusent les liens
symboliques et les répertoires sensibles restent interdits.

Ces vérifications applicatives n'éliminent pas toutes
les courses avec des processus externes capables de
modifier simultanément l'arborescence du projet.

Les définitions d'outils sont prises en compte dans
l'estimation prudente du budget de contexte, y compris
après les résultats des outils.

Un dépassement du budget interrompt l'exécution.
La récupération sélective, le comptage exact des tokens
et la mémoire SQLite restent à implémenter.

Aucun outil de suppression, aucune exécution de commandes
et aucune intégration MCP ne sont introduits ici.

Aucun test n'a été créé.
