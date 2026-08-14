# Local account transfer

`decodex-database-transfer` is a one-shot upgrade tool. It reads a bounded,
credential-negative account snapshot from standard input and reads the retired
`server/credentials.redb` vault without modifying it. It imports the exact account,
credential, quota, and routing tuple into `server/decodex.sqlite3` in one transaction.

The normal daemon and fresh installer do not invoke this tool when no retired vault is
present. The tool never prints credential values. A successful transfer retains the
source vault for rollback.
