use super::{PBody, Prepared};

fn lit(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            '"' => o.push_str("\\\""),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => o.push_str(&format!("\\x{:02x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn pairs(f: &[(String, String)]) -> String {
    f.iter().map(|(k, v)| format!("({}, {})", lit(k), lit(v))).collect::<Vec<_>>().join(", ")
}

pub(crate) fn generate(p: &Prepared) -> String {
    let mut o = String::from("import requests\n\n");
    o.push_str(&format!("url = {}\n", lit(&p.url)));
    if !p.headers.is_empty() {
        o.push_str("headers = {\n");
        for (k, v) in &p.headers {
            o.push_str(&format!("    {}: {},\n", lit(k), lit(v)));
        }
        o.push_str("}\n");
    }
    let mut args = vec![format!("{}, url", lit(p.method))];
    if !p.headers.is_empty() {
        args.push("headers=headers".into());
    }
    if let Some((u, pw)) = &p.basic {
        args.push(format!("auth=({}, {})", lit(u), lit(pw)));
    }
    match &p.body {
        PBody::None => {}
        PBody::Raw(b) if b.is_ascii() => args.push(format!("data={}", lit(b))),
        PBody::Raw(b) => args.push(format!("data={}.encode(\"utf-8\")", lit(b))),
        PBody::UrlEncoded(f) => args.push(format!("data=[{}]", pairs(f))),
        PBody::Multipart(f) => {
            let items = f
                .iter()
                .map(|(k, v)| format!("({}, (None, {}))", lit(k), lit(v)))
                .collect::<Vec<_>>()
                .join(", ");
            args.push(format!("files=[{items}]"));
        }
    }
    o.push_str(&format!("\nresponse = requests.request({})\n", args.join(", ")));
    o.push_str("print(response.status_code)\nprint(response.text)\n");
    o
}
