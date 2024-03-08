#!/usr/bin/python3

import imaplib

from dotenv import dotenv_values

import sys


config = dotenv_values("../.env")

if len(sys.argv) < 3:
    print(f"Usage: {sys.argv[0]} HOST PORT")
    exit(1)

client = imaplib.IMAP4(sys.argv[1], int(sys.argv[2]))
client.debug = 4


# client.login(config["USERNAME"], config["PASSWORD"])
def callback(bytes):
    username = config["USERNAME"]
    password = config["PASSWORD"]
    return f"\0{username}\0{password}".encode()


client.authenticate("PLAIN", callback)
client.list()
client.select("INBOX", False)
client.capability()
client.noop()
client.close()
client.logout()
