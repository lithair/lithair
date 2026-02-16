//! Embedded project templates rendered with simple `{{project_name}}` substitution.

const CARGO_TOML: &str = include_str!("cargo_toml.tmpl");
const MAIN_RS: &str = include_str!("main_rs.tmpl");
const MODELS_MOD: &str = include_str!("models_mod.tmpl");
const MODELS_ITEM: &str = include_str!("models_item.tmpl");
const ROUTES_MOD: &str = include_str!("routes_mod.tmpl");
const ROUTES_HEALTH: &str = include_str!("routes_health.tmpl");
const MIDDLEWARE_MOD: &str = include_str!("middleware_mod.tmpl");
const ENV_EXAMPLE: &str = include_str!("env_example.tmpl");
const GITIGNORE: &str = include_str!("gitignore.tmpl");
const README: &str = include_str!("readme.tmpl");
const INDEX_HTML: &str = include_str!("index_html.tmpl");
const STYLES_CSS: &str = include_str!("styles_css.tmpl");
const APP_JS: &str = include_str!("app_js.tmpl");

fn render(template: &str, project_name: &str) -> String {
    template.replace("{{project_name}}", project_name)
}

/// A file to write into the scaffolded project, with its relative path and rendered content.
pub struct TemplateFile {
    pub path: &'static str,
    pub content: String,
}

/// Returns all template files for a standard project.
///
/// When `include_frontend` is false, frontend assets are omitted.
pub fn standard_project(project_name: &str, include_frontend: bool) -> Vec<TemplateFile> {
    let mut files = vec![
        TemplateFile { path: "Cargo.toml", content: render(CARGO_TOML, project_name) },
        TemplateFile { path: "src/main.rs", content: render(MAIN_RS, project_name) },
        TemplateFile { path: "src/models/mod.rs", content: render(MODELS_MOD, project_name) },
        TemplateFile { path: "src/models/item.rs", content: render(MODELS_ITEM, project_name) },
        TemplateFile { path: "src/routes/mod.rs", content: render(ROUTES_MOD, project_name) },
        TemplateFile { path: "src/routes/health.rs", content: render(ROUTES_HEALTH, project_name) },
        TemplateFile {
            path: "src/middleware/mod.rs",
            content: render(MIDDLEWARE_MOD, project_name),
        },
        TemplateFile { path: ".env", content: render(ENV_EXAMPLE, project_name) },
        TemplateFile { path: ".env.example", content: render(ENV_EXAMPLE, project_name) },
        TemplateFile { path: ".gitignore", content: render(GITIGNORE, project_name) },
        TemplateFile { path: "README.md", content: render(README, project_name) },
        TemplateFile { path: "data/.gitkeep", content: String::new() },
    ];

    if include_frontend {
        files.extend([
            TemplateFile { path: "frontend/index.html", content: render(INDEX_HTML, project_name) },
            TemplateFile {
                path: "frontend/css/styles.css",
                content: render(STYLES_CSS, project_name),
            },
            TemplateFile { path: "frontend/js/app.js", content: render(APP_JS, project_name) },
        ]);
    }

    files
}
