# Configuration file for the Sphinx documentation builder.
#
# Mirrors the Greenbone Community documentation setup
# (https://github.com/greenbone/docs @ 53ab3ba090d9): Sphinx + MyST Markdown + Furo.
# The theme options below, custom.css, the logos and the favicon are copied
# from that repository unchanged; Greenbone publishes no shared theme package.
# https://www.sphinx-doc.org/en/master/usage/configuration.html

# -- Project information -----------------------------------------------------

project = "OpenVAS-MCP"
copyright = "2026, Greenbone AG"
author = "Greenbone AG"

# -- General configuration ---------------------------------------------------

extensions = [
    "sphinx.ext.githubpages",
    "myst_parser",
    "sphinx_copybutton",
    "sphinx_tabs.tabs",
    "sphinxext.opengraph",
]

templates_path = ["_templates"]

exclude_patterns = [
    "_build",
    "Thumbs.db",
    ".DS_Store",
    ".venv",
    ".vscode",
    "dist",
    # Internal notes kept out of the user documentation.
    "rust-mcp-server-roadmap.md",
    "README.md",
    # v0.2.1 was tagged but published without assets; v0.2.2 supersedes it.
    "releases/v0.2.1.md",
    # Contributor documentation stays in the repository, not on the user site.
    "development.md",
    "gateway-sync.md",
]

source_suffix = {
    ".rst": "restructuredtext",
    ".md": "markdown",
}

# -- Options for HTML output -------------------------------------------------

html_theme = "furo"
html_title = "OpenVAS-MCP Documentation"

html_favicon = "favicon.png"

html_static_path = ["_static"]
html_css_files = ["custom.css"]

repo_url = "https://github.com/greenbone-hive/OpenVAS-MCP/"
html_theme_options = {
    "source_repository": repo_url,
    "source_branch": "main",
    "source_directory": "docs/",
    "light_logo": "logo-light.svg",
    "dark_logo": "logo-dark.svg",
    "light_css_variables": {
        "color-content-foreground": "#4D4D4D",
        "color-foreground-primary": "4D4D4D",
        "color-foreground-secondary": "#808080",
        "color-code-background": "#333333",
        "color-code-foreground": "#E6E6E6",
        "color-admonition-title--note": "#0E823E",
        "admonition-font-size": "0.9rem",
        "color-background-primary": "#FFFFFF",
        "color-background-secondary": "#F5F5F5",
        "color-sidebar-background": "#F5F5F5",
        "color-link--hover": "#0E823E",
    },
    "dark_css_variables": {
        "color-content-foreground": "#F5F5F5",
        "color-foreground-primary": "F5F5F5",
        "color-foreground-secondary": "#E6E6E6",
        "color-code-background": "#333333",
        "color-code-foreground": "#E6E6E6",
        "color-admonition-title--note": "#11AB51",
        "admonition-font-size": "0.9rem",
        "color-background-primary": "#171717",
        "color-background-secondary": "#4D4D4D",
        "color-sidebar-background": "#333333",
        "color-link--hover": "#11AB51",
    },
    "footer_icons": [
        {
            "name": "GitHub",
            "url": repo_url,
            "html": """
                <svg stroke="currentColor" fill="currentColor" stroke-width="0" viewBox="0 0 16 16">
                    <path fill-rule="evenodd" d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0 0 16 8c0-4.42-3.58-8-8-8z"></path>
                </svg>
            """,
            "class": "",
        },
    ],
}

myst_html_meta = {
    "description lang=en": "OpenVAS-MCP – MCP server for Greenbone Vulnerability Management",
    "keywords": "OpenVAS, Greenbone, GVM, MCP, Model Context Protocol, AI agents, vulnerability management",
    "property=og:locale": "en_US",
}

# https://myst-parser.readthedocs.io/en/latest/syntax/optional.html#auto-generated-header-anchors
myst_heading_anchors = 3

myst_enable_extensions = ["colon_fence", "deflist", "fieldlist"]

pygments_style = "zenburn"

suppress_warnings = ["myst.header"]

# settings for sphinxext-opengraph
ogp_site_url = "https://greenbone-hive.github.io/OpenVAS-MCP/latest/"

copybutton_selector = "div:not(.no-copybutton) > div.highlight > pre"
copybutton_prompt_text = r"\$ "
copybutton_prompt_is_regexp = True

# linkcheck: GitHub rate-limits anonymous requests and the private GHCR pages
# answer 404 without a session.
linkcheck_ignore = [
    r"https://github\.com/greenbone-hive/OpenVAS-MCP/(pkgs|settings|actions)/.*",
    r"https://ghcr\.io/.*",
]
