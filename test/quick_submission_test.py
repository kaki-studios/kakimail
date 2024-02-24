#!/usr/bin/python3

import smtplib
import sys

from dotenv import dotenv_values

config = dotenv_values("../.env")

to_addr = "testto@kaki.foo"
from_addr = "testfrom@kaki.foo"

# Add the From: and To: headers at the start!
msg = f"From: {from_addr}\r\nTo: {to_addr}\r\n\r\n"
msg += "test \nmail\n goodbye\n from kaarlo submitted with auth!\n"

if len(sys.argv) < 3:
    print(f"Usage: {sys.argv[0]} HOST PORT")
    exit(1)

server = smtplib.SMTP(sys.argv[1], port=int(sys.argv[2]))
server.set_debuglevel(1)
server.login(config["USERNAME"], config["PASSWORD"])
server.noop()
server.sendmail(from_addr, to_addr, msg)
server.quit()
