use super::{dq_escape, PBody, Prepared};

fn lit(s: &str) -> String {
    dq_escape(s, true)
}

pub(crate) fn generate(p: &Prepared) -> String {
    let mut o = String::from(
        "// Cargo.toml: reqwest = { version = \"0.12\", features = [\"multipart\"] }, tokio = { version = \"1\", features = [\"full\"] }\n\
#[tokio::main]\nasync fn main() -> Result<(), reqwest::Error> {\n    let client = reqwest::Client::new();\n",
    );
    if let PBody::Multipart(f) = &p.body {
        o.push_str("    let form = reqwest::multipart::Form::new()");
        for (k, v) in f {
            o.push_str(&format!("\n        .text({}, {})", lit(k), lit(v)));
        }
        o.push_str(";\n");
    }
    o.push_str(&format!(
        "    let response = client\n        .request(reqwest::Method::{}, {})",
        p.method,
        lit(&p.url)
    ));
    for (k, v) in &p.headers {
        o.push_str(&format!("\n        .header({}, {})", lit(k), lit(v)));
    }
    if let Some((u, pw)) = &p.basic {
        o.push_str(&format!("\n        .basic_auth({}, Some({}))", lit(u), lit(pw)));
    }
    match &p.body {
        PBody::None => {}
        PBody::Raw(b) => o.push_str(&format!("\n        .body({})", lit(b))),
        PBody::UrlEncoded(f) => {
            let items = f.iter().map(|(k, v)| format!("({}, {})", lit(k), lit(v))).collect::<Vec<_>>().join(", ");
            o.push_str(&format!("\n        .form(&[{items}])"));
        }
        PBody::Multipart(_) => o.push_str("\n        .multipart(form)"),
    }
    o.push_str(
        "\n        .send()\n        .await?;\n\n    println!(\"{}\", response.status());\n    println!(\"{}\", response.text().await?);\n    Ok(())\n}\n",
    );
    o
}
