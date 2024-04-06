use crate::imap::{IMAPOp, IMAPState, ResponseInfo};

pub struct Logout;

impl IMAPOp for Logout {
    async fn process(
        tag: &str,
        _args: &str,
        _state: crate::imap::IMAPState,
        _db: std::sync::Arc<tokio::sync::Mutex<crate::database::DBClient>>,
    ) -> anyhow::Result<(Vec<Vec<u8>>, crate::imap::IMAPState, ResponseInfo)> {
        let mut resp = Vec::new();
        let untagged = "* BYE IMAP4rev2 Server logging out\r\n".as_bytes().to_vec();
        resp.push(untagged);
        let tagged = format!("{} OK LOGOUT completed\r\n", tag)
            .as_bytes()
            .to_vec();
        resp.push(tagged);
        Ok((resp, IMAPState::Logout, ResponseInfo::Regular))
    }
}
