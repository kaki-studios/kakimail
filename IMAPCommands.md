# Progress on imap commands

### any state:
- [X] capability
- [X] noop
- [X] logout
### not authenticated state:
- [X] starttls
- [X] authenticate
- [X] login
### authenticated state:
- [X] enable (no extensions supported)
- [X] select
- [X] examine
- [X] create
- [X] delete
- [X] rename
- [x] subscribe
- [X] unsubscribe
- [X] list (barebones, need more functionality)
- [X] namespace
- [X] status
- [X] append
- [X] idle (imaplib only works for IMAP4rev1, so i can't test IDLE)
### selected state:
- [X] close
- [X] unselect
- [X] expunge
- [ ] search
- [ ] fetch
- [ ] store
- [ ] copy
- [ ] move
- [X] uid
