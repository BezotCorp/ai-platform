use std::fmt::{Display, Formatter, Result};

use heck::{ToLowerCamelCase, ToUpperCamelCase};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeclarationKind {
    TypeAlias,
    Interface,
    Enum,
    Class,
}

impl DeclarationKind {
    pub fn expected_declaration_name(declaration_name: &str) -> String {
        declaration_name.to_upper_camel_case()
    }

    pub fn expected_file_name(
        declaration_name: &str,
        declaration_kind: DeclarationKind,
        has_jsx: bool,
    ) -> String {
        let normalized_name = Self::expected_declaration_name(declaration_name);

        let stem = normalized_name.to_lower_camel_case();

        let extension = match declaration_kind {
            Self::TypeAlias | Self::Interface | Self::Enum => "ts",

            Self::Class if has_jsx => "tsx",

            Self::Class => "ts",
        };

        format!("{stem}.{extension}")
    }

    pub fn is_declaration_name_normalized(declaration_name: &str) -> bool {
        Self::expected_declaration_name(declaration_name) == declaration_name
    }
}

impl Display for DeclarationKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> Result {
        let value = match self {
            Self::TypeAlias => "type",
            Self::Interface => "interface",
            Self::Enum => "enum",
            Self::Class => "class",
        };

        formatter.write_str(value)
    }
}
