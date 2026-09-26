# Orchestration supervisée dynamique

Ce mode est distinct du MoA en couches : un agent superviseur Ollama choisit à l'exécution quel agent indépendant solliciter, formule la prochaine tâche et examine les rapports reçus avant de déléguer à nouveau ou de conclure.

## Contrat WebSocket

Le champ `mode` d'une commande `run.start` accepte :

```json
{
  "kind": "multi_agent",
  "coordination": {
    "kind": "supervised",
    "supervisor": {
      "id": "manager",
      "role": "supervisor",
      "instructions": "Décompose la demande et vérifie les rapports.",
      "provider": "ollama",
      "model": "nom-du-modele"
    },
    "workers": [
      {
        "id": "developer",
        "role": "developer",
        "instructions": "Réalise les tâches de développement confiées.",
        "provider": "ollama",
        "model": "nom-du-modele"
      }
    ],
    "max_delegations": 4
  },
  "population": { "kind": "fixed" }
}
```

Une configuration exige entre un et huit travailleurs indépendants, des identifiants uniques et un maximum de une à douze délégations. Le superviseur ne peut déléguer qu'à un travailleur configuré. Ses décisions sont des objets JSON typés `delegate` ou `complete` ; les identifiants et les limites sont validés côté Rust.

Chaque travailleur conserve son propre historique **pendant l'exécution courante**, utilise le moteur `AgentTurn` commun et dispose des mêmes outils de projet, règles d'approbation et protections d'écriture que le mode mono-agent. Les rapports sont traités comme des données non vérifiées ; les plus récents sont transmis au superviseur sous une taille bornée. Le backend émet `orchestration.deciding`, `orchestration.delegated` et `orchestration.reported`, puis les événements habituels `agent.*`, `tool.*` et `run.*`.

## Portée exacte

Les délégations sont actuellement successives ; le superviseur ne lance pas plusieurs générations GPU simultanées et ne possède pas encore les outils de projet en accès direct. Les échanges entre travailleurs passent par le superviseur : aucun dialogue pair-à-pair, aucune population adaptative ou évolutionnaire, aucune persistance autonome de leurs tâches entre redémarrages n'est implémentée. Le mode `layered_moa` reste un choix distinct, inchangé dans sa méthode d'exécution.
