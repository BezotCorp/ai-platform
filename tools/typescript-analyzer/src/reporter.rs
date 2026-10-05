use crate::{declaration_kind::DeclarationKind, file_analysis::FileAnalysis, summary::Summary};

pub fn print_analysis(analysis: &FileAnalysis) {
    println!("{}", analysis.path.display());
    println!("  {}", if analysis.is_ok() { "OK" } else { "KO" });
    if analysis.has_parse_errors() {
        println!(
            "  parsing: {} diagnostic(s){}",
            analysis.parse_diagnostic_count,
            if analysis.fatal_parse_error {
                " + fatal error"
            } else {
                ""
            }
        );
        println!();
        return;
    }
    let actual_file_name = analysis
        .path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    for declaration in &analysis.declarations {
        let expected_declaration_name =
            DeclarationKind::expected_declaration_name(&declaration.name);
        let declaration_name_ok =
            DeclarationKind::is_declaration_name_normalized(&declaration.name);
        let expected_file_name = DeclarationKind::expected_file_name(
            &declaration.name,
            declaration.kind,
            analysis.has_jsx,
        );
        let file_name_ok = actual_file_name == expected_file_name;
        println!(
            "  {} {} (ligne {})",
            declaration.kind, declaration.name, declaration.line
        );
        println!("    type attendu: {}", expected_declaration_name);
        println!("    type trouvé : {}", declaration.name);
        println!(
            "    type        : {}",
            if declaration_name_ok { "OK" } else { "KO" }
        );
        println!("    fichier attendu: {}", expected_file_name);
        println!("    fichier trouvé : {}", actual_file_name);
        println!(
            "    fichier        : {}",
            if file_name_ok { "OK" } else { "KO" }
        );
    }
    if analysis.declarations.len() > 1 {
        println!(
            "  KO: {} déclarations nommées dans le même fichier",
            analysis.declarations.len()
        );
    }
    println!();
}

pub fn print_summary(summary: &Summary) {
    println!("Résumé");
    println!("  fichiers audités : {}", summary.audited);
    println!("  OK               : {}", summary.ok);
    println!("  KO               : {}", summary.ko);
    println!("  erreurs parsing  : {}", summary.parse_errors);
}
