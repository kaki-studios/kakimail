use crate::utils::*;
use anyhow::*;
use tokio::net::*;

struct SmtpServer {}

impl SmtpServer {
    fn new() -> Self {
        unimplemented!()
    }

    fn send(
        from: String,
        to: String,
        mail: String, /* change!! to a custom datatype like mail */
    ) -> Result<()> {
        Ok(())
        //resolve recipient
        //connect on port 25 using tcp
        //send mail
        //handle errors
    }
}
