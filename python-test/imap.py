#!/usr/bin/python3

import imaplib

from dotenv import dotenv_values

import sys
import time


config = dotenv_values("../.env")

if len(sys.argv) < 3:
    print(f"Usage: {sys.argv[0]} HOST PORT")
    exit(1)

client = imaplib.IMAP4(sys.argv[1], int(sys.argv[2]))
client.debug = 4


# client.login(config["USERNAME"], config["PASSWORD"])
def callback(bytes):
    print(bytes)
    username = config["USERNAME"]
    password = config["PASSWORD"]
    return f"\0{username}\0{password}".encode()


mail = b"""From: Tom Test <test@example.com>
To: test@kaki.foo
testing
bye"""

client.starttls()
client.authenticate("PLAIN", callback)
client.list()
client.status("INBOX", "(UIDNEXT MESSAGES)")
client.append("INBOX", "", time.time(), mail)
client.select("INBOX", False)
(typ, [data]) = client.search(None, "RETURN (MIN COUNT) ALL")
print(typ)
print(data)
client.expunge()
client.close()
client.logout()
