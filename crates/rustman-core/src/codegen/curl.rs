use super::{form_encode, PBody, Prepared};

/// Single-quote a string for POSIX shells.
pub(crate) fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

pub(crate) fn generate(p: &Prepared) -> String {
    let mut parts: Vec<String> = Vec::new();
    if p.method == "HEAD" {
        parts.push(format!("curl -I {}", sh_quote(&p.url)));
    } else {
        parts.push(format!("curl -X {} {}", p.method, sh_quote(&p.url)));
    }
    if let Some((u, pw)) = &p.basic {
        parts.push(format!("-u {}", sh_quote(&format!("{u}:{pw}"))));
    }
    for (k, v) in &p.headers {
        parts.push(format!("-H {}", sh_quote(&format!("{k}: {v}"))));
    }
    match &p.body {
        PBody::None => {}
        PBody::Raw(b) => parts.push(format!("-d {}", sh_quote(b))),
        PBody::UrlEncoded(f) => parts.push(format!("-d {}", sh_quote(&form_encode(f)))),
        PBody::Multipart(f) => {
            for (k, v) in f {
                parts.push(format!("--form-string {}", sh_quote(&format!("{k}={v}"))));
            }
        }
    }
    parts.join(" \\\n  ")
}
