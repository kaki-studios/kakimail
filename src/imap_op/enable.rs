use crate::imap::{IMAPOp, Response, ResponseInfo};

pub struct Enable;

impl IMAPOp for Enable {
    async fn process(
        tag: &str,
        _args: &str,
        state: crate::imap::IMAPState,
        _db: std::sync::Arc<tokio::sync::Mutex<crate::database::DBClient>>,
    ) -> anyhow::Result<(Response, crate::imap::IMAPState, crate::imap::ResponseInfo)> {
        let response = format!("{} BAD NO EXTENSIONS SUPPORTED\r\n", tag);
        Ok((
            vec![response.as_bytes().to_vec()],
            state,
            ResponseInfo::Regular,
        ))
    }
}
