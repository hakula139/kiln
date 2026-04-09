use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use indoc::indoc;
use tempfile::TempDir;

use crate::config::Config;
use crate::content::frontmatter::Frontmatter;
use crate::content::page::{Page, PageKind};
use crate::template::TemplateEngine;

static BASE_HTML: &str = indoc! {r#"
    <!DOCTYPE html>
    <html lang="{{ config.language }}">
    <head>
      <meta charset="utf-8">
      {% block title %}<title>{{ config.title }}</title>{% endblock %}
      {% block head %}{% endblock %}
    </head>

    <body>
      {% block body %}{% endblock %}
    </body>
    </html>
"#};

static POST_HTML: &str = indoc! {r#"
    {% extends "base.html" %}

    {% block title %}<title>{{ title }} - {{ config.title }}</title>{% endblock %}

    {% block head %}
      {%- if description %}
      <meta name="description" content="{{ description }}">
      {%- endif %}
      <link rel="canonical" href="{{ url | safe }}">
      <meta property="og:title" content="{{ title }}">
      <meta property="og:description" content="{{ description }}">
      <meta property="og:url" content="{{ url | safe }}">
      <meta property="og:type" content="article">
      <meta property="og:site_name" content="{{ config.title }}">
      {%- if featured_image %}
      <meta property="og:image" content="{{ config.base_url | safe }}{{ featured_image.src | safe }}">
      {%- endif %}
      <meta name="twitter:card" content="{% if featured_image %}summary_large_image{% else %}summary{% endif %}">
    {% endblock %}

    {% block body %}
      <article>
        <header>
          <h1>{{ title }}</h1>
          {% if date %}<time datetime="{{ date }}">{{ date }}</time>{% endif %}
        </header>
        {% if toc %}<aside>{{ toc | safe }}</aside>{% endif %}
        <div class="content">{{ content | safe }}</div>
      </article>
    {% endblock %}
"#};

static PAGE_HTML: &str = indoc! {r#"
    {% extends "base.html" %}

    {% block title %}<title>{{ title }} - {{ config.title }}</title>{% endblock %}

    {% block head %}
      {%- if description %}
      <meta name="description" content="{{ description }}">
      {%- endif %}
      <link rel="canonical" href="{{ url | safe }}">
    {% endblock %}

    {% block body %}
      <article class="page">
        <h1>{{ title }}</h1>
        <div class="content">{{ content | safe }}</div>
      </article>
    {% endblock %}
"#};

static HOME_HTML: &str = indoc! {r#"
    {% extends "base.html" %}

    {% block body %}
      <div class="home">
        <ul>
        {%- for page in pages %}
          <li><a href="{{ page.url | safe }}">{{ page.title }}</a></li>
        {%- endfor %}
        </ul>
        {%- if pagination.total_pages > 1 %}
        <nav>Page {{ pagination.current_page }} / {{ pagination.total_pages }}</nav>
        {%- endif %}
      </div>
    {% endblock %}
"#};

static ARCHIVE_HTML: &str = indoc! {r#"
    {% extends "base.html" %}

    {% block title %}<title>{{ name }} - {{ config.title }}</title>{% endblock %}

    {% block body %}
      <div class="archive">
        <h1>{{ name }}</h1>
        {%- for group in page_groups %}
          {%- if group.key %}
          <h3>{{ group.key }}</h3>
          {%- endif %}
          <ul>
          {%- for page in group.pages %}
            <li>
              <a href="{{ page.url | safe }}">{{ page.title }}</a>
              {%- if page.date %} ({{ page.date }}){%- endif %}
            </li>
          {%- endfor %}
          </ul>
        {%- endfor %}
        {%- if pagination.total_pages > 1 %}
        <nav class="pagination">
          {%- if pagination.prev_url %}
          <a href="{{ pagination.prev_url | safe }}">← Prev</a>
          {%- endif %}
          <span>Page {{ pagination.current_page }} / {{ pagination.total_pages }}</span>
          {%- for item in pagination.items %}
            {%- if item.number and item.is_current %}
            <span class="active">{{ item.number }}</span>
            {%- elif item.number %}
            <a href="{{ item.url | safe }}">{{ item.number }}</a>
            {%- else %}
            <span>&hellip;</span>
            {%- endif %}
          {%- endfor %}
          {%- if pagination.next_url %}
          <a href="{{ pagination.next_url | safe }}">Next →</a>
          {%- endif %}
        </nav>
        {%- endif %}
      </div>
    {% endblock %}
"#};

static OVERVIEW_HTML: &str = indoc! {r#"
    {% extends "base.html" %}

    {% block title %}<title>{{ kind | capitalize }} - {{ config.title }}</title>{% endblock %}

    {% block body %}
      <h1>All {{ kind }}</h1>

      <ul>
      {%- for bucket in buckets %}
        <li>
          <a href="{{ bucket.url | safe }}">{{ bucket.name }}</a> ({{ bucket.pages | length }})
          {%- for page in bucket.pages[:5] %}
          <a href="{{ page.url | safe }}">{{ page.title }}</a>
          {%- endfor %}
        </li>
      {%- endfor %}
      </ul>
    {% endblock %}
"#};

/// Persistent temp directory holding test templates (lives for the process).
static TEST_TEMPLATE_DIR: LazyLock<TempDir> = LazyLock::new(|| {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("base.html"), BASE_HTML).unwrap();
    fs::write(dir.path().join("post.html"), POST_HTML).unwrap();
    fs::write(dir.path().join("page.html"), PAGE_HTML).unwrap();
    fs::write(dir.path().join("home.html"), HOME_HTML).unwrap();
    fs::write(dir.path().join("archive.html"), ARCHIVE_HTML).unwrap();
    fs::write(dir.path().join("overview.html"), OVERVIEW_HTML).unwrap();
    dir
});

/// Returns the path to the test template directory.
pub fn template_dir() -> PathBuf {
    TEST_TEMPLATE_DIR.path().to_owned()
}

/// Copies the test templates into `dest` as real files on disk.
pub fn copy_templates(dest: &Path) {
    crate::output::copy_static(&template_dir(), dest).unwrap();
}

/// Creates a `TemplateEngine` using embedded test templates.
pub fn test_engine() -> TemplateEngine {
    TemplateEngine::new(None, Some(TEST_TEMPLATE_DIR.path())).unwrap()
}

/// Creates a `Config` with all defaults.
pub fn test_config() -> Config {
    toml::from_str("").unwrap()
}

/// Creates a minimal `Page` with defaults for testing.
///
/// Returns a `PageKind::Page` (standalone) with an empty body.
/// Callers override fields as needed (e.g., `.kind`, `.source_path`).
pub fn test_page(title: &str) -> Page {
    Page {
        frontmatter: Frontmatter {
            title: title.to_owned(),
            ..Frontmatter::default()
        },
        raw_content: String::new(),
        kind: PageKind::Page,
        slug: title.to_lowercase().replace(' ', "-"),
        summary: None,
        source_path: PathBuf::from(format!("content/{title}/index.md")),
        assets: Vec::new(),
    }
}

/// Writes a file at `dir.join(rel_path)`, creating parent directories as needed.
///
/// Generic helper for filesystem-based tests (content discovery, build, etc.).
pub fn write_test_file(dir: &Path, rel_path: &str, content: &str) {
    let path = dir.join(rel_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

/// RAII guard that restores filesystem permissions on drop.
///
/// Ensures cleanup happens even if the test panics, preventing
/// `TempDir::drop` failures from leftover restricted permissions.
pub struct PermissionGuard {
    path: PathBuf,
    mode: u32,
}

impl PermissionGuard {
    pub fn restrict(path: &Path, mode: u32) -> Self {
        let original = fs::metadata(path).unwrap().permissions().mode() & 0o7777;
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
        Self {
            path: path.to_owned(),
            mode: original,
        }
    }
}

impl Drop for PermissionGuard {
    fn drop(&mut self) {
        _ = fs::set_permissions(&self.path, fs::Permissions::from_mode(self.mode));
    }
}
