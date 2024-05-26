use std::{char, str::FromStr};

use crate::{
    imap_op::search::{ReturnOptions, SearchKeys},
    parsing::imap::SearchArgs,
    smtp_common::Mail,
};
use anyhow::{anyhow, Context, Result};
use chrono::FixedOffset;
use libsql_client::{args, client::Client, Statement, Value};
use tokio::sync::mpsc::Sender;

pub struct DBClient {
    db: Client,
    changes: tokio::sync::mpsc::Sender<String>,
}

impl DBClient {
    /// Creates a new database client.
    /// If the LIBSQL_CLIENT_URL environment variable is not set, a local database will be used.
    /// It's also possible to use a remote database by setting the LIBSQL_CLIENT_URL environment variable.
    /// The `mail` table will be automatically created if it does not exist.
    pub async fn new(tx: Sender<String>) -> Result<Self> {
        if std::env::var("LIBSQL_CLIENT_URL").is_err() {
            let mut db_path_buf = std::env::current_dir()?;
            db_path_buf.push("data/kakimail.db");
            let db_path = db_path_buf.display();
            tracing::warn!("LIBSQL_CLIENT_URL not set, using a default local database: {db_path}");
            std::env::set_var("LIBSQL_CLIENT_URL", format!("file://{db_path}"));
        }
        let db = libsql_client::Client::from_env().await?;

        //USERS TABLE, just in case kakimail-website didn't create it already
        db.batch([
            "PRAGMA foreign_keys = ON",
            "CREATE TABLE IF NOT EXISTS users (id INTEGER PRIMARY KEY, name TEXT UNIQUE, password TEXT);",
            "CREATE INDEX IF NOT EXISTS users_name ON users(name);",
            "CREATE INDEX IF NOT EXISTS users_id ON users(id);",
            //TESTING PURPOSES ONLY
            "INSERT OR IGNORE INTO users VALUES (0, 'test', 'nothashed')"
        ])
        .await.map_err(|e| {
                tracing::error!("1. {:?}", e);
                e
            })?;

        //MAILBOX TABLE
        db.batch([
            "PRAGMA foreign_keys = ON",
            "CREATE TABLE IF NOT EXISTS mailboxes (id integer primary key not null, name text, user_id integer not null, flags integer, FOREIGN KEY(user_id) REFERENCES users(id));",
            "CREATE INDEX IF NOT EXISTS mailbox_foreign_key ON mailboxes(user_id);",
            "CREATE INDEX IF NOT EXISTS mailbox_id ON mailboxes(id);",
            //TESTING PURPOSES ONLY
            "INSERT OR IGNORE INTO mailboxes VALUES (0, 'INBOX', 0, 0)"
        ])
        .await.map_err(|e| {
                tracing::error!("2. {:?}", e);
                e
            })?;

        //MAIL TABLE
        db.batch([
            "CREATE TABLE IF NOT EXISTS mail (uid integer unique not null, seqnum integer not null, date text, sender text, recipients text, data text, 
             mailbox_id integer not null, flags varchar(5), FOREIGN KEY(mailbox_id) REFERENCES mailboxes(id), PRIMARY KEY(uid));",
            //                                  varchar(5) because we have 5 flags
            "CREATE INDEX IF NOT EXISTS mail_date ON mail(date);",
            "CREATE INDEX IF NOT EXISTS mail_uid ON mail(uid);",
            "CREATE INDEX IF NOT EXISTS mail_flags ON mail(flags);",
            "CREATE INDEX IF NOT EXISTS mail_foreign_key ON mail(mailbox_id);",
        ])
        .await.map_err(|e| {
                tracing::error!("3. {:?}", e);
                e
            })?;
        Ok(Self { db, changes: tx })
    }
    pub async fn next_uid(&self) -> i64 {
        self.biggest_uid_inner().await.map(|i| i + 1).unwrap_or(1)
    }
    async fn biggest_uid_inner(&self) -> Result<i64> {
        i64::try_from(
            self.db
                .execute(Statement::new("SELECT MAX(uid) FROM mail"))
                .await?
                .rows
                .first()
                .context("no rows")?
                .values
                .first()
                .context("no values")?,
        )
        .map_err(|_| anyhow!("couln't parse"))
    }

    pub async fn next_seqnum(&self, mailbox_id: i32) -> i64 {
        self.biggest_seqnum_inner(mailbox_id)
            .await
            .map(|i| i + 1)
            .unwrap_or(1)
    }
    async fn biggest_seqnum_inner(&self, mailbox_id: i32) -> Result<i64> {
        i64::try_from(
            self.db
                .execute(Statement::with_args(
                    "SELECT MAX(seqnum) FROM mail WHERE mailbox_id = ?",
                    args!(mailbox_id),
                ))
                .await?
                .rows
                .first()
                .context("No rows returned")?
                .values
                .first()
                .context("No values returned")?,
        )
        .map_err(|_| anyhow!("couldn't parse"))
    }

    /// Replicates received mail to the database
    pub async fn replicate(
        &self,
        mail: Mail,
        mailbox_id: i32,
        datetime: Option<chrono::DateTime<FixedOffset>>,
    ) -> Result<()> {
        self.changes.send("* 1 EXISTS\r\n".to_owned()).await?;
        let time = if let Some(x) = datetime {
            x.format(crate::parsing::DB_DATETIME_FMT).to_string()
        } else {
            chrono::offset::Utc::now()
                .format(crate::parsing::DB_DATETIME_FMT)
                .to_string()
        };
        let next_uid = self.next_uid().await;
        let next_seqnum = self.next_seqnum(mailbox_id).await;

        self.db
            .execute(Statement::with_args(
                "INSERT INTO mail VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                libsql_client::args!(
                    next_uid as i32,
                    next_seqnum as i32,
                    time,
                    mail.from,
                    mail.to.join(", "),
                    mail.data,
                    mailbox_id,
                    0
                ),
            ))
            .await
            .map(|_| ())
    }

    /// Cleans up old mail
    #[allow(unused)]
    pub async fn delete_old_mail(&self) -> Result<()> {
        //NOTE this will mess up the seqnums
        let now = chrono::offset::Utc::now();
        let a_week_ago = now - chrono::Duration::days(7);
        let a_week_ago = &a_week_ago.format("%Y-%m-%d %H:%M:%S%.3f").to_string();
        tracing::trace!("Deleting old mail from before {a_week_ago}");
        let count = self.mail_count(None).await.unwrap_or(0);
        tracing::debug!("Found {count} old mail");

        self.db
            .execute(Statement::with_args(
                "DELETE FROM mail WHERE date < ?",
                libsql_client::args!(a_week_ago),
            ))
            .await
            .ok();
        Ok(())
    }
    ///mailbox_id is none if you want all mail from all mailboxes
    pub async fn mail_count(&self, mailbox_id: Option<i32>) -> Result<i64> {
        let statement = match mailbox_id {
            Some(x) => Statement::with_args(
                "SELECT COUNT(*) FROM MAIL WHERE mailbox_id = ?",
                libsql_client::args!(x),
            ),
            None => Statement::new("SELECT COUNT(*) FROM mail"),
        };
        i64::try_from(
            self.db
                .execute(statement)
                .await?
                .rows
                .first()
                .context("No rows returned from a COUNT(*) query")?
                .values
                .first()
                .context("No values returned from a COUNT(*) query")?,
        )
        .map_err(|e| anyhow!(e))
    }
    pub async fn get_mailbox_id(&self, user_id: i32, mailbox_name: &str) -> Result<i32> {
        if let Ok(x) = self.get_mailbox_id_no_inbox(user_id, mailbox_name).await {
            return Ok(x);
        }
        if mailbox_name != "INBOX" {
            return Err(anyhow!("no such mailbox: {}", mailbox_name));
        }
        //we need to create the inbox mailbox bc it must exist
        let result = self
            .db
            .execute(Statement::with_args(
                "INSERT INTO mailboxes(name, user_id, flags) VALUES(?, ?, 0)",
                libsql_client::args!(mailbox_name, user_id),
            ))
            .await?;
        if let Some(x) = result.last_insert_rowid {
            Ok(x as i32)
        } else {
            Err(anyhow!("no such mailbox"))
        }
    }
    async fn get_mailbox_id_no_inbox(&self, user_id: i32, mailbox_name: &str) -> Result<i32> {
        let result = self
            .db
            .execute(Statement::with_args(
                "SELECT id FROM mailboxes WHERE user_id = ? AND name = ?",
                libsql_client::args!(user_id, mailbox_name),
            ))
            .await?;
        //fighting the compiler
        let result = result
            .rows
            .first()
            .ok_or(anyhow!("no rows found"))?
            .values
            .first()
            .ok_or(anyhow!("no data found"))?;
        if let Value::Integer { value: x } = result {
            return Ok(*x as i32);
        }
        Err(anyhow!("wrong datatype"))
    }

    ///used with plain auth
    ///if user doesn't exist or the password is incorrect, returns None
    ///otherwise returns the users id
    pub async fn check_user(&self, username: &str, password: &str) -> Option<i32> {
        let values = self
            .db
            .execute(Statement::with_args(
                "SELECT id, password FROM users WHERE name = ?",
                libsql_client::args!(username),
            ))
            .await
            .ok()?;
        //fighting with the compiler
        let mut values = values.rows.first()?.values.iter();
        let id = i32::try_from(values.next()?).ok()?;
        let Value::Blob { value: hash } = values.next()? else {
            return None;
        };
        let hash = std::str::from_utf8(hash).ok()?;

        if !bcrypt::verify(password, hash).ok()? {
            Option::None
        } else {
            Some(id)
        }
    }
    pub async fn get_user_id(&self, username: &str) -> Option<i32> {
        let values = &self
            .db
            .execute(Statement::with_args(
                "SELECT id from users WHERE name = ?",
                libsql_client::args!(username),
            ))
            .await
            .ok()?;
        let values = values.rows.first()?.values.first()?;
        if let Value::Integer { value: x } = values {
            return Some(*x as i32);
        }

        None
    }
    pub async fn create_mailbox(&self, user_id: i32, mailbox_name: &str) -> Result<()> {
        self.db
            .execute(Statement::with_args(
                "INSERT INTO mailboxes(name, user_id, flags) VALUES(?, ?, 0) ",
                libsql_client::args!(mailbox_name, user_id),
            ))
            .await?;

        Ok(())
    }
    pub async fn delete_mailbox(&self, mailbox_id: i32) -> Result<()> {
        self.db
            .execute(Statement::with_args(
                "DELETE FROM mail WHERE mailbox_id = ?",
                libsql_client::args!(mailbox_id),
            ))
            .await?;
        self.db
            .execute(Statement::with_args(
                "DELETE FROM mailboxes WHERE id = ?",
                args!(mailbox_id),
            ))
            .await?;

        Ok(())
    }
    pub async fn rename_mailbox(&self, new_name: &str, mailbox_id: i32) -> Result<()> {
        self.db
            .execute(Statement::with_args(
                "UPDATE mailboxes SET name = ? WHERE id = ?",
                args!(new_name, mailbox_id),
            ))
            .await?;
        Ok(())
    }
    pub async fn get_mailbox_names_for_user(&self, user_id: i32) -> Option<Vec<String>> {
        let result = self
            .db
            .execute(Statement::with_args(
                "SELECT name FROM mailboxes WHERE user_id = ?",
                args!(user_id),
            ))
            .await
            .ok()?;
        let vec = result
            .rows
            .iter()
            .map(|row| row.values.first())
            .flatten()
            .map(|e| {
                if let Value::Text { value: t } = e {
                    Some(t.clone())
                } else {
                    None
                }
            })
            .flatten()
            .collect::<Vec<String>>();
        if vec.is_empty() {
            self.create_mailbox(user_id, "INBOX").await.ok()?;
            Some(vec!["INBOX".to_string()])
        } else {
            Some(vec)
        }
    }
    pub async fn expunge(&self, mailbox_id: i32, uid: Option<(i32, i32)>) -> Result<Vec<i32>> {
        self.changes.send("* 1 EXPUNGE\r\n".to_owned()).await?;
        let deleted = IMAPFlags::Deleted.to_string();
        let statement = if let Some((start, end)) = uid {
            Statement::with_args(
                "DELETE FROM mail WHERE uid BETWEEN ? AND ? AND flags like ? RETURNING seqnum",
                args!(start, end, deleted),
            )
        } else {
            Statement::with_args(
                "DELETE FROM mail WHERE mailbox_id = ? AND flags like ? RETURNING seqnum",
                args!(mailbox_id, deleted),
            )
        };
        let results = self.db.execute(statement).await?;
        //borrow checker issues
        let results = results
            .rows
            .iter()
            .flat_map(|row| row.values.first())
            .collect::<Vec<_>>();
        let results2 = results
            .iter()
            .flat_map(|val| i32::try_from(*val))
            .collect::<Vec<_>>();
        for seqnum in &results2 {
            self.db
                .execute(Statement::with_args(
                    "UPDATE mail SET seqnum = seqnum - 1 WHERE seqnum > ?",
                    args!(*seqnum),
                ))
                .await?;
        }
        let sequence_nums = results2
            .iter()
            .enumerate()
            .map(|(i, val)| *val - i as i32)
            .collect();
        Ok(sequence_nums)
    }

    pub async fn mail_count_with_flags(
        &self,
        mailbox_id: i32,
        flags: Vec<(IMAPFlags, bool)>,
    ) -> Result<i32> {
        let mut flagnum: [char; 5] = ['_'; 5]; //five flags
        for (flag, on) in flags {
            let indicator = if on { '1' } else { '0' };
            flagnum[flag as usize] = indicator;
        }
        tracing::debug!("flagnum is {:?}", flagnum);
        i32::try_from(
            self.db
                .execute(Statement::with_args(
                    "SELECT COUNT(*) FROM mail WHERE mailbox_id = ? AND flags LIKE ?",
                    args!(mailbox_id, flagnum.iter().collect::<String>()),
                ))
                .await?
                .rows
                .first()
                .context("failed count(*) query")?
                .values
                .first()
                .context("failed count(*) query")?,
        )
        .map_err(|e| anyhow!(e))
    }
    pub async fn change_mailbox_subscribed(&self, mailbox_id: i32, subscribed: bool) -> Result<()> {
        let flag = if subscribed { 1 } else { 0 };
        self.db
            .execute(Statement::with_args(
                "UPDATE mailboxes SET flags = ? WHERE id = ?",
                args!(flag, mailbox_id),
            ))
            .await?;
        Ok(())
    }
    pub fn get_search_query(
        search_args: SearchArgs,
        mailbox_id: i32,
        uid: bool,
    ) -> Result<Statement> {
        let db_args: Vec<_> = search_args
            .search_keys
            .iter()
            .map(|i| SearchKeys::to_sql_arg(i, uid))
            .collect();
        let (raw_str, values) = db_args.iter().filter(|(i, _)| !i.is_empty()).fold(
            (String::new(), args!().to_vec()),
            |mut acc, n| {
                acc.1.extend(n.1.clone());
                acc.0.extend(n.0.chars());
                acc.0.extend(" AND ".chars());
                acc
            },
        );
        let requirements = raw_str
            //dirty
            .strip_suffix(" AND ")
            .context("should always happen")?;
        tracing::debug!(
            "db_args: {:?}, values: {:?}, raw_str: {:?}",
            db_args,
            values,
            requirements
        );
        let select = if uid { "uid" } else { "seqnum" };

        let string = format!(
            //test this
            "SELECT {} FROM mail WHERE mailbox_id = {} AND {}",
            select, mailbox_id, requirements
        );
        println!("{}\n{:?}", string, values);
        let stmt = Statement::with_args(string, &values);
        println!("{}", stmt.to_string());
        Ok(stmt)
    }
    pub async fn exec_search_query(
        &self,
        search_args: SearchArgs,
        mailbox_id: i32,
        uid: bool,
    ) -> Result<String> {
        let stmt = Self::get_search_query(search_args.clone(), mailbox_id, uid)?;
        //NOTE get_search_query is seperate for unit tests
        let result = self.db.execute(stmt).await?;
        let str_result = result
            .rows
            .iter()
            .map(|s| s.values.clone())
            .flat_map(|s| {
                s.iter()
                    .filter_map(|i| {
                        if let Value::Integer { value: x } = i {
                            Some(*x as i32)
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<i32>>()
            })
            .collect::<Vec<i32>>();

        let fmt_result = search_args
            .return_opts
            .iter()
            .filter_map(|ret_op| match ret_op {
                ReturnOptions::Min => {
                    if let Some(min) = str_result.iter().min() {
                        Some(format!("MIN {}", min))
                    } else {
                        None
                    }
                }
                ReturnOptions::Max => {
                    if let Some(max) = str_result.iter().max() {
                        Some(format!("MAX {}", max))
                    } else {
                        None
                    }
                }
                ReturnOptions::All => Some(format!(
                    "ALL {}",
                    str_result
                        .iter()
                        .map(i32::to_string)
                        .collect::<Vec<String>>()
                        .join(",")
                )),
                ReturnOptions::Count => Some(format!("COUNT {}", str_result.len())),
                ReturnOptions::Save => Some("".to_owned()),
            })
            .collect::<Vec<String>>()
            .join(" ");
        println!("test_result: {}", fmt_result);
        Ok(fmt_result)
    }
}

//NOTE rethink this
#[repr(u8)]
#[derive(Debug, Clone)]
pub enum IMAPFlags {
    Answered = 1 << 0,
    Flagged = 1 << 1,
    Deleted = 1 << 2,
    Seen = 1 << 3,
    Draft = 1 << 4,
}

impl ToString for IMAPFlags {
    fn to_string(&self) -> String {
        let raw_str = match self {
            IMAPFlags::Answered => "____1",
            IMAPFlags::Flagged => "___1_",
            IMAPFlags::Deleted => "__1__",
            IMAPFlags::Seen => "_1___",
            IMAPFlags::Draft => "1____",
        };
        raw_str.to_string()
    }
}

impl FromStr for IMAPFlags {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> std::prelude::v1::Result<Self, Self::Err> {
        match s {
            "\"\\Answered\"" => Ok(IMAPFlags::Answered),
            "\"\\Flagged\"" => Ok(IMAPFlags::Flagged),
            "\"\\Deleted\"" => Ok(IMAPFlags::Deleted),
            "\"\\Seen\"" => Ok(IMAPFlags::Seen),
            "\"\\Draft\"" => Ok(IMAPFlags::Draft),
            x => Err(anyhow!("invalid flag: {}", x)),
        }
    }
}
