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
