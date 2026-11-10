use super::{base64, PBody, Prepared};

fn lit(s: &str) -> String {
    serde_json::to_string(s).expect("string serializes")
}

fn pairs(f: &[(String, String)]) -> String {
    f.iter().map(|(k, v)| format!("[{}, {}]", lit(k), lit(v))).collect::<Vec<_>>().join(", ")
}

pub(crate) fn generate(p: &Prepared) -> String {
    let mut headers = p.headers.clone();
    if let Some((u, pw)) = &p.basic {
        headers.push(("Authorization".into(), format!("Basic {}", base64(&format!("{u}:{pw}")))));
    }

    let mut o = String::from("async function main() {\n");
    if let PBody::Multipart(f) = &p.body {
        o.push_str("  const form = new FormData();\n");
        for (k, v) in f {
            o.push_str(&format!("  form.append({}, {});\n", lit(k), lit(v)));
        }
        o.push('\n');
    }
    o.push_str(&format!("  const response = await fetch({}, {{\n", lit(&p.url)));
    o.push_str(&format!("    method: {},\n", lit(p.method)));
    if !headers.is_empty() {
        o.push_str("    headers: {\n");
        for (k, v) in &headers {
            o.push_str(&format!("      {}: {},\n", lit(k), lit(v)));
        }
        o.push_str("    },\n");
    }
    match &p.body {
        PBody::None => {}
        PBody::Raw(b) => o.push_str(&format!("    body: {},\n", lit(b))),
        PBody::UrlEncoded(f) => o.push_str(&format!("    body: new URLSearchParams([{}]),\n", pairs(f))),
        PBody::Multipart(_) => o.push_str("    body: form,\n"),
    }
    o.push_str("  });\n\n  const data = await response.text();\n  console.log(response.status, data);\n}\n\nmain().catch(console.error);\n");
    o
}
