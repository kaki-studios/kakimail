//NOTE: this is clade-generated code
//haven't checked that much, documentation (rfc9051) is very confusing

use mailparse::{parse_mail, MailHeaderMap, ParsedMail};

use std::error::Error;

use super::fetch;

#[derive(Debug, Clone)]
pub enum BodyStructure {
    Text(TextPart),
    Multipart(MultipartPart),
    Message(MessagePart),
}

#[derive(Debug, Clone)]
pub struct TextPart {
    subtype: String,
    parameters: Vec<(String, String)>,
    id: Option<String>,
    description: Option<String>,
    encoding: String,
    size: usize,
    lines: usize,
    md5: Option<String>,
    disposition: Option<(String, Vec<(String, String)>)>,
    language: Option<Vec<String>>,
    location: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MultipartPart {
    subtype: String,
    parts: Vec<BodyStructure>,
    parameters: Vec<(String, String)>,
    disposition: Option<(String, Vec<(String, String)>)>,
    language: Option<Vec<String>>,
    location: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MessagePart {
    subtype: String,
    parameters: Vec<(String, String)>,
    id: Option<String>,
    description: Option<String>,
    encoding: String,
    size: usize,
    envelope: String, // Simplified for this example
    body: Box<BodyStructure>,
    lines: usize,
    md5: Option<String>,
    disposition: Option<(String, Vec<(String, String)>)>,
    language: Option<Vec<String>>,
    location: Option<String>,
}

pub fn parse_bodystructure(raw_email: &[u8]) -> Result<BodyStructure, Box<dyn Error>> {
    let parsed_mail = parse_mail(raw_email)?;
    Ok(build_bodystructure(&parsed_mail))
}

fn build_bodystructure(part: &ParsedMail) -> BodyStructure {
    match part.ctype.mimetype.split('/').collect::<Vec<_>>()[..] {
        ["text", subtype] => BodyStructure::Text(TextPart {
            subtype: subtype.to_string(),
            parameters: part
                .ctype
                .params
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            id: part.get_headers().get_first_value("Content-ID"),
            description: part.get_headers().get_first_value("Content-Description"),
            encoding: part.ctype.charset.clone(),
            size: part.get_body().map(|i| i.len()).unwrap_or(0),
            lines: part
                .get_body()
                .unwrap_or("".to_string())
                .split(|c| c == '\n')
                .count(),
            md5: part.get_headers().get_first_value("Content-MD5"),
            disposition: parse_content_disposition(part),
            language: parse_content_language(part),
            location: part.get_headers().get_first_value("Content-Location"),
        }),
        ["multipart", subtype] => BodyStructure::Multipart(MultipartPart {
            subtype: subtype.to_string(),
            parts: part.subparts.iter().map(build_bodystructure).collect(),
            parameters: part
                .ctype
                .params
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            disposition: parse_content_disposition(part),
            language: parse_content_language(part),
            location: part.get_headers().get_first_value("Content-Location"),
        }),
        ["message", "rfc822"] => BodyStructure::Message(MessagePart {
            subtype: "rfc822".to_string(),
            parameters: part
                .ctype
                .params
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            id: part.get_headers().get_first_value("Content-ID"),
            description: part.get_headers().get_first_value("Content-Description"),
            encoding: part.ctype.charset.clone(),
            size: part.get_body().map(|i| i.len()).unwrap_or(0),
            envelope: fetch::envelope_to_string(part),
            body: Box::new(build_bodystructure(&part.subparts[0])),
            lines: part
                .get_body()
                .unwrap_or("".to_string())
                .split(|c| c == '\n')
                .count(),
            md5: part.get_headers().get_first_value("Content-MD5"),
            disposition: parse_content_disposition(part),
            language: parse_content_language(part),
            location: part.get_headers().get_first_value("Content-Location"),
        }),
        _ => BodyStructure::Text(TextPart {
            subtype: "plain".to_string(),
            parameters: vec![],
            id: None,
            description: None,
            encoding: "7BIT".to_string(),
            size: part.get_body().map(|i| i.len()).unwrap_or(0),
            lines: part
                .get_body()
                .unwrap_or("".to_string())
                .split(|c| c == '\n')
                .count(),
            md5: None,
            disposition: None,
            language: None,
            location: None,
        }),
    }
}

fn parse_content_disposition(part: &ParsedMail) -> Option<(String, Vec<(String, String)>)> {
    part.get_headers()
        .get_first_value("Content-Disposition")
        .map(|v| {
            let parts: Vec<&str> = v.splitn(2, ';').collect();
            let disposition_type = parts[0].trim().to_string();
            let params = if parts.len() > 1 {
                parts[1]
                    .split(';')
                    .filter_map(|p| {
                        let kv: Vec<&str> = p.splitn(2, '=').map(str::trim).collect();
                        if kv.len() == 2 {
                            Some((kv[0].to_string(), kv[1].trim_matches('"').to_string()))
                        } else {
                            None
                        }
                    })
                    .collect()
            } else {
                vec![]
            };
            (disposition_type, params)
        })
}

fn parse_content_language(part: &ParsedMail) -> Option<Vec<String>> {
    part.get_headers()
        .get_first_value("Content-Language")
        .map(|v| v.split(',').map(|s| s.trim().to_string()).collect())
}

pub fn bodystructure_to_string(structure: &BodyStructure, include_extension: bool) -> String {
    match structure {
        BodyStructure::Text(text) => {
            let mut result = format!(
                r#"("TEXT" "{}" {} {} {} "{}" {} {})"#,
                text.subtype,
                params_to_string(&text.parameters),
                option_to_string(&text.id),
                option_to_string(&text.description),
                text.encoding,
                text.size,
                text.lines
            );
            if include_extension {
                result = result[..result.len() - 1].to_string();
                result.push_str(&format!(
                    " {} {} {} {})",
                    option_to_string(&text.md5),
                    disposition_to_string(&text.disposition),
                    language_to_string(&text.language),
                    option_to_string(&text.location)
                ));
            }
            result
        }
        BodyStructure::Multipart(multipart) => {
            let parts = multipart
                .parts
                .iter()
                .map(|p| bodystructure_to_string(p, include_extension))
                .collect::<Vec<_>>()
                .join(" ");
            let mut result = format!(
                r#"({} "{}" {})"#,
                parts,
                multipart.subtype,
                params_to_string(&multipart.parameters)
            );
            if include_extension {
                result = result[..result.len() - 1].to_string();
                result.push_str(&format!(
                    " {} {} {})",
                    disposition_to_string(&multipart.disposition),
                    language_to_string(&multipart.language),
                    option_to_string(&multipart.location)
                ));
            }
            result
        }
        BodyStructure::Message(message) => {
            let mut result = format!(
                r#"("MESSAGE" "RFC822" {} {} {} "{}" {} {} {} {})"#,
                params_to_string(&message.parameters),
                option_to_string(&message.id),
                option_to_string(&message.description),
                message.encoding,
                message.size,
                message.envelope,
                bodystructure_to_string(&message.body, include_extension),
                message.lines
            );
            if include_extension {
                result = result[..result.len() - 1].to_string();
                result.push_str(&format!(
                    " {} {} {} {})",
                    option_to_string(&message.md5),
                    disposition_to_string(&message.disposition),
                    language_to_string(&message.language),
                    option_to_string(&message.location)
                ));
            }
            result
        }
    }
}

fn params_to_string(params: &[(String, String)]) -> String {
    if params.is_empty() {
        "NIL".to_string()
    } else {
        format!(
            "({})",
            params
                .iter()
                .map(|(k, v)| format!(r#""{}" "{}""#, k, v))
                .collect::<Vec<_>>()
                .join(" ")
        )
    }
}

fn option_to_string(opt: &Option<String>) -> String {
    opt.as_ref()
        .map(|s| format!(r#""{}""#, s))
        .unwrap_or_else(|| "NIL".to_string())
}

fn disposition_to_string(disposition: &Option<(String, Vec<(String, String)>)>) -> String {
    disposition
        .as_ref()
        .map_or("NIL".to_string(), |(disp_type, params)| {
            format!("({} {})", disp_type, params_to_string(params))
        })
}

fn language_to_string(language: &Option<Vec<String>>) -> String {
    language.as_ref().map_or("NIL".to_string(), |langs| {
        if langs.len() == 1 {
            format!(r#""{}""#, langs[0])
        } else {
            format!(
                "({})",
                langs
                    .iter()
                    .map(|l| format!(r#""{}""#, l))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        }
    })
}

pub fn body_to_string(structure: &BodyStructure) -> String {
    bodystructure_to_string(structure, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_text_email() {
        let raw_email = r#"From: sender@example.com
To: recipient@example.com
Subject: Test Email
Content-Type: text/plain; charset="utf-8"
Content-Disposition: inline
Content-Language: en
Content-Location: https://example.com/test.txt

This is a test email."#;

        let structure = parse_bodystructure(raw_email.as_bytes()).unwrap();

        let body_result = body_to_string(&structure);
        assert_eq!(
            body_result,
            r#"("TEXT" "plain" ("charset" "utf-8") NIL NIL "utf-8" 21 1)"#
        );

        let bodystructure_result = bodystructure_to_string(&structure, true);
        assert_eq!(
            bodystructure_result,
            r#"("TEXT" "plain" ("charset" "utf-8") NIL NIL "utf-8" 21 1 NIL (inline NIL) "en" "https://example.com/test.txt")"#
        );
    }

    #[test]
    fn test_multipart_mixed_email() {
        let raw_email = r#"From: sender@example.com
To: recipient@example.com
Subject: Multipart Mixed Email
Content-Type: multipart/mixed; boundary="boundary123"

--boundary123
Content-Type: text/plain; charset="utf-8"
Content-Disposition: inline

This is the text part of the email.

--boundary123
Content-Type: application/pdf; name="document.pdf"
Content-Disposition: attachment; filename="document.pdf"
Content-Transfer-Encoding: base64

JVBERi0xLjMKJcTl8uXrp/Og0MTGCjQgMCBvYmoKPDwgL0xlbmd0aCA1IDAgUiAvRmlsdGVyIC9GbGF0ZURlY29kZSA+PgpzdHJlYW0KeAFLy
--boundary123--"#;

        let structure = parse_bodystructure(raw_email.as_bytes()).unwrap();

        let body_result = body_to_string(&structure);
        assert_eq!(
            body_result,
            r#"(("TEXT" "plain" ("charset" "utf-8") NIL NIL "utf-8" 37 1) ("APPLICATION" "pdf" ("name" "document.pdf") NIL NIL "base64" 96 NIL) "MIXED")"#
        );

        let bodystructure_result = bodystructure_to_string(&structure, true);
        assert_eq!(
            bodystructure_result,
            r#"(("TEXT" "plain" ("charset" "utf-8") NIL NIL "utf-8" 37 1 NIL (inline NIL) NIL NIL) ("APPLICATION" "pdf" ("name" "document.pdf") NIL NIL "base64" 96 NIL NIL (attachment ("filename" "document.pdf")) NIL NIL) "MIXED" NIL NIL NIL NIL)"#
        );
    }

    #[test]
    fn test_nested_multipart_email() {
        let raw_email = r#"From: sender@example.com
To: recipient@example.com
Subject: Nested Multipart Email
Content-Type: multipart/mixed; boundary="outer"

--outer
Content-Type: text/plain; charset="utf-8"

This is the first part of the email.

--outer
Content-Type: multipart/alternative; boundary="inner"

--inner
Content-Type: text/plain; charset="utf-8"

This is the plain text version.

--inner
Content-Type: text/html; charset="utf-8"

<html><body><p>This is the HTML version.</p></body></html>

--inner--

--outer
Content-Type: application/octet-stream
Content-Disposition: attachment; filename="data.bin"

[Binary data would go here]

--outer--"#;

        let structure = parse_bodystructure(raw_email.as_bytes()).unwrap();

        let body_result = body_to_string(&structure);
        assert_eq!(
            body_result,
            r#"(("TEXT" "plain" ("charset" "utf-8") NIL NIL "utf-8" 39 1) (("TEXT" "plain" ("charset" "utf-8") NIL NIL "utf-8" 31 1) ("TEXT" "html" ("charset" "utf-8") NIL NIL "utf-8" 61 1) "ALTERNATIVE") ("APPLICATION" "octet-stream" NIL NIL NIL "7BIT" 27 NIL) "MIXED")"#
        );

        let bodystructure_result = bodystructure_to_string(&structure, true);
        assert_eq!(
            bodystructure_result,
            r#"(("TEXT" "plain" ("charset" "utf-8") NIL NIL "utf-8" 39 1 NIL NIL NIL NIL) (("TEXT" "plain" ("charset" "utf-8") NIL NIL "utf-8" 31 1 NIL NIL NIL NIL) ("TEXT" "html" ("charset" "utf-8") NIL NIL "utf-8" 61 1 NIL NIL NIL NIL) "ALTERNATIVE" NIL NIL NIL NIL) ("APPLICATION" "octet-stream" NIL NIL NIL "7BIT" 27 NIL NIL (attachment ("filename" "data.bin")) NIL NIL) "MIXED" NIL NIL NIL NIL)"#
        );
    }

    #[test]
    fn test_message_rfc822_email() {
        let raw_email = r#"From: sender@example.com
To: recipient@example.com
Subject: Forwarded Message
Content-Type: message/rfc822

From: original@example.com
To: recipient@example.com
Subject: Original Message
Content-Type: text/plain

This is the content of the original message."#;

        let structure = parse_bodystructure(raw_email.as_bytes()).unwrap();

        let body_result = body_to_string(&structure);
        assert_eq!(
            body_result,
            r#"("MESSAGE" "RFC822" NIL NIL NIL "7BIT" 185 ("TODO: Implement envelope parsing" ("TEXT" "plain" NIL NIL NIL "7BIT" 46 1) 46) 5)"#
        );

        let bodystructure_result = bodystructure_to_string(&structure, true);
        assert_eq!(
            bodystructure_result,
            r#"("MESSAGE" "RFC822" NIL NIL NIL "7BIT" 185 ("TODO: Implement envelope parsing" ("TEXT" "plain" NIL NIL NIL "7BIT" 46 1 NIL NIL NIL NIL) 46) 5 NIL NIL NIL NIL)"#
        );
    }

    #[test]
    fn test_empty_multipart_email() {
        let raw_email = r#"From: sender@example.com
To: recipient@example.com
Subject: Empty Multipart Email
Content-Type: multipart/mixed; boundary="boundary"

--boundary
--boundary--"#;

        let structure = parse_bodystructure(raw_email.as_bytes()).unwrap();

        let body_result = body_to_string(&structure);
        assert_eq!(body_result, r#"("TEXT" "plain" NIL NIL NIL "7BIT" 0 0)"#);

        let bodystructure_result = bodystructure_to_string(&structure, true);
        assert_eq!(
            bodystructure_result,
            r#"("TEXT" "plain" NIL NIL NIL "7BIT" 0 0 NIL NIL NIL NIL)"#
        );
    }
}
