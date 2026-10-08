# Occurrences Goose Conservées

## Historique

Référence :
`crates/bcaip/src/session/import_formats/imported_session.rs`

Valeur :
`ImportFormat::Goose`, `convert_to_goose_session_json`, commentaires décrivant le format natif Goose

Type :
`historique`

Raison :
ces occurrences désignent explicitement l'ancien format d'export/import Goose. Elles ne décrivent pas le produit courant, mais le format source que BCAIP sait relire.

Action future :
introduire un format natif BCAIP distinct si nécessaire, sans supprimer la lecture des exports Goose historiques.

Référence :
`crates/bcaip/src/session/session_manager.rs`

Valeur :
`ImportFormat::convert_to_goose_session_json(json)`

Type :
`historique`

Raison :
appel vers le convertisseur du format historique Goose lors de l'import de session.

Action future :
renommer uniquement si le convertisseur est scindé entre formats Goose legacy et BCAIP natif.

Référence :
`crates/bcaip-cli/src/commands/session.rs`

Valeur :
`ImportFormat::Goose => "goose"`

Type :
`historique`

Raison :
nom de format d'import historique exposé par la CLI. La chaîne identifie le type de fichier source, pas le branding courant.

Action future :
aucune.

Référence :
`crates/bcaip/src/session/import_formats/pi.rs`, `crates/bcaip/src/session/import_formats/claude_code.rs`

Valeur :
commentaires `replay-in-goose` et `Goose models tool responses`

Type :
`historique`

Raison :
commentaires liés à la conversion vers le modèle de session historique Goose conservé par l'importer.

Action future :
clarifier ces commentaires si un modèle de session BCAIP natif remplace complètement le modèle historique.

Référence :
`crates/bcaip-cli/src/scenario_tests/recordings/**`

Valeur :
textes système et réponses enregistrées contenant `goose`, `Goose`, ou `GOOSE`

Type :
`historique`

Raison :
enregistrements de tests historiques. Les modifier sans régénération invaliderait leur rôle de fixture.

Action future :
régénérer les recordings si la suite de scénarios est officiellement rebaselined.

## Externe

Référence :
`crates/bcaip-providers/src/declarative/definitions/zai.json`

Valeur :
`https://docs.z.ai/devpack/tool/goose`

Type :
`externe`

Raison :
documentation officielle du fournisseur Z.ai pour son intégration Goose. Cette URL n'appartient pas à BezotCorp.

Action future :
aucune tant que Z.ai ne fournit pas une URL BCAIP équivalente.

Référence :
`crates/bcaip/src/providers/xai_oauth.rs`

Valeur :
`referrer=goose`

Type :
`externe`

Raison :
paramètre d'attribution xAI externe. Le commentaire du code marque explicitement cette valeur comme allowlistée côté xAI.

Action future :
ne changer que si xAI fournit une valeur BCAIP approuvée.

Référence :
`crates/bcaip/src/dictation/whisper_data/tokens.json`

Valeur :
`Ġgoose`, `Ġgoosebumps`, `Ġgoose bumps`

Type :
`externe`

Raison :
vocabulaire de modèle Whisper, sans rapport avec le branding produit.

Action future :
aucune.
