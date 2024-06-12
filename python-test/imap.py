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


mail = r"""Received: by mail-yb1-f180.google.com with SMTP id 3f1490d57ef6-dfdff9771f8so879870276.1 for ; Wed, 12 Jun 2024 02:03:27 -0700 (PDT) DKIM-Signature: v=1; a=rsa-sha256; c=relaxed/relaxed; d=gmail.com; s 230601; t18183006; x18787806; darn=idont.date; h=to:subject:message-id:date:from:mime-version:from:to:cc:subject :date:message-id:reply-to; bh=5r0fqoeu2ACiR0HfDQpC+rNs84xo/bn2Y1Piq5mV+fY=; bõI7D2eGGY1etz73OoCk3T1CRiD5olPqnsxdeZs7o49IZ9TODg8iG1+PeSxQI7bk/d je0KnCFsYycRT+tvNC2OwKNN7RAPIuT4J3rvmvoXQTK3rA0M1jzpWwsABFn7hUzs8Lmw b/2YvAw1Cz1vXPaqJW+8Jp3QY47bMFgA+sYzysjjKd3HubzriyA3Hnmry+M0Ox6VgzVq ypBtApCaXiVThyw0oppwpqTjIt0gfnsufpwPIf3gFVFEX1Rde0T+opK0NWov4RYfcACt cYuq2u5aefUNKzbyto635jaorPKXb3Eo2JJTA+WcbqFC+TkSytgaQyq/jqn+J2CWKP9b Uc5w=X-Google-DKIM-Signature: v=1; a=rsa-sha256; c=relaxed/relaxed; d100.net; s 230601; t18183006; x18787806; h=to:subject:message-id:date:from:mime-version:x-gm-message-state :from:to:cc:subject:date:message-id:reply-to; bh=5r0fqoeu2ACiR0HfDQpC+rNs84xo/bn2Y1Piq5mV+fY=; b=LzK87Ug/964OZ+GgN8htFhBsAQqE+xDLnAM/UbjsuDmCYpncuec36CMRtycDKBizG0 YOJtn7UIP34pgbIqM01Y4aoBP4gmgSEk0Wpc5xhaPpP4ph/aLpoYu4qQoCc5yKBhF4z+ SWQAVszKrMc3Mo4BuJSwUbYDpkqOLkrdvwjG2yYVFo21SanWUjs9WqJKo845fEnJf6Eb 5tBomBI6rf8SN6uCqO8eIWVhDHU9diXMcG1D30jE5wuRht/XXoM679LGT+JARVmNkwA6 zzWjAzsZ9GxGqLep8r9e498MQZeMc7fjb5zoNovUaakli3hc8/MsDpKHR1bjHPp9Xhx9 +3Lw=X-Gm-Message-State: AOJu0YzdQyUKWg6oEKW4erYP++1MEZJIeyCm/emaRkKQuvY61FOOBZQA BIoeYV5pVkYOSuKaqtRYvMImSLqIRmBlhKB+YKwznCkZEjt7qBkgLLbytAATefY6hH5Cb7s2TFX cPjCmx7SHGSwYqw1qECBO/vjAJApFzA=X-Google-Smtp-Source: AGHT+IHGs3uOwOvlu7HS+Sq4LF3t9ZfrlhKMpzPz+ct+FQF8yZDGMf+f4ONuINBpOIbd/asDlJF+8OAH2oEdupGYLEMX-Received: by 2002:a5b:dc1:0:b0:de6:3d8:3deb with SMTP id 3f1490d57ef6-dfe66b5fe5emr1141354276.21.1718183006353; Wed, 12 Jun 2024 02:03:26 -0700 (PDT) MIME-Version: 1.0 From: =?UTF-8?Q?Kaarlo_KirvelÃ¤?= Date: Wed, 12 Jun 2024 12:04:58 +0300 Message-ID: Subject: test To: kaki-studios@idont.date Content-Type: multipart/alternative; boundary="000000000000c0c13a061aada7e1" --000000000000c0c13a061aada7e1 Content-Type: text/plain; charset="UTF-8" teststststststststststststststs --000000000000c0c13a061aada7e1 Content-Type: text/html; charset="UTF-8"
teststststststststststststststs
--000000000000c0c13a061aada7e1-- . """

client.starttls()
client.authenticate("PLAIN", callback)
client.list()
client.status("INBOX", "(UIDNEXT MESSAGES)")
client.append("INBOX", "", time.time(), bytes(mail, "utf-8"))
client.select("INBOX", False)
(typ, [data]) = client.search(None, "RETURN (MIN COUNT ALL) TO test SENTON 12-Jun-2024")
print(typ)
print(data)
client.expunge()
client.close()
client.logout()
end = time.perf_counter()

final_time = end - start
print("FINAL TIME: ", final_time)
