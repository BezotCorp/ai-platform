use std::{error::Error, fs, io, path::Path};

use oxc_allocator::Allocator;
use oxc_ast::ast::{Declaration as OxcDeclaration, ExportDefaultDeclarationKind, Statement};
use oxc_parser::Parser;
use oxc_span::SourceType;

use crate::{
    declaration::Declaration, declaration_kind::DeclarationKind, file_analysis::FileAnalysis,
    jsx_detector::JsxDetector,
};

pub fn analyze_file(path: &Path) -> Result<FileAnalysis, Box<dyn Error>> {
    let source = fs::read_to_string(path)?;

    let source_type = SourceType::from_path(path).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unsupported source type for {}: {error}", path.display()),
        )
    })?;

    let allocator = Allocator::default();

    let parsed = Parser::new(&allocator, &source, source_type).parse();

    let parse_diagnostic_count = parsed.diagnostics.len();
    let fatal_parse_error = parsed.fatal_error;

    if fatal_parse_error || parse_diagnostic_count > 0 {
        return Ok(FileAnalysis {
            path: path.to_path_buf(),
            declarations: Vec::new(),
            has_jsx: false,
            parse_diagnostic_count,
            fatal_parse_error,
        });
    }

    let has_jsx = JsxDetector::detect(&parsed.program);
    let mut declarations = Vec::new();

    for statement in &parsed.program.body {
        collect_statement(statement, &source, &mut declarations);
    }

    Ok(FileAnalysis {
        path: path.to_path_buf(),
        declarations,
        has_jsx,
        parse_diagnostic_count,
        fatal_parse_error,
    })
}

fn collect_statement(statement: &Statement<'_>, source: &str, declarations: &mut Vec<Declaration>) {
    match statement {
        Statement::TSTypeAliasDeclaration(declaration) => {
            push_declaration(
                declarations,
                DeclarationKind::TypeAlias,
                declaration.id.name.as_str(),
                declaration.span.start,
                source,
            );
        }

        Statement::TSInterfaceDeclaration(declaration) => {
            push_declaration(
                declarations,
                DeclarationKind::Interface,
                declaration.id.name.as_str(),
                declaration.span.start,
                source,
            );
        }

        Statement::TSEnumDeclaration(declaration) => {
            push_declaration(
                declarations,
                DeclarationKind::Enum,
                declaration.id.name.as_str(),
                declaration.span.start,
                source,
            );
        }

        Statement::ClassDeclaration(declaration) => {
            if let Some(identifier) = &declaration.id {
                push_declaration(
                    declarations,
                    DeclarationKind::Class,
                    identifier.name.as_str(),
                    declaration.span.start,
                    source,
                );
            }
        }

        Statement::ExportDeclaration(export) => {
            collect_exported_declaration(&export.declaration, source, declarations);
        }

        Statement::ExportDefaultDeclaration(export) => {
            collect_default_export(&export.declaration, source, declarations);
        }

        _ => {}
    }
}

fn collect_exported_declaration(
    declaration: &OxcDeclaration<'_>,
    source: &str,
    declarations: &mut Vec<Declaration>,
) {
    match declaration {
        OxcDeclaration::TSTypeAliasDeclaration(declaration) => {
            push_declaration(
                declarations,
                DeclarationKind::TypeAlias,
                declaration.id.name.as_str(),
                declaration.span.start,
                source,
            );
        }

        OxcDeclaration::TSInterfaceDeclaration(declaration) => {
            push_declaration(
                declarations,
                DeclarationKind::Interface,
                declaration.id.name.as_str(),
                declaration.span.start,
                source,
            );
        }

        OxcDeclaration::TSEnumDeclaration(declaration) => {
            push_declaration(
                declarations,
                DeclarationKind::Enum,
                declaration.id.name.as_str(),
                declaration.span.start,
                source,
            );
        }

        OxcDeclaration::ClassDeclaration(declaration) => {
            if let Some(identifier) = &declaration.id {
                push_declaration(
                    declarations,
                    DeclarationKind::Class,
                    identifier.name.as_str(),
                    declaration.span.start,
                    source,
                );
            }
        }

        _ => {}
    }
}

fn collect_default_export(
    declaration: &ExportDefaultDeclarationKind<'_>,
    source: &str,
    declarations: &mut Vec<Declaration>,
) {
    match declaration {
        ExportDefaultDeclarationKind::TSInterfaceDeclaration(declaration) => {
            push_declaration(
                declarations,
                DeclarationKind::Interface,
                declaration.id.name.as_str(),
                declaration.span.start,
                source,
            );
        }

        ExportDefaultDeclarationKind::ClassDeclaration(declaration) => {
            if let Some(identifier) = &declaration.id {
                push_declaration(
                    declarations,
                    DeclarationKind::Class,
                    identifier.name.as_str(),
                    declaration.span.start,
                    source,
                );
            }
        }

        _ => {}
    }
}

fn push_declaration(
    declarations: &mut Vec<Declaration>,
    kind: DeclarationKind,
    name: &str,
    start: u32,
    source: &str,
) {
    declarations.push(Declaration::new(kind, name, source_line(source, start)));
}

fn source_line(source: &str, start: u32) -> usize {
    source
        .as_bytes()
        .iter()
        .take(start as usize)
        .filter(|byte| **byte == b'\n')
        .count()
        + 1
}
