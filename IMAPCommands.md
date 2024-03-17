# Progress on imap commands

### any state:
- [X] capability
- [X] noop
- [X] logout
### not authenticated state:
- [ ] starttls
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
- [x] unsubscribe
- [X] list (barebones, need more functionality)
- [X] namespace
- [X] status
- [ ] append
- [ ] idle
### selected state:
- [X] close
- [X] unselect
- [X] expunge
- [ ] search
- [ ] store
- [ ] copy
- [ ] move
- [X] uid
