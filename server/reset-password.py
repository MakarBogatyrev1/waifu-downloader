#!/usr/bin/env python3
"""Administrative reset on a stopped Waifu Sync instance. Never accepts passwords as arguments."""
import getpass, sqlite3, sys
from argon2 import PasswordHasher
if len(sys.argv)!=3:
    raise SystemExit('Usage: reset-password.py DATABASE_FILE USERNAME (stop the service first)')
from pathlib import Path
database=Path(sys.argv[1]).resolve(strict=True)
username=sys.argv[2].strip().lower()
password=getpass.getpass('New password (10-128 UTF-8 bytes): ')
if not 10<=len(password.encode('utf-8'))<=128:raise SystemExit('Invalid password length')
if password!=getpass.getpass('Repeat password: '):raise SystemExit('Passwords do not match')
hashed=PasswordHasher().hash(password)
with sqlite3.connect(database) as db:
    uid=db.execute('SELECT id FROM users WHERE username=?',(username,)).fetchone()
    if uid is None:raise SystemExit('User not found')
    db.execute('UPDATE users SET password=? WHERE id=?',(hashed,uid[0]))
    db.execute('DELETE FROM sessions WHERE uid=?',(uid[0],))
print('Password changed. All sessions revoked. You may start the service.')
