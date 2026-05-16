use anyhow::anyhow;

use crate::{
    imap::{IMAPOp, IMAPState, ResponseInfo},
    parsing,
};

pub struct List;

impl IMAPOp for List {
    async fn process(
        tag: &str,
        args: &str,
        state: crate::imap::IMAPState,
        db: std::sync::Arc<tokio::sync::Mutex<crate::database::DBClient>>,
    ) -> anyhow::Result<(
        crate::imap::Response,
        crate::imap::IMAPState,
        crate::imap::ResponseInfo,
    )> {
        let id = match state {
            IMAPState::Authed(id) => id,
            IMAPState::Selected(x) => x.user_id,
            _ => return Err(anyhow!("bad state")),
        };
        let parsed =
            parsing::imap::parse_list(args).map_err(|e| anyhow!("invalid LIST args: {:?}", e))?;
        let reference = parsed.get(0).map(String::as_str).unwrap_or("");
        let pattern = parsed.get(1).map(String::as_str).unwrap_or("*");
        let mut full_pattern = if reference.is_empty() || pattern.eq_ignore_ascii_case("INBOX") {
            pattern.to_string()
        } else if pattern.is_empty() {
            reference.to_string()
        } else {
            format!("{}/{}", reference.trim_end_matches('/'), pattern)
        };
        if full_pattern.is_empty() {
            full_pattern = "*".to_string();
        }

        let mailboxes = db.lock().await.get_mailboxes_for_user(id).await?;
        let mut response = Vec::new();
        if pattern.is_empty() {
            response.push(b"* LIST (\\Noselect) \"/\" \"\"\r\n".to_vec());
        } else {
            for (mailbox, subscribed) in mailboxes {
                if mailbox_matches(&mailbox, &full_pattern) {
                    let mut attrs = Vec::new();
                    if subscribed {
                        attrs.push("\\Subscribed");
                    }
                    response.push(
                        format!(
                            "* LIST ({}) \"/\" {}\r\n",
                            attrs.join(" "),
                            parsing::imap::quote_string(&mailbox)
                        )
                        .into_bytes(),
                    );
                }
            }
        }
        response.push(format!("{} OK LIST completed\r\n", tag).into_bytes());
        Ok((response, state, ResponseInfo::Regular))
    }
}

fn mailbox_matches(name: &str, pattern: &str) -> bool {
    if pattern.eq_ignore_ascii_case("INBOX") {
        return name.eq_ignore_ascii_case("INBOX");
    }
    wildcard_match(name.as_bytes(), pattern.as_bytes())
}

fn wildcard_match(name: &[u8], pattern: &[u8]) -> bool {
    match pattern.split_first() {
        None => name.is_empty(),
        Some((&b'*', rest)) => {
            wildcard_match(name, rest) || (!name.is_empty() && wildcard_match(&name[1..], pattern))
        }
        Some((&b'%', rest)) => {
            wildcard_match(name, rest)
                || (!name.is_empty() && name[0] != b'/' && wildcard_match(&name[1..], pattern))
        }
        Some((&p, rest)) => !name.is_empty() && p == name[0] && wildcard_match(&name[1..], rest),
    }
}
