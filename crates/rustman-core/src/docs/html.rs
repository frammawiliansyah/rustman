use super::{auth_text, body_text, curl_snippet, method_str};
use crate::models::collection::{Collection, Item};
use crate::models::http::HttpRequest;

/// Generates a single-file, dependency-free HTML documentation page.
pub struct HtmlDocGenerator;

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

const CSS: &str = r#"
:root{--bg:#0d1117;--panel:#161b22;--border:#30363d;--text:#e6edf3;--muted:#8b949e;--accent:#58a6ff}
*{box-sizing:border-box}
body{margin:0;background:var(--bg);color:var(--text);font:14px/1.5 -apple-system,Segoe UI,Roboto,sans-serif;display:flex;min-height:100vh}
#sidebar{width:300px;flex:none;background:var(--panel);border-right:1px solid var(--border);position:sticky;top:0;height:100vh;overflow-y:auto;padding:16px}
#sidebar h1{font-size:16px;margin:0 0 12px}
#search{width:100%;padding:8px 10px;background:var(--bg);color:var(--text);border:1px solid var(--border);border-radius:6px;margin-bottom:12px}
#sidebar ul{list-style:none;margin:0;padding-left:12px}
#sidebar>ul{padding-left:0}
#sidebar .folder{color:var(--muted);font-size:12px;text-transform:uppercase;letter-spacing:.05em;margin:10px 0 4px}
#sidebar a{display:flex;align-items:center;gap:8px;color:var(--text);text-decoration:none;padding:3px 6px;border-radius:4px;overflow:hidden;white-space:nowrap;text-overflow:ellipsis}
#sidebar a:hover{background:var(--bg)}
main{flex:1;min-width:0;padding:24px 32px;max-width:960px}
main>h1{margin-top:0}
.desc{color:var(--muted)}
.card{background:var(--panel);border:1px solid var(--border);border-radius:8px;padding:16px;margin:16px 0}
.card h3{margin:0 0 8px}
.urlbar{display:flex;gap:10px;align-items:center;background:var(--bg);border:1px solid var(--border);border-radius:6px;padding:8px 10px;font-family:ui-monospace,Menlo,monospace;word-break:break-all}
.badge{display:inline-block;min-width:52px;text-align:center;padding:1px 6px;border-radius:4px;font:700 11px ui-monospace,monospace;color:#fff}
.m-get{background:#2ea043}.m-post{background:#d9822b}.m-put{background:#1f6feb}.m-delete{background:#da3633}.m-patch{background:#8957e5}.m-head,.m-options{background:#6e7681}
table{border-collapse:collapse;width:100%;margin:6px 0 12px}
th,td{text-align:left;border-bottom:1px solid var(--border);padding:4px 8px;font-family:ui-monospace,Menlo,monospace;font-size:12px;word-break:break-all}
h4{margin:12px 0 4px;color:var(--muted);font-size:12px;text-transform:uppercase}
pre{background:var(--bg);border:1px solid var(--border);border-radius:6px;padding:10px;overflow-x:auto;margin:0;font-size:12px}
.tabs{display:flex;gap:4px;margin-bottom:6px}
.tab{background:none;border:1px solid var(--border);color:var(--muted);padding:4px 10px;border-radius:6px;cursor:pointer}
.tab.active{color:var(--text);border-color:var(--accent)}
.pane{display:none;position:relative}.pane.active{display:block}
.copy{position:absolute;top:6px;right:6px;background:var(--panel);color:var(--text);border:1px solid var(--border);border-radius:4px;cursor:pointer;padding:2px 8px}
.hidden{display:none!important}
@media(max-width:800px){body{flex-direction:column}#sidebar{width:100%;height:auto;position:static}main{padding:16px}}
"#;

const JS: &str = r#"
function filterEndpoints(q){
  q=q.trim().toLowerCase();
  document.querySelectorAll('#sidebar li.endpoint').forEach(function(li){
    li.classList.toggle('hidden', q!=='' && li.textContent.toLowerCase().indexOf(q)===-1);
  });
  document.querySelectorAll('#sidebar li.group').forEach(function(g){
    var any=g.querySelector('li.endpoint:not(.hidden)');
    g.classList.toggle('hidden', q!=='' && !any);
  });
}
document.getElementById('search').addEventListener('input',function(e){filterEndpoints(e.target.value)});
document.querySelectorAll('.card').forEach(function(card){
  card.querySelectorAll('.tab').forEach(function(tab){
    tab.addEventListener('click',function(){
      card.querySelectorAll('.tab,.pane').forEach(function(x){x.classList.remove('active')});
      tab.classList.add('active');
      card.querySelector('#'+tab.dataset.pane).classList.add('active');
    });
  });
  var btn=card.querySelector('.copy');
  if(btn){btn.addEventListener('click',function(){
    var t=card.querySelector('.pane[id$="-curl"] code').textContent;
    var done=function(){btn.textContent='Copied!';setTimeout(function(){btn.textContent='Copy'},1200)};
    if(navigator.clipboard){navigator.clipboard.writeText(t).then(done)}
    else{var a=document.createElement('textarea');a.value=t;document.body.appendChild(a);a.select();document.execCommand('copy');a.remove();done()}
  })}
});
"#;

impl HtmlDocGenerator {
    pub fn generate(collection: &Collection) -> String {
        let mut side = String::new();
        let mut main = String::new();
        let mut counter = 0usize;
        Self::walk(&collection.items, &mut side, &mut main, &mut counter);
        if counter == 0 {
            main.push_str("<p class=\"desc\">No endpoints.</p>");
        }
        let desc = collection
            .description
            .as_deref()
            .filter(|d| !d.trim().is_empty())
            .map(|d| format!("<p class=\"desc\">{}</p>", esc(d.trim())))
            .unwrap_or_default();
        let name = esc(&collection.name);
        format!(
            "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\n\
<title>{name} - API Documentation</title>\n<style>{CSS}</style>\n</head>\n<body>\n\
<aside id=\"sidebar\">\n<h1>{name}</h1>\n\
<input id=\"search\" type=\"search\" placeholder=\"Search endpoints...\" autocomplete=\"off\">\n\
<ul>{side}</ul>\n</aside>\n<main>\n<h1>{name}</h1>\n{desc}\n{main}</main>\n\
<script>{JS}</script>\n</body>\n</html>\n"
        )
    }

    fn walk(items: &[Item], side: &mut String, main: &mut String, counter: &mut usize) {
        for item in items {
            match item {
                Item::Folder(f) => {
                    side.push_str(&format!("<li class=\"group\"><div class=\"folder\">{}</div><ul>", esc(&f.name)));
                    main.push_str(&format!("<h2>{}</h2>\n", esc(&f.name)));
                    if let Some(d) = f.description.as_deref().filter(|d| !d.trim().is_empty()) {
                        main.push_str(&format!("<p class=\"desc\">{}</p>\n", esc(d.trim())));
                    }
                    Self::walk(&f.items, side, main, counter);
                    side.push_str("</ul></li>");
                }
                Item::Request(r) => {
                    *counter += 1;
                    let id = format!("ep-{counter}");
                    let m = method_str(r.method);
                    side.push_str(&format!(
                        "<li class=\"endpoint\"><a href=\"#{id}\"><span class=\"badge m-{}\">{m}</span>{}</a></li>",
                        m.to_ascii_lowercase(),
                        esc(&r.name)
                    ));
                    Self::request(r, &id, main);
                }
            }
        }
    }

    fn request(r: &HttpRequest, id: &str, out: &mut String) {
        let m = method_str(r.method);
        out.push_str(&format!(
            "<section class=\"card\" id=\"{id}\">\n<h3>{}</h3>\n\
<div class=\"urlbar\"><span class=\"badge m-{}\">{m}</span><span>{}</span></div>\n",
            esc(&r.name),
            m.to_ascii_lowercase(),
            esc(&r.url)
        ));
        out.push_str(&format!("<h4>Auth</h4><p>{}</p>\n", esc(&auth_text(&r.auth))));
        if !r.headers.is_empty() {
            out.push_str("<h4>Headers</h4><table><tr><th>Key</th><th>Value</th></tr>");
            for h in &r.headers {
                let dis = if h.enabled { "" } else { " (disabled)" };
                out.push_str(&format!("<tr><td>{}</td><td>{}{dis}</td></tr>", esc(&h.key), esc(&h.value)));
            }
            out.push_str("</table>\n");
        }
        if !r.query_params.is_empty() {
            out.push_str("<h4>Query Params</h4><table><tr><th>Key</th><th>Value</th></tr>");
            for q in &r.query_params {
                out.push_str(&format!("<tr><td>{}</td><td>{}</td></tr>", esc(&q.key), esc(&q.value)));
            }
            out.push_str("</table>\n");
        }
        let curl = curl_snippet(r);
        let mut tabs = String::new();
        let mut panes = String::new();
        let mut first = true;
        if let Some((_, text)) = body_text(&r.body) {
            tabs.push_str(&format!("<button class=\"tab active\" data-pane=\"{id}-body\">Body</button>"));
            panes.push_str(&format!("<div class=\"pane active\" id=\"{id}-body\"><pre><code>{}</code></pre></div>", esc(&text)));
            first = false;
        }
        let act = if first { " active" } else { "" };
        tabs.push_str(&format!("<button class=\"tab{act}\" data-pane=\"{id}-curl\">cURL</button>"));
        panes.push_str(&format!(
            "<div class=\"pane{act}\" id=\"{id}-curl\"><button class=\"copy\">Copy</button><pre><code>{}</code></pre></div>",
            esc(&curl)
        ));
        out.push_str(&format!("<div class=\"tabs\">{tabs}</div>\n{panes}\n</section>\n"));
    }
}
