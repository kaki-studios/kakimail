# to-do list of features that need to be implemented

- [X] proper dockerfile and deployment
- [X] email receiving
- [X] testing (using python scripts hehe)
- [X] email sending (at least to kakimail)
- [X] enforce auth when submitting email through port 587
- [X] some kind of database
- [ ] IMAP
    - [X] split `imap.rs` into multiple files
    - [x] support for multiple mailboxes and namespacing (see next example)
    - [X] support for command arguments inside quotes, eg `A000 SUBSCRIBE "Personal/School Stuff"` (did it dirtily)
    - [ ] all commands
- [X] support for some SMTP extensions **MOST NOTABLY STARTTLS**
- [X] support for implicit tls ports
- [X] TLS with [tokio-rustls](https://crates.io/crates/tokio-rustls)
    - [X] use certs from porkbun (use their api to retrieve it)
- [ ] dmarc, dkim and spf or whatever
- [ ] parsing with nom (for difficult commands)

other stuff?? add here!
