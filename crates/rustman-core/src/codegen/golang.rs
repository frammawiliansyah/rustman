use super::{dq_escape, PBody, Prepared};

fn lit(s: &str) -> String {
    dq_escape(s, false)
}

pub(crate) fn generate(p: &Prepared) -> String {
    let mut imports = vec!["fmt", "io", "net/http"];
    match &p.body {
        PBody::None => {}
        PBody::Raw(_) => imports.push("strings"),
        PBody::UrlEncoded(_) => {
            imports.push("strings");
            imports.push("net/url");
        }
        PBody::Multipart(_) => {
            imports.push("bytes");
            imports.push("mime/multipart");
        }
    }
    imports.sort();

    let mut o = String::from("package main\n\nimport (\n");
    for i in imports {
        o.push_str(&format!("\t\"{i}\"\n"));
    }
    o.push_str(")\n\nfunc main() {\n");

    let body_arg = match &p.body {
        PBody::None => "nil".to_string(),
        PBody::Raw(b) => format!("strings.NewReader({})", lit(b)),
        PBody::UrlEncoded(f) => {
            o.push_str("\tform := url.Values{}\n");
            for (k, v) in f {
                o.push_str(&format!("\tform.Add({}, {})\n", lit(k), lit(v)));
            }
            o.push('\n');
            "strings.NewReader(form.Encode())".to_string()
        }
        PBody::Multipart(f) => {
            o.push_str("\tpayload := &bytes.Buffer{}\n\twriter := multipart.NewWriter(payload)\n");
            for (k, v) in f {
                o.push_str(&format!("\tif err := writer.WriteField({}, {}); err != nil {{\n\t\tpanic(err)\n\t}}\n", lit(k), lit(v)));
            }
            o.push_str("\tif err := writer.Close(); err != nil {\n\t\tpanic(err)\n\t}\n\n");
            "payload".to_string()
        }
    };

    o.push_str(&format!(
        "\treq, err := http.NewRequest({}, {}, {})\n\tif err != nil {{\n\t\tpanic(err)\n\t}}\n",
        lit(p.method),
        lit(&p.url),
        body_arg
    ));
    for (k, v) in &p.headers {
        o.push_str(&format!("\treq.Header.Set({}, {})\n", lit(k), lit(v)));
    }
    if let Some((u, pw)) = &p.basic {
        o.push_str(&format!("\treq.SetBasicAuth({}, {})\n", lit(u), lit(pw)));
    }
    if matches!(p.body, PBody::Multipart(_)) {
        o.push_str("\treq.Header.Set(\"Content-Type\", writer.FormDataContentType())\n");
    }
    o.push_str(
        "\n\tres, err := http.DefaultClient.Do(req)\n\tif err != nil {\n\t\tpanic(err)\n\t}\n\tdefer res.Body.Close()\n\n\
\tbody, err := io.ReadAll(res.Body)\n\tif err != nil {\n\t\tpanic(err)\n\t}\n\tfmt.Println(res.Status)\n\tfmt.Println(string(body))\n}\n",
    );
    o
}
