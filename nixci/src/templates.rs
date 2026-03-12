use maud::{html, Markup, PreEscaped, DOCTYPE};

use crate::models::{Build, Repo};

pub fn layout(title: &str, content: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html {
            head {
                title { (title) " — nixci" }
                script src="/static/htmx.min.js" {}
                script src="/static/htmx-sse.js" {}
                style {
                    (PreEscaped(r#"
                        * { margin: 0; padding: 0; box-sizing: border-box; }
                        body { font-family: monospace; max-width: 960px; margin: 0 auto; padding: 1em; }
                        nav { margin-bottom: 1em; padding-bottom: 0.5em; border-bottom: 1px solid #ccc; }
                        nav a { margin-right: 1em; }
                        table { width: 100%; border-collapse: collapse; }
                        th, td { text-align: left; padding: 0.3em 0.5em; border-bottom: 1px solid #eee; }
                        .status-pending { color: #888; }
                        .status-running { color: #07f; }
                        .status-success { color: #0a0; }
                        .status-failure { color: #c00; }
                        .status-cancelled { color: #888; }
                        .status-skipped { color: #888; }
                        pre.log { background: #111; color: #eee; padding: 1em; overflow-x: auto; max-height: 80vh; overflow-y: auto; font-size: 0.85em; }
                        pre.log .stderr { color: #f88; }
                        form { display: inline; }
                        button { cursor: pointer; padding: 0.3em 0.8em; }
                        a { color: #07f; }
                        h1, h2, h3 { margin: 0.5em 0; }
                        .build-meta { margin: 0.5em 0 1em; }
                        .build-meta span { margin-right: 1em; }
                    "#))
                }
            }
            body {
                nav {
                    a href="/" { "nixci" }
                    a href="/repos" { "repos" }
                    a href="/settings" { "settings" }
                }
                (content)
            }
        }
    }
}

pub fn status_class(status: &str) -> &'static str {
    match status {
        "pending" => "status-pending",
        "running" => "status-running",
        "success" => "status-success",
        "failure" => "status-failure",
        "cancelled" => "status-cancelled",
        "skipped" => "status-skipped",
        _ => "",
    }
}

pub fn build_row(build: &Build) -> Markup {
    html! {
        tr id=(format!("build-{}", build.id))
           hx-get=(format!("/partials/build/{}", build.id))
           hx-trigger="every 5s"
           hx-swap="outerHTML"
        {
            td { a href=(format!("/builds/{}", build.id)) { "#" (build.id) } }
            td { code { (build.commit_sha.get(..8).unwrap_or(&build.commit_sha)) } }
            td {
                @if let Some(ref branch) = build.branch {
                    (branch)
                }
                @if let Some(pr) = build.pr_number {
                    " PR #" (pr)
                }
            }
            td { code { (build.flake_attr) } }
            td class=(status_class(&build.status)) { (build.status) }
            td { (build.triggered_by) }
            td {
                @if build.status == "failure" || build.status == "cancelled" {
                    form hx-post=(format!("/builds/{}/retry", build.id)) hx-swap="outerHTML" {
                        button type="submit" { "retry" }
                    }
                }
            }
        }
    }
}

pub fn repo_row(repo: &Repo) -> Markup {
    html! {
        tr {
            td { a href=(format!("/repos/{}", repo.id)) { (repo.full_name) } }
            td { (repo.default_branch) }
            td { @if repo.webhook_active { "active" } @else { "disabled" } }
        }
    }
}

pub fn build_table(builds: &[Build]) -> Markup {
    html! {
        table {
            thead {
                tr {
                    th { "ID" }
                    th { "Commit" }
                    th { "Ref" }
                    th { "Attr" }
                    th { "Status" }
                    th { "Trigger" }
                    th {}
                }
            }
            tbody {
                @for build in builds {
                    (build_row(build))
                }
                @if builds.is_empty() {
                    tr { td colspan="7" { "No builds yet." } }
                }
            }
        }
    }
}

pub fn log_line_html(stream: &str, line: &str) -> Markup {
    let class = if stream == "stderr" { "stderr" } else { "" };
    html! {
        span class=(class) { (line) "\n" }
    }
}
