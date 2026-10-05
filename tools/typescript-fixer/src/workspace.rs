use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    fs,
    path::{Component, Path, PathBuf},
};

use pathdiff::diff_paths;
use regex::{Captures, Regex};

use crate::{
    fix_action::FixAction, fix_plan::FixPlan, prepared_change::PreparedChange,
    skipped_action::SkippedAction,
};

pub(crate) struct Workspace {
    root: PathBuf,
    original: BTreeMap<PathBuf, String>,
    current: BTreeMap<PathBuf, String>,
    renames: BTreeMap<PathBuf, PathBuf>,
}

impl Workspace {
    pub(crate) fn load(root: &Path) -> Result<Self, String> {
        let mut original = BTreeMap::new();
        load_directory(root, &mut original)?;
        Ok(Self {
            root: root.to_path_buf(),
            current: original.clone(),
            original,
            renames: BTreeMap::new(),
        })
    }

    pub(crate) fn prepare(&mut self, plan: &FixPlan) -> Result<Vec<SkippedAction>, String> {
        let mut skipped = Vec::new();
        /*
         * Les suppressions d'imports proviennent directement
         * de diagnostics tsc : aucune analyse d'usage n'est
         * refaite dans le fixer.
         */
        for (action_index, action) in plan.actions().iter().enumerate() {
            let FixAction::RemoveUnusedImport {
                importer,
                symbol,
                whole_import,
                line,
                ..
            } = action
            else {
                continue;
            };
            if let Err(error) =
                self.remove_unused_import(importer, symbol.as_deref(), *whole_import, *line)
            {
                skipped.push(SkippedAction {
                    action_index,
                    description: action.describe(),
                    reason: error,
                });
            }
        }
        /*
         * Les modules manquants ne sont redirigés que lorsque
         * l'analyzer a produit une cible unique.
         */
        for (action_index, action) in plan.actions().iter().enumerate() {
            let FixAction::RedirectModule {
                importer,
                current_module,
                target,
                ..
            } = action
            else {
                continue;
            };
            if let Err(error) = self.redirect_module(importer, current_module, target) {
                skipped.push(SkippedAction {
                    action_index,
                    description: action.describe(),
                    reason: error,
                });
            }
        }
        /*
         * Les redirections tsc ont déjà été résolues par
         * l'analyzer. Le fixer ne cherche aucune cible.
         */
        for (action_index, action) in plan.actions().iter().enumerate() {
            let FixAction::RedirectImport {
                importer,
                symbol,
                current_module,
                target,
                kind,
                ..
            } = action
            else {
                continue;
            };
            if let Err(error) = self.redirect_import(importer, symbol, current_module, target, kind)
            {
                skipped.push(SkippedAction {
                    action_index,
                    description: action.describe(),
                    reason: error,
                });
            }
        }
        /*
         * Les renommages de symboles sont déterministes.
         * Ils sont appliqués au workspace RAM avant les
         * extractions.
         */
        for action in plan.actions() {
            if let FixAction::RenameType { from, to, .. } = action {
                self.rename_identifier(from, to);
            }
        }
        /*
         * On conserve l'index d'origine de chaque action
         * pour que le debug puisse montrer exactement
         * laquelle a été préparée ou ignorée.
         *
         * Pour un même fichier, les extractions sont
         * traitées du bas vers le haut.
         */
        let mut extracts: Vec<(usize, &FixAction)> = plan
            .actions()
            .iter()
            .enumerate()
            .filter(|(_, action)| matches!(action, FixAction::ExtractDeclaration { .. }))
            .collect();
        extracts.sort_by_key(|(_, left)| std::cmp::Reverse(extraction_key(left)));
        for (action_index, action) in extracts {
            let FixAction::ExtractDeclaration {
                source,
                target,
                kind,
                name,
                ..
            } = action
            else {
                unreachable!();
            };
            if let Err(error) = self.extract_declaration(source, target, kind, name) {
                if is_skippable_extraction_error(&error) {
                    skipped.push(SkippedAction {
                        action_index,
                        description: action.describe(),
                        reason: error,
                    });

                    continue;
                }
                return Err(error);
            }
        }
        /*
         * Les renommages de fichiers restent des erreurs
         * globales s'ils deviennent incohérents.
         *
         * Contrairement à une extraction ambiguë, on ne
         * veut pas continuer après avoir préparé une
         * réécriture de chemins qui ne pourrait ensuite
         * pas être finalisée.
         */
        for action in plan.actions() {
            if let FixAction::RenameFile { from, to, .. } = action {
                self.rename_file(from, to)?;
            }
        }
        self.validate()?;
        skipped.sort_by_key(|action| action.action_index);
        Ok(skipped)
    }

    pub(crate) fn changes(&self) -> Vec<PreparedChange> {
        let mut changes = Vec::new();
        let mut consumed_original = BTreeSet::new();
        let mut consumed_current = BTreeSet::new();
        for (from, to) in &self.renames {
            consumed_original.insert(from.clone());
            consumed_current.insert(to.clone());
            changes.push(PreparedChange {
                before_path: Some(from.clone()),
                after_path: Some(to.clone()),
                before: self.original.get(from).cloned(),
                after: self.current.get(to).cloned(),
            });
        }
        for (path, before) in &self.original {
            if consumed_original.contains(path) {
                continue;
            }
            match self.current.get(path) {
                Some(after) if after != before => {
                    changes.push(PreparedChange {
                        before_path: Some(path.clone()),
                        after_path: Some(path.clone()),
                        before: Some(before.clone()),
                        after: Some(after.clone()),
                    });
                }
                Some(_) => {}
                None => {
                    changes.push(PreparedChange {
                        before_path: Some(path.clone()),
                        after_path: None,
                        before: Some(before.clone()),
                        after: None,
                    });
                }
            }
        }
        for (path, after) in &self.current {
            if consumed_current.contains(path) || self.original.contains_key(path) {
                continue;
            }
            changes.push(PreparedChange {
                before_path: None,
                after_path: Some(path.clone()),
                before: None,
                after: Some(after.clone()),
            });
        }
        changes
    }

    pub(crate) fn apply(&self) -> Result<(), String> {
        self.validate()?;
        /*
         * Les renommages physiques sont effectués une fois.
         */
        for (from, to) in &self.renames {
            fs::rename(from, to).map_err(|error| {
                format!(
                    "failed to rename {} -> {}: {error}",
                    from.display(),
                    to.display(),
                )
            })?;
        }
        /*
         * Chaque fichier modifié est écrit une seule fois.
         */
        for (path, content) in &self.current {
            let before = self.original_content_for(path);
            if before.is_some_and(|value| value == content) {
                continue;
            }
            fs::write(path, content)
                .map_err(|error| format!("failed to write {}: {error}", path.display(),))?;
        }
        Ok(())
    }

    fn remove_unused_import(
        &mut self,
        importer: &Path,
        symbol: Option<&str>,
        whole_import: bool,
        line: usize,
    ) -> Result<(), String> {
        let content = self.current.get(importer).cloned().ok_or_else(|| {
            format!(
                "cannot remove unused import: importer is not loaded: {}",
                importer.display(),
            )
        })?;
        let pattern = Regex::new(r#"(?ms)^[ \t]*import\b.*?;[ \t]*(?:\r?\n)?"#)
            .map_err(|error| error.to_string())?;
        let imports: Vec<_> = pattern.find_iter(&content).collect();
        if whole_import {
            let matches: Vec<_> = imports
                .iter()
                .filter(|statement| line_number(&content, statement.start()) == line)
                .collect();
            if matches.len() != 1 {
                return Err(format!(
                    "cannot safely remove entire import in {}:{}: expected one import declaration, found {}",
                    importer.display(),
                    line,
                    matches.len(),
                ));
            }
            let statement = matches[0];
            let mut updated = String::with_capacity(content.len());
            updated.push_str(&content[..statement.start()]);
            updated.push_str(&content[statement.end()..]);
            self.current.insert(importer.to_path_buf(), updated);
            return Ok(());
        }
        let Some(symbol) = symbol else {
            return Err(format!(
                "cannot remove import in {}:{}: missing symbol",
                importer.display(),
                line,
            ));
        };
        let mut candidates = Vec::new();
        for statement in imports {
            if !statement.as_str().contains(symbol) {
                continue;
            }
            if import_contains_binding(statement.as_str(), symbol) {
                candidates.push(statement);
            }
        }
        if candidates.len() != 1 {
            return Err(format!(
                "cannot safely remove `{symbol}` from {}: expected exactly one import binding, found {}",
                importer.display(),
                candidates.len(),
            ));
        }
        let statement = candidates.remove(0);
        let replacement = remove_binding_from_import(statement.as_str(), symbol)?;
        let mut updated = String::with_capacity(content.len());
        updated.push_str(&content[..statement.start()]);
        updated.push_str(&replacement);
        updated.push_str(&content[statement.end()..]);
        self.current.insert(importer.to_path_buf(), updated);
        Ok(())
    }

    fn redirect_module(
        &mut self,
        importer: &Path,
        current_module: &str,
        target: &Path,
    ) -> Result<(), String> {
        let content = self.current.get(importer).cloned().ok_or_else(|| {
            format!(
                "cannot redirect module `{current_module}`: importer is not loaded: {}",
                importer.display(),
            )
        })?;
        let target_module = relative_module_specifier(importer, target)?;
        let pattern = Regex::new(&format!(r#"["']{}["']"#, regex::escape(current_module,),))
            .map_err(|error| error.to_string())?;
        let matches = pattern.find_iter(&content).count();
        if matches != 1 {
            return Err(format!(
                "cannot safely redirect module `{current_module}` in {}: expected exactly one reference, found {matches}",
                importer.display(),
            ));
        }
        let updated = pattern
            .replace(&content, format!("'{target_module}'"))
            .into_owned();
        self.current.insert(importer.to_path_buf(), updated);
        Ok(())
    }

    fn redirect_import(
        &mut self,
        importer: &Path,
        symbol: &str,
        current_module: &str,
        target: &Path,
        kind: &str,
    ) -> Result<(), String> {
        let content = self.current.get(importer).cloned().ok_or_else(|| {
            format!(
                "cannot redirect `{symbol}`: importer is not loaded: {}",
                importer.display(),
            )
        })?;
        /*
         * On cible uniquement un import nommé provenant
         * exactement du module signalé par tsc.
         *
         * Rien n'est réécrit sur simple ressemblance.
         */
        let pattern = Regex::new(&format!(
            r#"(?s)import\s+(type\s+)?\{{([^}}]*)\}}\s+from\s+["']{}["']\s*;"#,
            regex::escape(current_module,),
        ))
        .map_err(|error| error.to_string())?;
        let mut matches = Vec::new();
        for captures in pattern.captures_iter(&content) {
            let Some(statement) = captures.get(0) else {
                continue;
            };
            let body = captures.get(2).map(|value| value.as_str()).unwrap_or("");
            let pieces: Vec<String> = body
                .split(',')
                .map(str::trim)
                .filter(|piece| !piece.is_empty())
                .map(str::to_string)
                .collect();
            let matching: Vec<usize> = pieces
                .iter()
                .enumerate()
                .filter_map(|(index, piece)| {
                    (imported_name(piece) == Some(symbol)).then_some(index)
                })
                .collect();
            if matching.len() == 1 {
                matches.push((
                    statement.start(),
                    statement.end(),
                    captures.get(1).is_some(),
                    pieces,
                    matching[0],
                ));
            }
        }
        if matches.len() != 1 {
            return Err(format!(
                "cannot safely redirect `{symbol}` in {} from `{current_module}`: expected exactly one matching named import, found {}",
                importer.display(),
                matches.len(),
            ));
        }
        let (start, end, whole_type_only, mut pieces, moved_index) = matches.remove(0);
        let moved = pieces.remove(moved_index);
        /*
         * type/interface => import type obligatoire.
         *
         * enum/class conservent la nature de l'import
         * existant car ils peuvent exister au runtime.
         */
        let moved_type_only = whole_type_only
            || moved.trim_start().starts_with("type ")
            || matches!(kind, "type" | "interface");
        let moved_binding = moved
            .trim_start()
            .strip_prefix("type ")
            .unwrap_or(moved.trim())
            .trim();
        let target_module = relative_module_specifier(importer, target)?;
        let new_import = if moved_type_only {
            format!("import type {{ {moved_binding} }} from '{target_module}';")
        } else {
            format!("import {{ {moved_binding} }} from '{target_module}';")
        };
        let old_import = if pieces.is_empty() {
            String::new()
        } else if whole_type_only {
            format!(
                "import type {{ {} }} from '{}';",
                pieces.join(", ",),
                current_module,
            )
        } else {
            format!(
                "import {{ {} }} from '{}';",
                pieces.join(", ",),
                current_module,
            )
        };
        let mut updated = String::with_capacity(content.len() + new_import.len());
        updated.push_str(&content[..start]);
        updated.push_str(&old_import);
        updated.push_str(&content[end..]);
        if !updated.contains(&new_import) {
            updated = insert_bridge(&updated, &new_import);
        }
        self.current.insert(importer.to_path_buf(), updated);
        Ok(())
    }

    fn rename_identifier(&mut self, from: &str, to: &str) {
        for content in self.current.values_mut() {
            let updated = replace_identifier_in_code(content, from, to);
            if updated != *content {
                *content = updated;
            }
        }
    }

    fn extract_declaration(
        &mut self,
        source: &Path,
        target: &Path,
        kind: &str,
        name: &str,
    ) -> Result<(), String> {
        if self.current.contains_key(target) {
            return Err(format!(
                "prepared destination already exists: {}",
                target.display(),
            ));
        }
        let source_text = self
            .current
            .get(source)
            .cloned()
            .ok_or_else(|| format!("source not loaded: {}", source.display(),))?;
        let (start, end) = declaration_span(&source_text, kind, name)?;
        let raw_declaration = source_text[start..end].trim().to_string();
        let was_exported = declaration_is_exported(&raw_declaration);
        let declaration = ensure_exported(&raw_declaration);
        /*
         * Dépendances réellement utilisées dans la déclaration.
         */
        let imports = collect_import_statements(&source_text);
        let mut target_imports = Vec::new();
        let mut imported_bindings = HashSet::new();
        for import in &imports {
            let Some((filtered_import, bindings)) = filter_import_statement(import, &declaration)
            else {
                continue;
            };
            target_imports.push(filtered_import);
            imported_bindings.extend(bindings);
        }
        /*
         * Recherche des symboles top-level locaux utilisés
         * par la déclaration extraite.
         */
        let local_symbols = collect_top_level_symbols(&source_text, start, end);
        let mut local_dependency_imports = Vec::new();
        for symbol in local_symbols {
            if symbol.name == name
                || imported_bindings.contains(&symbol.name)
                || !references_identifier(&declaration, &symbol.name)
            {
                continue;
            }
            if !symbol.exported {
                return Err(format!(
                    "cannot safely extract `{name}` from {}: local dependency `{}` is not exported",
                    source.display(),
                    symbol.name,
                ));
            }
            /*
             * Une dépendance runtime du nouveau fichier vers
             * son ancien module est dangereuse :
             *
             * source -> déclaration extraite
             * déclaration extraite -> source
             *
             * On préfère SKIP plutôt que créer silencieusement
             * une dépendance circulaire.
             */
            if !symbol.type_only {
                return Err(format!(
                    "cannot safely extract `{name}` from {}: runtime dependency `{}` would create a module cycle",
                    source.display(),
                    symbol.name,
                ));
            }
            let source_module = module_specifier(source)?;
            let statement = format!(
                "import type {{ {} }} from '{}';",
                symbol.name, source_module,
            );
            if !target_imports.contains(&statement) {
                local_dependency_imports.push(statement);
            }
        }
        let mut target_text = String::new();
        for import in target_imports.into_iter().chain(local_dependency_imports) {
            target_text.push_str(import.trim());
            target_text.push('\n');
        }
        if !target_text.is_empty() {
            target_text.push('\n');
        }
        target_text.push_str(declaration.trim());
        target_text.push('\n');
        /*
         * Retrait de la déclaration du module original.
         */
        let mut remaining = String::new();
        remaining.push_str(&source_text[..start]);
        remaining.push_str(&source_text[end..]);
        /*
         * Si le module original continue à utiliser la déclaration,
         * on crée un import local vers son nouveau fichier.
         *
         * S'il l'exportait auparavant, on conserve aussi l'API
         * publique avec un re-export.
         */
        let target_module = module_specifier(target)?;
        let needs_local_binding = contains_identifier(&remaining, name);
        let bridge = bridge_statements(
            kind,
            name,
            &target_module,
            needs_local_binding,
            was_exported,
        );
        if !bridge.is_empty() {
            remaining = insert_bridge(&remaining, &bridge);
        }
        self.current.insert(source.to_path_buf(), remaining);
        self.current.insert(target.to_path_buf(), target_text);
        Ok(())
    }

    fn rename_file(&mut self, from: &Path, to: &Path) -> Result<(), String> {
        if self.current.contains_key(to) {
            return Err(format!(
                "prepared rename destination already exists: {}",
                to.display(),
            ));
        }
        /*
         * Avant de déplacer le module, on met à jour tous les
         * module specifiers qui résolvent réellement vers lui.
         */
        let paths: Vec<PathBuf> = self.current.keys().cloned().collect();
        for importer in paths {
            let Some(content) = self.current.get(&importer).cloned() else {
                continue;
            };
            let updated = rewrite_module_specifiers(&importer, &content, from, to)?;
            if updated != content {
                self.current.insert(importer, updated);
            }
        }
        let content = self
            .current
            .remove(from)
            .ok_or_else(|| format!("rename source not loaded: {}", from.display(),))?;
        self.current.insert(to.to_path_buf(), content);
        self.renames.insert(from.to_path_buf(), to.to_path_buf());
        Ok(())
    }

    fn validate(&self) -> Result<(), String> {
        for path in self.current.keys() {
            if !path.starts_with(&self.root) {
                return Err(format!("prepared path outside root: {}", path.display(),));
            }
        }
        for (from, to) in &self.renames {
            if !self.original.contains_key(from) {
                return Err(format!(
                    "rename source was not present in original workspace: {}",
                    from.display(),
                ));
            }
            if self.original.contains_key(to) {
                return Err(format!(
                    "rename destination already existed before preparation: {}",
                    to.display(),
                ));
            }
        }
        Ok(())
    }

    fn original_content_for(&self, current_path: &Path) -> Option<&String> {
        if let Some((original_path, _)) = self
            .renames
            .iter()
            .find(|(_, target)| target.as_path() == current_path)
        {
            return self.original.get(original_path);
        }
        self.original.get(current_path)
    }
}

#[derive(Debug)]
struct LocalSymbol {
    name: String,
    exported: bool,
    type_only: bool,
}

fn is_skippable_extraction_error(error: &str) -> bool {
    error.starts_with("cannot safely extract")
        || error.starts_with("prepared destination already exists")
        || error.starts_with("unable to locate")
        || error.starts_with("unsupported declaration kind")
        || error.starts_with("declaration opening brace not found")
        || error.starts_with("declaration closing brace not found")
        || error.starts_with("type declaration terminator not found")
}

fn extraction_key(action: &FixAction) -> (PathBuf, usize) {
    match action {
        FixAction::ExtractDeclaration { source, line, .. } => (source.clone(), *line),

        _ => unreachable!(),
    }
}

fn load_directory(directory: &Path, files: &mut BTreeMap<PathBuf, String>) -> Result<(), String> {
    for entry in fs::read_dir(directory)
        .map_err(|error| format!("failed to read directory {}: {error}", directory.display(),))?
    {
        let entry = entry.map_err(|error| error.to_string())?;

        let path = entry.path();

        if path.is_dir() {
            load_directory(&path, files)?;

            continue;
        }

        if !is_typescript_file(&path) {
            continue;
        }

        let content = fs::read_to_string(&path)
            .map_err(|error| format!("failed to read {}: {error}", path.display(),))?;

        files.insert(path, content);
    }

    Ok(())
}

fn is_typescript_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };

    (name.ends_with(".ts") || name.ends_with(".tsx")) && !name.ends_with(".d.ts")
}

fn declaration_span(text: &str, kind: &str, name: &str) -> Result<(usize, usize), String> {
    let pattern = Regex::new(&format!(
        r"(?m)^[ \t]*(?:export[ \t]+)?(?:default[ \t]+)?{}[ \t]+{}\b",
        regex::escape(kind),
        regex::escape(name),
    ))
    .map_err(|error| error.to_string())?;

    let found = pattern
        .find(text)
        .ok_or_else(|| format!("unable to locate `{kind} {name}` in prepared source",))?;

    let start = found.start();

    match kind {
        "type" => scan_type_end(text, start),

        "interface" | "enum" | "class" => scan_braced_end(text, start),

        other => Err(format!("unsupported declaration kind: {other}",)),
    }
}

fn declaration_is_exported(declaration: &str) -> bool {
    declaration.trim_start().starts_with("export ")
}

fn ensure_exported(declaration: &str) -> String {
    if declaration_is_exported(declaration) {
        return declaration.to_string();
    }

    format!("export {declaration}")
}

fn collect_import_statements(text: &str) -> Vec<String> {
    let mut imports = Vec::new();

    let mut current = String::new();

    let mut collecting = false;

    for line in text.lines() {
        let trimmed = line.trim_start();

        if !collecting && trimmed.starts_with("import ") {
            collecting = true;
            current.clear();
        }

        if !collecting {
            continue;
        }

        current.push_str(line);
        current.push('\n');

        if line.trim_end().ends_with(';') {
            imports.push(current.trim_end().to_string());

            current.clear();
            collecting = false;
        }
    }

    imports
}

fn filter_import_statement(
    statement: &str,
    declaration: &str,
) -> Option<(String, HashSet<String>)> {
    let normalized = statement.trim();

    /*
     * Un import uniquement pour ses effets de bord n'est
     * pas une dépendance lexicale de la déclaration.
     */
    if !normalized.contains(" from ") {
        return None;
    }

    let from_position = normalized.rfind(" from ")?;

    let mut clause = normalized["import".len()..from_position].trim();

    let module = normalized[from_position + " from ".len()..].trim();

    let whole_type_only = clause.starts_with("type ");

    if whole_type_only {
        clause = clause["type ".len()..].trim();
    }

    let mut pieces = Vec::new();

    let mut used_bindings = HashSet::new();

    /*
     * Default import :
     *
     * import React, { ... } from 'react';
     */
    if !clause.starts_with('{') && !clause.starts_with('*') {
        let end = clause.find(',').unwrap_or(clause.len());

        let default_binding = clause[..end].trim();

        if !default_binding.is_empty() && references_identifier(declaration, default_binding) {
            pieces.push(default_binding.to_string());

            used_bindings.insert(default_binding.to_string());
        }

        clause = clause[end.min(clause.len())..]
            .trim_start_matches(',')
            .trim();
    }

    /*
     * Namespace import :
     *
     * import * as Foo from './foo';
     */
    if clause.starts_with('*') {
        let tokens: Vec<&str> = clause.split_whitespace().collect();

        if tokens.len() >= 3 && tokens[0] == "*" && tokens[1] == "as" {
            let binding = tokens[2];

            if references_identifier(declaration, binding) {
                pieces.push(format!("* as {binding}"));

                used_bindings.insert(binding.to_string());
            }
        }
    }

    /*
     * Named imports :
     *
     * import { Foo, type Bar, Baz as Qux } from ...
     */
    if let (Some(open), Some(close)) = (clause.find('{'), clause.rfind('}'))
        && close > open
    {
        let inside = &clause[open + 1..close];

        let mut selected = Vec::new();

        for raw in inside.split(',') {
            let specifier = raw.trim();

            if specifier.is_empty() {
                continue;
            }

            let without_type = specifier.strip_prefix("type ").unwrap_or(specifier).trim();

            let parts: Vec<&str> = without_type.split_whitespace().collect();

            let local_binding = match parts.as_slice() {
                [name] => *name,

                [_original, "as", alias] => *alias,

                _ => {
                    /*
                     * Syntaxe inattendue :
                     * on ne l'invente pas.
                     */
                    continue;
                }
            };

            if !references_identifier(declaration, local_binding) {
                continue;
            }

            selected.push(specifier.to_string());

            used_bindings.insert(local_binding.to_string());
        }

        if !selected.is_empty() {
            pieces.push(format!("{{ {} }}", selected.join(", ",),));
        }
    }

    if pieces.is_empty() {
        return None;
    }

    let prefix = if whole_type_only {
        "import type "
    } else {
        "import "
    };

    Some((
        format!("{prefix}{} from {module}", pieces.join(", ",),),
        used_bindings,
    ))
}

fn references_identifier(text: &str, identifier: &str) -> bool {
    if identifier.is_empty() {
        return false;
    }

    let cleaned = mask_comments_and_strings(text);

    let bytes = cleaned.as_bytes();

    let needle = identifier.as_bytes();

    let mut index = 0usize;

    while index + needle.len() <= bytes.len() {
        if &bytes[index..index + needle.len()] != needle {
            index += 1;
            continue;
        }

        if !identifier_boundary(bytes, index, needle.len()) {
            index += 1;
            continue;
        }

        if !is_member_declaration_name(bytes, index, needle.len()) {
            return true;
        }

        index += needle.len();
    }

    false
}

fn is_member_declaration_name(bytes: &[u8], start: usize, len: usize) -> bool {
    let previous = previous_significant_byte(bytes, start);

    let next = next_significant_byte(bytes, start + len);

    /*
     * Interface / type / class members :
     *
     * {
     *   stop(...): void
     *   hour: number
     *   optional?: string
     * }
     *
     * Le nom du membre n'est pas une référence vers un
     * symbole homonyme du module.
     */
    let at_member_boundary = previous.is_none() || matches!(previous, Some(b'{' | b';' | b','));

    if !at_member_boundary {
        return false;
    }

    matches!(next, Some(b'(' | b':' | b'?'))
}

fn previous_significant_byte(bytes: &[u8], start: usize) -> Option<u8> {
    let mut index = start;

    while index > 0 {
        index -= 1;

        if !bytes[index].is_ascii_whitespace() {
            return Some(bytes[index]);
        }
    }

    None
}

fn next_significant_byte(bytes: &[u8], start: usize) -> Option<u8> {
    let mut index = start;

    while index < bytes.len() {
        if !bytes[index].is_ascii_whitespace() {
            return Some(bytes[index]);
        }

        index += 1;
    }

    None
}

fn collect_identifiers(text: &str) -> HashSet<String> {
    let cleaned = mask_comments_and_strings(text);

    let identifier = Regex::new(r"[A-Za-z_$][A-Za-z0-9_$]*").expect("identifier regex");

    identifier
        .find_iter(&cleaned)
        .map(|matched| matched.as_str().to_string())
        .collect()
}

fn collect_top_level_symbols(
    text: &str,
    excluded_start: usize,
    excluded_end: usize,
) -> Vec<LocalSymbol> {
    let mut result = Vec::new();

    let declaration =
        Regex::new(
            r"(?m)^[ \t]*(export[ \t]+)?(?:default[ \t]+)?(?:declare[ \t]+)?(?:abstract[ \t]+)?(type|interface|enum|class|function|const|let|var)[ \t]+([A-Za-z_$][A-Za-z0-9_$]*)",
        )
        .expect(
            "top-level declaration regex",
        );

    for captures in declaration.captures_iter(text) {
        let Some(whole) = captures.get(0) else {
            continue;
        };

        if whole.start() >= excluded_start && whole.start() < excluded_end {
            continue;
        }

        /*
         * La regex trouve aussi les déclarations indentées
         * dans les fonctions/classes. Elles ne sont locales
         * au module que si la profondeur syntaxique est 0.
         */
        if brace_depth_at(text, whole.start()) != 0 {
            continue;
        }

        let Some(kind) = captures.get(2) else {
            continue;
        };

        let Some(name) = captures.get(3) else {
            continue;
        };

        result.push(LocalSymbol {
            name: name.as_str().to_string(),
            exported: captures.get(1).is_some(),
            type_only: matches!(kind.as_str(), "type" | "interface"),
        });
    }

    result
}

fn brace_depth_at(text: &str, end: usize) -> usize {
    let bytes = text.as_bytes();

    let mut depth = 0usize;

    let mut index = 0usize;

    while index < end && index < bytes.len() {
        if starts_line_comment(bytes, index) {
            index = consume_line_comment(bytes, index);

            continue;
        }

        if starts_block_comment(bytes, index) {
            index = consume_block_comment(bytes, index);

            continue;
        }

        if matches!(bytes[index], b'\'' | b'"' | b'`') {
            index = consume_string(bytes, index);

            continue;
        }

        match bytes[index] {
            b'{' => {
                depth += 1;
            }

            b'}' => {
                depth = depth.saturating_sub(1);
            }

            _ => {}
        }

        index += 1;
    }

    depth
}

fn line_number(source: &str, byte_offset: usize) -> usize {
    source
        .as_bytes()
        .iter()
        .take(byte_offset)
        .filter(|byte| **byte == b'\n')
        .count()
        + 1
}

fn import_contains_binding(statement: &str, symbol: &str) -> bool {
    let escaped = regex::escape(symbol);

    let Ok(pattern) = Regex::new(&format!(
        r"(?m)(?:^|[\s{{,])(?:type\s+)?{escaped}(?:\s+as\s+[A-Za-z_$][A-Za-z0-9_$]*)?(?:[\s,}}]|$)"
    )) else {
        return false;
    };

    pattern.is_match(statement)
}

fn remove_binding_from_import(statement: &str, symbol: &str) -> Result<String, String> {
    let semicolon = statement
        .rfind(';')
        .ok_or_else(|| "import declaration has no semicolon".to_string())?;

    let import = &statement[..=semicolon];

    let trailing = &statement[semicolon + 1..];

    let from_pattern =
        Regex::new(r#"(?s)\s+from\s+(["'][^"']+["'])\s*;$"#).map_err(|error| error.to_string())?;

    let Some(from) = from_pattern.captures(import) else {
        return Err(format!(
            "unsupported import declaration while removing `{symbol}`"
        ));
    };

    let module = from
        .get(1)
        .map(|value| value.as_str())
        .ok_or_else(|| "import module not found".to_string())?;

    let from_match = from
        .get(0)
        .ok_or_else(|| "import tail not found".to_string())?;

    let mut head = import["import".len()..from_match.start()]
        .trim()
        .to_string();

    let import_type = if let Some(rest) = head.strip_prefix("type ") {
        head = rest.trim().to_string();

        true
    } else {
        false
    };

    let mut default_binding = None;

    let mut named_bindings: Vec<String> = Vec::new();

    if let (Some(open), Some(close)) = (head.find('{'), head.rfind('}')) {
        let before = head[..open].trim().trim_end_matches(',').trim();

        if !before.is_empty() {
            default_binding = Some(before.to_string());
        }

        let body = &head[open + 1..close];

        named_bindings = body
            .split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(str::to_string)
            .collect();
    } else if head.starts_with("* as ") {
        let namespace = head.trim_start_matches("* as ").trim();

        if namespace == symbol {
            return Ok(trailing.to_string());
        }

        return Err(format!("`{symbol}` was not the namespace import binding"));
    } else {
        default_binding = Some(head.clone());
    }

    let default_matches = default_binding
        .as_deref()
        .is_some_and(|binding| binding == symbol);

    let original_named_count = named_bindings.len();

    named_bindings.retain(|binding| imported_name(binding) != Some(symbol));

    let named_removed = named_bindings.len() != original_named_count;

    if !default_matches && !named_removed {
        return Err(format!("import binding `{symbol}` not found"));
    }

    if default_matches {
        default_binding = None;
    }

    if default_binding.is_none() && named_bindings.is_empty() {
        return Ok(trailing.to_string());
    }

    let mut rebuilt = String::from("import ");

    if import_type {
        rebuilt.push_str("type ");
    }

    match (default_binding, named_bindings.is_empty()) {
        (Some(default), true) => {
            rebuilt.push_str(&default);
        }

        (Some(default), false) => {
            rebuilt.push_str(&default);

            rebuilt.push_str(", { ");

            rebuilt.push_str(&named_bindings.join(", "));

            rebuilt.push_str(" }");
        }

        (None, false) => {
            rebuilt.push_str("{ ");

            rebuilt.push_str(&named_bindings.join(", "));

            rebuilt.push_str(" }");
        }

        (None, true) => {
            return Ok(trailing.to_string());
        }
    }

    rebuilt.push_str(" from ");

    rebuilt.push_str(module);

    rebuilt.push(';');

    rebuilt.push_str(trailing);

    Ok(rebuilt)
}

fn imported_name(piece: &str) -> Option<&str> {
    let piece = piece.trim();

    let piece = piece.strip_prefix("type ").unwrap_or(piece).trim();

    piece.split_whitespace().next()
}

fn relative_module_specifier(importer: &Path, target: &Path) -> Result<String, String> {
    let importer_directory = importer
        .parent()
        .ok_or_else(|| format!("importer has no parent directory: {}", importer.display(),))?;

    let target_without_extension = target.with_extension("");

    let relative = diff_paths(&target_without_extension, importer_directory).ok_or_else(|| {
        format!(
            "cannot compute relative module path from {} to {}",
            importer.display(),
            target.display(),
        )
    })?;

    let mut text = relative.to_string_lossy().replace('\\', "/");

    if !text.starts_with('.') {
        text = format!("./{text}");
    }

    Ok(text)
}

fn module_specifier(path: &Path) -> Result<String, String> {
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| format!("invalid module filename: {}", path.display(),))?;

    Ok(format!("./{stem}"))
}

fn bridge_statements(
    kind: &str,
    name: &str,
    module: &str,
    needs_local_binding: bool,
    was_exported: bool,
) -> String {
    let type_only = matches!(kind, "type" | "interface");

    let mut result = String::new();

    if needs_local_binding {
        if type_only {
            result.push_str(&format!("import type {{ {name} }} from '{module}';\n",));
        } else {
            result.push_str(&format!("import {{ {name} }} from '{module}';\n",));
        }
    }

    if was_exported {
        if type_only {
            result.push_str(&format!("export type {{ {name} }} from '{module}';\n",));
        } else {
            result.push_str(&format!("export {{ {name} }} from '{module}';\n",));
        }
    }

    result
}

fn insert_bridge(text: &str, bridge: &str) -> String {
    let mut result = String::with_capacity(text.len() + bridge.len() + 1);

    result.push_str(bridge);

    if !bridge.ends_with("\n\n") {
        result.push('\n');
    }

    result.push_str(text);

    result
}

fn rewrite_module_specifiers(
    importer: &Path,
    content: &str,
    old_path: &Path,
    new_path: &Path,
) -> Result<String, String> {
    let patterns = [
        Regex::new(r#"(from\s+)(["'])([^"']+)(["'])"#),
        Regex::new(r#"(import\s+)(["'])([^"']+)(["'])"#),
        Regex::new(r#"(import\s*\(\s*)(["'])([^"']+)(["'])"#),
    ];

    let mut updated = content.to_string();

    for pattern in patterns {
        let pattern = pattern.map_err(|error| error.to_string())?;

        updated = pattern
            .replace_all(&updated, |captures: &Captures| {
                let prefix = captures.get(1).map(|value| value.as_str()).unwrap_or("");

                let quote = captures.get(2).map(|value| value.as_str()).unwrap_or("'");

                let specifier = captures.get(3).map(|value| value.as_str()).unwrap_or("");

                let closing = captures.get(4).map(|value| value.as_str()).unwrap_or(quote);

                if quote != closing {
                    return captures[0].to_string();
                }

                let Some(replacement) =
                    rewritten_specifier(importer, specifier, old_path, new_path)
                else {
                    return captures[0].to_string();
                };

                format!("{prefix}{quote}{replacement}{closing}")
            })
            .into_owned();
    }

    Ok(updated)
}

fn rewritten_specifier(
    importer: &Path,
    specifier: &str,
    old_path: &Path,
    new_path: &Path,
) -> Option<String> {
    if !specifier.starts_with('.') {
        return None;
    }

    let importer_directory = importer.parent()?;

    let base = normalize_path(&importer_directory.join(specifier));

    let old = normalize_path(old_path);

    if !module_candidate_matches(&base, &old) {
        return None;
    }

    let new_without_extension = new_path.with_extension("");

    let relative = diff_paths(&new_without_extension, importer_directory)?;

    let mut text = relative.to_string_lossy().replace('\\', "/");

    if !text.starts_with('.') {
        text = format!("./{text}");
    }

    Some(text)
}

fn module_candidate_matches(base: &Path, target: &Path) -> bool {
    if normalize_path(base) == normalize_path(target) {
        return true;
    }

    for extension in ["ts", "tsx"] {
        if normalize_path(&base.with_extension(extension)) == normalize_path(target) {
            return true;
        }
    }

    for index in ["index.ts", "index.tsx"] {
        if normalize_path(&base.join(index)) == normalize_path(target) {
            return true;
        }
    }

    false
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();

    for component in path.components() {
        match component {
            Component::CurDir => {}

            Component::ParentDir => {
                result.pop();
            }

            other => {
                result.push(other.as_os_str());
            }
        }
    }

    result
}

fn contains_identifier(text: &str, identifier: &str) -> bool {
    collect_identifiers(text).contains(identifier)
}

fn replace_identifier_in_code(text: &str, from: &str, to: &str) -> String {
    if from.is_empty() || from == to {
        return text.to_string();
    }

    let bytes = text.as_bytes();

    let needle = from.as_bytes();

    let mut result = String::with_capacity(text.len());

    let mut index = 0;

    while index < bytes.len() {
        if starts_line_comment(bytes, index) {
            let end = consume_line_comment(bytes, index);

            result.push_str(&text[index..end]);

            index = end;
            continue;
        }

        if starts_block_comment(bytes, index) {
            let end = consume_block_comment(bytes, index);

            result.push_str(&text[index..end]);

            index = end;
            continue;
        }

        if matches!(bytes[index], b'\'' | b'"' | b'`') {
            let end = consume_string(bytes, index);

            result.push_str(&text[index..end]);

            index = end;
            continue;
        }

        if index + needle.len() <= bytes.len()
            && &bytes[index..index + needle.len()] == needle
            && identifier_boundary(bytes, index, needle.len())
        {
            result.push_str(to);
            index += needle.len();
            continue;
        }

        result.push(bytes[index] as char);

        index += 1;
    }

    result
}

fn mask_comments_and_strings(text: &str) -> String {
    let bytes = text.as_bytes();

    let mut result = Vec::with_capacity(bytes.len());

    let mut index = 0;

    while index < bytes.len() {
        if starts_line_comment(bytes, index) {
            let end = consume_line_comment(bytes, index);

            for byte in &bytes[index..end] {
                result.push(if *byte == b'\n' { b'\n' } else { b' ' });
            }

            index = end;
            continue;
        }

        if starts_block_comment(bytes, index) {
            let end = consume_block_comment(bytes, index);

            for byte in &bytes[index..end] {
                result.push(if *byte == b'\n' { b'\n' } else { b' ' });
            }

            index = end;
            continue;
        }

        if matches!(bytes[index], b'\'' | b'"' | b'`') {
            let end = consume_string(bytes, index);

            for byte in &bytes[index..end] {
                result.push(if *byte == b'\n' { b'\n' } else { b' ' });
            }

            index = end;
            continue;
        }

        result.push(bytes[index]);

        index += 1;
    }

    String::from_utf8(result).unwrap_or_default()
}

fn identifier_boundary(bytes: &[u8], start: usize, len: usize) -> bool {
    let before = start
        .checked_sub(1)
        .and_then(|index| bytes.get(index))
        .copied();

    let after = bytes.get(start + len).copied();

    !before.is_some_and(is_identifier_byte) && !after.is_some_and(is_identifier_byte)
}

fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$'
}

fn starts_line_comment(bytes: &[u8], index: usize) -> bool {
    bytes.get(index) == Some(&b'/') && bytes.get(index + 1) == Some(&b'/')
}

fn starts_block_comment(bytes: &[u8], index: usize) -> bool {
    bytes.get(index) == Some(&b'/') && bytes.get(index + 1) == Some(&b'*')
}

fn consume_line_comment(bytes: &[u8], start: usize) -> usize {
    let mut index = start + 2;

    while index < bytes.len() && bytes[index] != b'\n' {
        index += 1;
    }

    index
}

fn consume_block_comment(bytes: &[u8], start: usize) -> usize {
    let mut index = start + 2;

    while index + 1 < bytes.len() {
        if bytes[index] == b'*' && bytes[index + 1] == b'/' {
            return index + 2;
        }

        index += 1;
    }

    bytes.len()
}

fn consume_string(bytes: &[u8], start: usize) -> usize {
    let quote = bytes[start];

    let mut index = start + 1;

    while index < bytes.len() {
        if bytes[index] == b'\\' {
            index += 2;
            continue;
        }

        if bytes[index] == quote {
            return index + 1;
        }

        index += 1;
    }

    bytes.len()
}

fn scan_braced_end(text: &str, start: usize) -> Result<(usize, usize), String> {
    let bytes = text.as_bytes();

    let mut open = None;

    for index in start..bytes.len() {
        if bytes[index] == b'{' {
            open = Some(index);
            break;
        }
    }

    let open = open.ok_or("declaration opening brace not found")?;

    let mut depth = 0usize;

    let mut index = open;

    while index < bytes.len() {
        if starts_line_comment(bytes, index) {
            index = consume_line_comment(bytes, index);

            continue;
        }

        if starts_block_comment(bytes, index) {
            index = consume_block_comment(bytes, index);

            continue;
        }

        if matches!(bytes[index], b'\'' | b'"' | b'`') {
            index = consume_string(bytes, index);

            continue;
        }

        match bytes[index] {
            b'{' => {
                depth += 1;
            }

            b'}' => {
                depth -= 1;

                if depth == 0 {
                    let mut end = index + 1;

                    while end < bytes.len() && matches!(bytes[end], b' ' | b'\t' | b'\r') {
                        end += 1;
                    }

                    if end < bytes.len() && bytes[end] == b';' {
                        end += 1;
                    }

                    if end < bytes.len() && bytes[end] == b'\n' {
                        end += 1;
                    }

                    return Ok((start, end));
                }
            }

            _ => {}
        }

        index += 1;
    }

    Err("declaration closing brace not found".into())
}

fn scan_type_end(text: &str, start: usize) -> Result<(usize, usize), String> {
    let bytes = text.as_bytes();

    let mut round = 0usize;
    let mut square = 0usize;
    let mut curly = 0usize;
    let mut angle = 0usize;
    let mut index = start;

    while index < bytes.len() {
        if starts_line_comment(bytes, index) {
            index = consume_line_comment(bytes, index);

            continue;
        }

        if starts_block_comment(bytes, index) {
            index = consume_block_comment(bytes, index);

            continue;
        }

        if matches!(bytes[index], b'\'' | b'"' | b'`') {
            index = consume_string(bytes, index);

            continue;
        }

        match bytes[index] {
            b'(' => round += 1,

            b')' => {
                round = round.saturating_sub(1);
            }

            b'[' => square += 1,

            b']' => {
                square = square.saturating_sub(1);
            }

            b'{' => curly += 1,

            b'}' => {
                curly = curly.saturating_sub(1);
            }

            b'<' => angle += 1,

            b'>' => {
                angle = angle.saturating_sub(1);
            }

            b';' if round == 0 && square == 0 && curly == 0 && angle == 0 => {
                let mut end = index + 1;

                if end < bytes.len() && bytes[end] == b'\n' {
                    end += 1;
                }

                return Ok((start, end));
            }

            _ => {}
        }

        index += 1;
    }

    Err("type declaration terminator not found".into())
}
