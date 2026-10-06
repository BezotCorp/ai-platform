//! Everything specific to skills: filesystem discovery (`SKILL.md` walking +
//! built-ins) and the runtime MCP client (`client` submodule). User-facing
//! CRUD lives in `crate::sources`, which generalizes across source types.

mod arguments;
mod builtin;
mod client;
mod skill_frontmatter;
mod supporting_files;

pub use client::{EXTENSION_NAME, SkillsClient};
pub use skill_frontmatter::{
    SkillFrontmatter, all_skill_dirs, discover_skills, global_skills_dir, list_installed_skills,
    loaded_skill_context_with_args, project_skills_dir, skill_argument_hint, skill_argument_names,
};
pub(crate) use skill_frontmatter::{
    build_skill_md, discover_skills_with_config, infer_skill_name, is_global_skill_dir,
    parse_skill_frontmatter, resolve_discoverable_skill_dir, resolve_skill_dir, skill_base_dir,
    validate_skill_name,
};
pub(crate) use supporting_files::{
    create_source_file, load_supporting_file, read_source_file, write_source_file,
};
