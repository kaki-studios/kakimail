#!/usr/bin/python3

import imaplib

from dotenv import dotenv_values

import sys
import time


config = dotenv_values("../.env")

if len(sys.argv) < 3:
    print(f"Usage: {sys.argv[0]} HOST PORT")
    exit(1)

start = time.perf_counter()

client = imaplib.IMAP4(sys.argv[1], int(sys.argv[2]))
client.debug = 4


# client.login(config["USERNAME"], config["PASSWORD"])
def callback(bytes):
    print(bytes)
    username = config["USERNAME"]
    password = config["PASSWORD"]
    return f"\0{username}\0{password}".encode()


mail = r"""Subject: test 
Date: 01 Jan 2023 23:59:59 +0000
To: kaki@kaki.foo
teststststststststststststststs
"""

client.starttls()
client.authenticate("PLAIN", callback)
client.list()
client.status("INBOX", "(UIDNEXT MESSAGES)")
client.append("INBOX", "", time.time(), bytes(mail, "utf-8"))
client.select("INBOX", False)
for req in ["SINCE", "ON", "BEFORE"]:
    (typ, [data]) = client.search(None, f"RETURN (MIN COUNT ALL) SENT{req} 14-Jun-2024")
    print(typ)
    print(data)
(typ, [data]) = client.search(None, "RETURN (MIN COUNT ALL) SUBJECT test")
print(typ)
print(data)
client.fetch("1:*", "BODY[]")
client.expunge()
client.close()
client.logout()
end = time.perf_counter()

final_time = end - start
print("FINAL TIME: ", final_time)
