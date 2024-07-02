pub mod imap;
//old, only difference is no timezone
// pub const DB_DATETIME_FMT: &'static str = "%Y-%m-%d %H:%M:%S%.3f";
pub const DB_DATETIME_FMT: &'static str = "%F %T%.3f%:z";
pub const DB_DATE_FMT: &'static str = "%Y-%m-%d";
// pub const IMAP_DATETIME_FMT: &'static str = "%d-%b-%y %H:%M:%S %z";
pub const IMAP_DATETIME_FMT: &'static str = "%d-%b-%Y %T %z";
pub const SEARCH_DATE_FMT: &'static str = "%d-%b-%Y";
//we made our own fmt string because in imap, the weekday is optional and we don't want to error
//because of it
// pub const MAIL_DATETIME_FMT: &'static str = "%d %b %Y %T %z";
pub const MAIL_NAIVE_DATE_FMT: &'static str = "%d %b %Y";
