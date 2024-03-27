# to-do list of features that need to be implemented

- [X] proper dockerfile and deployment
- [X] email receiving
- [X] testing (using python scripts hehe)
- [X] email sending (at least to kakimail)
- [X] enforce auth when submitting email through port 587
- [X] some kind of database
- [ ] IMAP
    - [ ] split `imap.rs` into multiple files
    - [x] support for multiple mailboxes and namespacing (see next example)
    - [ ] support for command arguments inside quotes, eg `A000 SUBSCRIBE "Personal/School Stuff"`
- [ ] support for some SMTP extensions **MOST NOTABLY STARTTLS**
- [ ] support for implicit tls ports
- [ ] TLS with [tokio-rustls](https://crates.io/crates/tokio-rustls)
    - [ ] use certs from porkbun (use their api to retrieve it)



other stuff?? add here!
