use std::str::FromStr;

use crate::{
    imap_op::search::{ReturnOptions, SearchKeys, SequenceSet},
    parsing::{self, imap::SearchArgs},
    smtp_common::Mail,
    utils,
};
use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, FixedOffset};
use fancy_regex::Regex;
use libsql_client::Value;
use rusqlite::*;
use tokio::sync::mpsc::Sender;

pub fn regex_capture(pattern: &str, text: &str, capture_idx: i32) -> Option<String> {
    let re = Regex::new(pattern).ok()?;
    re.captures(text)
        .map(|mat| mat.map(|l| l.get(capture_idx as usize)))
        .ok()?
        .flatten()
        .map(|l| l.as_str().to_string())
}

pub fn rfc2822_to_date(input: &str) -> Result<String> {
    //could make more efficient
    let date = chrono::NaiveDate::parse_from_str(input, super::parsing::MAIL_NAIVE_DATE_FMT)?;
    Ok(date.format(parsing::DB_DATE_FMT).to_string())
}

fn canonical_mailbox_name(input: &str) -> String {
    let trimmed = input.trim_matches('"');
    if trimmed.eq_ignore_ascii_case("INBOX") {
        "INBOX".to_string()
    } else {
        trimmed.to_string()
    }
}

#[derive(Debug, Clone)]
pub struct MailRow {
    pub seqnum: i32,
    pub uid: i32,
    pub date: DateTime<FixedOffset>,
    pub sender: String,
    pub recipients: String,
    pub data: String,
    pub flags: String,
}

#[derive(Debug, Clone)]
pub struct FlagUpdate {
    pub seqnum: i32,
    pub uid: i32,
    pub flags: String,
}

#[derive(Debug, Clone)]
pub struct CopyResult {
    pub source_uid: i32,
    pub dest_uid: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreMode {
    Replace,
    Add,
    Remove,
}

pub struct DBClient {
    db: rusqlite::Connection,
    changes: tokio::sync::mpsc::Sender<String>,
}

impl DBClient {
    pub async fn new(tx: Sender<String>) -> Result<Self> {
        let path = if let Ok(value) = std::env::var("SQLITE_URL") {
            value
        } else {
            tracing::warn!(
                "SQLITE_URL not set, using a default local database: ./data/kakimail/.db"
            );
            "./data/kakimail.db".to_string()
        };
        let db = rusqlite::Connection::open(path)?;

        //safety: trust me bro
        unsafe {
            let _guard = LoadExtensionGuard::new(&db)?;
            //NOTE: need to have sqlite3-pcre installed
            db.load_extension("/usr/lib/sqlite3/pcre.so", None)?;
        }

        db.create_scalar_function(
            "regex_capture",
            3,
            rusqlite::functions::FunctionFlags::SQLITE_UTF8,
            move |ctx| {
                let pattern = ctx.get::<String>(0)?;
                let text = ctx.get::<String>(1)?;
                let capture_idx = ctx.get::<i32>(2)?;
                match regex_capture(&pattern, &text, capture_idx) {
                    Some(result) => Ok(result),
                    None => Ok("".to_string()), // Return an empty string if no match is found
                }
            },
        )?;

        db.create_scalar_function(
            "rfc2822_to_date",
            1,
            rusqlite::functions::FunctionFlags::SQLITE_UTF8,
            move |ctx| {
                let datetime_str = ctx.get::<String>(0)?;
                Ok(rfc2822_to_date(&datetime_str).unwrap_or("".to_owned()))
            },
        )?;

        //USERS TABLE, just in case kakimail-website didn't create it already
        db.execute_batch(
            "PRAGMA foreign_keys = ON;
            CREATE TABLE IF NOT EXISTS users (id INTEGER PRIMARY KEY, name TEXT UNIQUE, password TEXT);
            CREATE INDEX IF NOT EXISTS users_name ON users(name);
            CREATE INDEX IF NOT EXISTS users_id ON users(id);"
        )
        .map_err(|e| {
                tracing::error!("1. {:?}", e);
                e
            })?;

        //MAILBOX TABLE
        db.execute_batch(
            "PRAGMA foreign_keys = ON;
            CREATE TABLE IF NOT EXISTS mailboxes (id integer primary key not null, name text, user_id integer not null, flags integer, FOREIGN KEY(user_id) REFERENCES users(id));
            CREATE INDEX IF NOT EXISTS mailbox_foreign_key ON mailboxes(user_id);
            CREATE INDEX IF NOT EXISTS mailbox_user_name ON mailboxes(user_id, name);
            CREATE INDEX IF NOT EXISTS mailbox_id ON mailboxes(id);"
            // INSERT OR IGNORE INTO mailboxes VALUES (0, 'INBOX', 0, 0)"
            //NOTE: testing only
        )
        .map_err(|e| {
                tracing::error!("2. {:?}", e);
                e
            })?;

        //MAIL TABLE
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS mail (uid integer unique not null, seqnum integer not null, date text, sender text, recipients text, data text, mailbox_id integer not null, flags varchar(5), FOREIGN KEY(mailbox_id) REFERENCES mailboxes(id), PRIMARY KEY(uid));
            CREATE INDEX IF NOT EXISTS mail_date ON mail(date);
            CREATE INDEX IF NOT EXISTS mail_uid ON mail(uid);
            CREATE INDEX IF NOT EXISTS mail_flags ON mail(flags);
            CREATE INDEX IF NOT EXISTS mail_foreign_key ON mail(mailbox_id);"
        )
        .map_err(|e| {
                tracing::error!("3. {:?}", e);
                e
            })?;
        Ok(Self { db, changes: tx })
    }
    pub async fn next_uid(&self) -> i64 {
        self.biggest_uid_inner().await.map(|i| i + 1).unwrap_or(1)
    }
    async fn biggest_uid_inner(&self) -> Result<i64> {
        let x = self
            .db
            .prepare("SELECT MAX(uid) FROM mail")?
            .query(())?
            .next()?
            .context("no rows")?
            .get::<_, i32>(0)?;
        Ok(x as i64)
    }

    pub async fn next_seqnum(&self, mailbox_id: i32) -> i64 {
        self.biggest_seqnum_inner(mailbox_id)
            .await
            .map(|i| i + 1)
            .unwrap_or(1)
    }
    async fn biggest_seqnum_inner(&self, mailbox_id: i32) -> Result<i64> {
        Ok(self
            .db
            .prepare("SELECT MAX(seqnum) FROM mail WHERE mailbox_id = ?1")?
            .query([mailbox_id])?
            .next()?
            .context("no rows")?
            .get::<_, i32>(0)? as i64)
    }

    /// Replicates received mail to the database
    pub async fn replicate(
        &self,
        mail: Mail,
        mailbox_id: i32,
        datetime: Option<chrono::DateTime<FixedOffset>>,
    ) -> Result<i32> {
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
        tracing::info!("replicating mail: {:?}", mail.clone());

        self.db.execute(
            "INSERT INTO mail VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            (
                next_uid as i32,
                next_seqnum as i32,
                time,
                mail.from,
                mail.to.join(", "),
                mail.data,
                mailbox_id,
                "00000",
            ),
        )?;
        Ok(next_uid as i32)
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
            .execute("DELETE FROM mail WHERE date < ?", [a_week_ago])?;
        Ok(())
    }
    ///mailbox_id is none if you want all mail from all mailboxes
    pub async fn mail_count(&self, mailbox_id: Option<i32>) -> Result<i64> {
        let mut y;
        let mut result = match mailbox_id {
            Some(x) => {
                y = self
                    .db
                    .prepare("SELECT COUNT(*) FROM MAIL WHERE mailbox_id = ?")?;
                y.query([x])?
            }

            None => {
                y = self.db.prepare("SELECT COUNT(*) FROM mail")?;
                y.query(())?
            }
        };

        i64::try_from(result.next()?.context("no rows")?.get::<_, i32>(0)?).map_err(|e| anyhow!(e))
    }
    pub async fn get_mailbox_id(&self, user_id: i32, mailbox_name: &str) -> Result<i32> {
        let mailbox_name = canonical_mailbox_name(mailbox_name);
        if let Ok(x) = self.get_mailbox_id_no_inbox(user_id, &mailbox_name).await {
            return Ok(x);
        }
        if mailbox_name != "INBOX" {
            return Err(anyhow!("no such mailbox: {}", mailbox_name));
        }
        tracing::info!("no inbox for user: {user_id}, creating now...");
        //we need to create the inbox mailbox bc it must exist
        let result = self
            .db
            .prepare("INSERT INTO mailboxes(name, user_id, flags) VALUES(?, ?, 0) RETURNING id")?
            .query(rusqlite::params![mailbox_name, user_id])?
            .next()?
            .context("no rows")?
            .get::<_, i32>(0)?;
        Ok(result)
    }
    async fn get_mailbox_id_no_inbox(&self, user_id: i32, mailbox_name: &str) -> Result<i32> {
        let mailbox_name = canonical_mailbox_name(mailbox_name);
        let result = self
            .db
            .prepare("SELECT id FROM mailboxes WHERE user_id = ?1 AND name = ?2")?
            .query_row(rusqlite::params![user_id, mailbox_name], |row| {
                row.get::<_, i32>(0)
            })?;
        Ok(result)
    }

    ///used with plain auth
    ///if user doesn't exist or the password is incorrect, returns None
    ///otherwise returns the users id
    pub async fn check_user(&self, username: &str, password: &str) -> Option<i32> {
        let result = self
            .db
            .prepare("SELECT id, password FROM users WHERE name = ?")
            .ok()?
            .query_row([username], |r| {
                //genius
                Ok(r.get::<_, i32>(0).ok().zip(r.get::<_, Vec<u8>>(1).ok()))
            })
            .ok()??;

        let hash = std::str::from_utf8(&result.1).ok()?;

        if !bcrypt::verify(password, hash).ok()? {
            Option::None
        } else {
            Some(result.0)
        }
    }
    pub async fn get_user_id(&self, username: &str) -> Option<i32> {
        let values = &self
            .db
            .prepare("SELECT id from users WHERE name = ?")
            .ok()?
            .query([username])
            .ok()?
            .next()
            .ok()??
            .get::<_, i32>(0)
            .ok()?;
        return Some(*values);
    }
    pub async fn create_mailbox(&self, user_id: i32, mailbox_name: &str) -> Result<()> {
        let mailbox_name = canonical_mailbox_name(mailbox_name);
        if self
            .get_mailbox_id_no_inbox(user_id, &mailbox_name)
            .await
            .is_ok()
        {
            return Err(anyhow!("Mailbox already exists"));
        }
        let rownum = self
            .db
            .prepare("INSERT INTO mailboxes(name, user_id, flags) VALUES(?, ?, 0)")?
            .execute(params![&mailbox_name, user_id])?;
        if rownum == 0 {
            tracing::warn!("couldn't create mailbox: uid: {user_id}, mbox name: {mailbox_name}");
            Err(anyhow!("Mailbox already exists"))
        } else {
            Ok(())
        }
    }
    pub async fn delete_mailbox(&self, mailbox_id: i32) -> Result<()> {
        self.db
            .prepare("DELETE FROM mail WHERE mailbox_id = ?")?
            .execute([mailbox_id])?;
        self.db
            .prepare("DELETE FROM mailboxes WHERE id = ?")?
            .execute([mailbox_id])?;

        Ok(())
    }
    pub async fn rename_mailbox(&self, new_name: &str, mailbox_id: i32) -> Result<()> {
        let new_name = canonical_mailbox_name(new_name);
        self.db
            .prepare("UPDATE mailboxes SET name = ? WHERE id = ?")?
            .execute(params![new_name, mailbox_id])?;
        Ok(())
    }
    pub async fn get_mailbox_names_for_user(&self, user_id: i32) -> Option<Vec<String>> {
        Some(
            self.get_mailboxes_for_user(user_id)
                .await
                .ok()?
                .into_iter()
                .map(|(name, _)| name)
                .collect(),
        )
    }

    pub async fn get_mailboxes_for_user(&self, user_id: i32) -> Result<Vec<(String, bool)>> {
        let mut result = self
            .db
            .prepare("SELECT name, flags FROM mailboxes WHERE user_id = ? ORDER BY name")?;
        let x = result.query([user_id])?;
        let mut vec = x
            .mapped(|i| Ok((i.get::<_, String>(0)?, i.get::<_, i32>(1)? != 0)))
            .flatten()
            .collect::<Vec<(String, bool)>>();
        if vec.is_empty() {
            tracing::warn!("no mailboxes for uid: {user_id}, creating inbox");
            self.create_mailbox(user_id, "INBOX").await.ok();
            vec.push(("INBOX".to_string(), false));
        }
        Ok(vec)
    }
    pub async fn expunge(&self, mailbox_id: i32, uid: Option<SequenceSet>) -> Result<Vec<i32>> {
        self.changes.send("* 1 EXPUNGE\r\n".to_owned()).await.ok();
        let deleted = IMAPFlags::Deleted.to_string();
        let mut sql =
            "SELECT seqnum, uid FROM mail WHERE mailbox_id = ? AND flags LIKE ?".to_string();
        let mut uid_values = vec![];
        if let Some(uid_set) = uid {
            let (uid_sql, raw_values) = utils::sequence_set_to_sql(uid_set, "uid");
            uid_values = raw_values;
            sql.push_str(" AND ");
            sql.push_str(&uid_sql);
        }
        let mut values: Vec<&dyn ToSql> = vec![&mailbox_id, &deleted];
        values.extend(uid_values.iter().flat_map(value_to_param));
        sql.push_str(" ORDER BY seqnum");

        let rows = self
            .db
            .prepare(&sql)?
            .query_map(values.as_slice(), |row| {
                Ok((row.get::<_, i32>(0)?, row.get::<_, i32>(1)?))
            })?
            .flatten()
            .collect::<Vec<(i32, i32)>>();
        for (_, uid) in &rows {
            self.db
                .prepare("DELETE FROM mail WHERE uid = ?")?
                .execute([*uid])?;
        }
        self.renumber_mailbox(mailbox_id)?;
        Ok(adjust_expunge_sequence_numbers(
            rows.into_iter().map(|(seqnum, _)| seqnum).collect(),
        ))
    }

    pub async fn mail_count_with_flags(
        &self,
        mailbox_id: i32,
        flags: Vec<(IMAPFlags, bool)>,
    ) -> Result<i32> {
        let mut flagnum: [char; 5] = ['_'; 5]; //five flags
        for (flag, on) in flags {
            let indicator = if on { '1' } else { '0' };
            flagnum[flag.index()] = indicator;
        }
        tracing::debug!("flagnum is {:?}", flagnum);
        Ok(self
            .db
            .prepare("SELECT COUNT(*) FROM mail WHERE mailbox_id = ? AND flags LIKE ?")?
            .query(params![mailbox_id, flagnum.iter().collect::<String>()])?
            .next()?
            .context("no rows")?
            .get::<_, i32>(0)?)
    }
    pub async fn status_size(&self, mailbox_id: i32) -> Result<i64> {
        Ok(self
            .db
            .prepare("SELECT COALESCE(SUM(length(data)), 0) FROM mail WHERE mailbox_id = ?")?
            .query([mailbox_id])?
            .next()?
            .context("no rows")?
            .get::<_, i64>(0)?)
    }

    pub fn mailbox_uidvalidity(&self, mailbox_id: i32) -> i32 {
        mailbox_id.max(1)
    }

    pub async fn change_mailbox_subscribed(&self, mailbox_id: i32, subscribed: bool) -> Result<()> {
        let flag = if subscribed { 1 } else { 0 };
        self.db
            .prepare("UPDATE mailboxes SET flags = ? WHERE id = ?")?
            .execute(params![flag, mailbox_id])?;
        Ok(())
    }
    pub fn get_search_query(
        search_args: SearchArgs,
        mailbox_id: i32,
        uid: bool,
    ) -> Result<(String, Vec<Value>)> {
        let (mut raw_str, mut values) = (String::new(), vec![]);
        search_args
            .search_keys
            .iter()
            .map(|i| SearchKeys::to_sql_arg(i, uid))
            .for_each(|i| {
                values.extend(i.1.clone());
                raw_str.extend(i.0.chars());
                raw_str.extend(" AND ".chars());
            });
        let requirements = raw_str.strip_suffix(" AND ").unwrap_or("1 = 1");
        let select = if uid { "uid" } else { "seqnum" };

        let final_str = format!(
            "SELECT {} FROM mail WHERE mailbox_id = {} AND {}",
            select, mailbox_id, requirements
        );
        Ok((final_str, values))
    }
    pub async fn exec_search_query(
        &self,
        search_args: SearchArgs,
        mailbox_id: i32,
        uid: bool,
    ) -> Result<String> {
        //NOTE get_search_query is seperate for unit tests
        let (stmt, values) = Self::get_search_query(search_args.clone(), mailbox_id, uid)?;
        // tracing::debug!("sql search statement is: {}", stmt);

        let values = values
            .iter()
            .flat_map(|i| value_to_param(i))
            .collect::<Vec<_>>();
        let mut result = self.db.prepare(&stmt)?;
        let x = result.query(values.as_slice())?;
        let i32_result = x
            .mapped(|i| i.get::<_, i32>(0))
            .flatten()
            .collect::<Vec<_>>();

        let fmt_result = search_args
            .return_opts
            .iter()
            .filter_map(|ret_op| match ret_op {
                ReturnOptions::Min => {
                    if let Some(min) = i32_result.iter().min() {
                        Some(format!("MIN {}", min))
                    } else {
                        None
                    }
                }
                ReturnOptions::Max => {
                    if let Some(max) = i32_result.iter().max() {
                        Some(format!("MAX {}", max))
                    } else {
                        None
                    }
                }
                ReturnOptions::All => Some(format!(
                    "ALL {}",
                    // i32_result
                    //     .iter()
                    //     .map(i32::to_string)
                    //     .collect::<Vec<String>>()
                    //     .join(",")
                    SequenceSet::from(i32_result.clone()).to_string()
                )),
                ReturnOptions::Count => Some(format!("COUNT {}", i32_result.len())),
                ReturnOptions::Save => Some("".to_owned()),
            })
            .collect::<Vec<String>>()
            .join(" ");
        // println!("test_result: {}", fmt_result);
        Ok(fmt_result)
    }
    ///fetches the raw data from the requested sequence-set or uid-set nums
    pub fn fetch(
        &self,
        sequence_set: SequenceSet,
        uid: bool,
        mailbox_id: i32,
    ) -> Result<Vec<MailRow>> {
        self.select_mail_rows(mailbox_id, sequence_set, uid)
    }

    pub fn store_flags(
        &self,
        mailbox_id: i32,
        sequence_set: SequenceSet,
        by_uid: bool,
        mode: StoreMode,
        flags: &[IMAPFlags],
    ) -> Result<Vec<FlagUpdate>> {
        let rows = self.select_mail_rows(mailbox_id, sequence_set, by_uid)?;
        let mut updates = Vec::new();
        for row in rows {
            let new_flags = apply_flags(&row.flags, flags, mode);
            self.db
                .prepare("UPDATE mail SET flags = ? WHERE uid = ?")?
                .execute(params![new_flags, row.uid])?;
            updates.push(FlagUpdate {
                seqnum: row.seqnum,
                uid: row.uid,
                flags: new_flags,
            });
        }
        Ok(updates)
    }

    pub async fn copy_messages(
        &self,
        src_mailbox_id: i32,
        dest_mailbox_id: i32,
        sequence_set: SequenceSet,
        by_uid: bool,
    ) -> Result<Vec<CopyResult>> {
        let rows = self.select_mail_rows(src_mailbox_id, sequence_set, by_uid)?;
        let mut next_uid = self.next_uid().await;
        let mut next_seqnum = self.next_seqnum(dest_mailbox_id).await;
        let mut copied = Vec::new();
        for row in rows {
            self.db.execute(
                "INSERT INTO mail VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    next_uid as i32,
                    next_seqnum as i32,
                    row.date.format(parsing::DB_DATETIME_FMT).to_string(),
                    row.sender,
                    row.recipients,
                    row.data,
                    dest_mailbox_id,
                    row.flags,
                ],
            )?;
            copied.push(CopyResult {
                source_uid: row.uid,
                dest_uid: next_uid as i32,
            });
            next_uid += 1;
            next_seqnum += 1;
        }
        Ok(copied)
    }

    pub fn delete_messages(
        &self,
        mailbox_id: i32,
        sequence_set: SequenceSet,
        by_uid: bool,
    ) -> Result<Vec<i32>> {
        let rows = self.select_mail_rows(mailbox_id, sequence_set, by_uid)?;
        for row in &rows {
            self.db
                .prepare("DELETE FROM mail WHERE uid = ?")?
                .execute([row.uid])?;
        }
        self.renumber_mailbox(mailbox_id)?;
        Ok(adjust_expunge_sequence_numbers(
            rows.into_iter().map(|row| row.seqnum).collect(),
        ))
    }

    fn select_mail_rows(
        &self,
        mailbox_id: i32,
        sequence_set: SequenceSet,
        by_uid: bool,
    ) -> Result<Vec<MailRow>> {
        let column = if by_uid { "uid" } else { "seqnum" };
        let (sql_str, values) = utils::sequence_set_to_sql(sequence_set, column);
        let mut values = values
            .iter()
            .flat_map(|i| value_to_param(i))
            .collect::<Vec<_>>();
        values.push(&mailbox_id);

        let sql_statement = format!(
            "SELECT seqnum, uid, date, sender, recipients, data, flags FROM mail WHERE {} AND mailbox_id = ? ORDER BY seqnum",
            sql_str
        );
        let mut stmt = self.db.prepare(&sql_statement)?;
        let rows = stmt
            .query_map(values.as_slice(), |row| {
                Ok(MailRow {
                    seqnum: row.get::<_, i32>(0)?,
                    uid: row.get::<_, i32>(1)?,
                    date: row.get::<_, DateTime<FixedOffset>>(2)?,
                    sender: row.get::<_, String>(3)?,
                    recipients: row.get::<_, String>(4)?,
                    data: row.get::<_, String>(5)?,
                    flags: row.get::<_, String>(6)?,
                })
            })?
            .map(|i| i.map_err(|e| e.into()))
            .collect::<Result<Vec<MailRow>>>()?;
        Ok(rows)
    }

    fn renumber_mailbox(&self, mailbox_id: i32) -> Result<()> {
        let uids = self
            .db
            .prepare("SELECT uid FROM mail WHERE mailbox_id = ? ORDER BY seqnum, uid")?
            .query_map([mailbox_id], |row| row.get::<_, i32>(0))?
            .flatten()
            .collect::<Vec<_>>();
        for (idx, uid) in uids.iter().enumerate() {
            self.db
                .prepare("UPDATE mail SET seqnum = ? WHERE uid = ?")?
                .execute(params![(idx + 1) as i32, uid])?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IMAPFlags {
    Answered,
    Flagged,
    Deleted,
    Seen,
    Draft,
}

impl IMAPFlags {
    pub fn index(self) -> usize {
        match self {
            IMAPFlags::Draft => 0,
            IMAPFlags::Seen => 1,
            IMAPFlags::Deleted => 2,
            IMAPFlags::Flagged => 3,
            IMAPFlags::Answered => 4,
        }
    }
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
        match s
            .trim()
            .trim_matches('"')
            .trim_start_matches('\\')
            .to_ascii_lowercase()
            .as_str()
        {
            "answered" => Ok(IMAPFlags::Answered),
            "flagged" => Ok(IMAPFlags::Flagged),
            "deleted" => Ok(IMAPFlags::Deleted),
            "seen" => Ok(IMAPFlags::Seen),
            "draft" => Ok(IMAPFlags::Draft),
            x => Err(anyhow!("invalid flag: {}", x)),
        }
    }
}

fn apply_flags(input: &str, flags: &[IMAPFlags], mode: StoreMode) -> String {
    let mut chars = normalize_flag_string(input);
    match mode {
        StoreMode::Replace => chars = ['0'; 5],
        StoreMode::Add | StoreMode::Remove => {}
    }
    for flag in flags {
        chars[flag.index()] = match mode {
            StoreMode::Remove => '0',
            StoreMode::Replace | StoreMode::Add => '1',
        };
    }
    chars.iter().collect()
}

fn normalize_flag_string(input: &str) -> [char; 5] {
    let mut chars = ['0'; 5];
    for (idx, c) in input.chars().take(5).enumerate() {
        chars[idx] = if c == '1' { '1' } else { '0' };
    }
    chars
}

fn adjust_expunge_sequence_numbers(mut seqnums: Vec<i32>) -> Vec<i32> {
    seqnums.sort_unstable();
    seqnums
        .into_iter()
        .enumerate()
        .map(|(idx, seqnum)| seqnum - idx as i32)
        .collect()
}

pub fn db_flag_to_readable_flag(input: &str) -> String {
    let mut vec = Vec::new();
    for (idx, val) in input.chars().enumerate() {
        if val == '1' {
            match idx {
                0 => vec.push("\\Draft"),
                1 => vec.push("\\Seen"),
                2 => vec.push("\\Deleted"),
                3 => vec.push("\\Flagged"),
                4 => vec.push("\\Answered"),
                _ => {}
            }
        }
    }
    vec.join(" ")
}

//scary
pub fn value_to_param<'a>(value: &'a Value) -> Option<&'a dyn ToSql> {
    match value {
        Value::Null => None,
        Value::Text { value: x } => Some(x),
        Value::Blob { value: x } => Some(x),
        Value::Float { value: x } => Some(x),
        Value::Integer { value: x } => Some(x),
    }
}
