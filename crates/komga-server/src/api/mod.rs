// routers for the M8 misc endpoints are registered in main.rs by the coordinating agent
// actuator is registered in main.rs by the coordinating agent
#[allow(dead_code)]
pub mod actuator;
#[allow(dead_code)]
pub mod announcements;
pub mod books;
pub mod claim;
#[allow(dead_code)]
pub mod client_settings;
pub mod collections;
#[allow(dead_code)]
pub mod filesystem;
#[allow(dead_code)]
pub mod fonts;
#[allow(dead_code)]
pub mod history;
#[allow(dead_code)]
pub mod kobo;
#[allow(dead_code)]
pub mod koreader;
pub mod libraries;
pub mod login;
#[allow(dead_code)]
pub mod oauth2;
#[allow(dead_code)]
pub mod opds_v1;
#[allow(dead_code)]
pub mod opds_v2;
#[allow(dead_code)]
pub mod openapi;
#[allow(dead_code)]
pub mod page_hashes;
pub mod readlists;
pub mod referential;
#[allow(dead_code)]
pub mod releases;
pub mod restriction;
pub mod series;
#[allow(dead_code)]
pub mod settings;
#[allow(dead_code)]
pub mod syncpoints;
#[allow(dead_code)]
pub mod tasks;
#[allow(dead_code)]
pub mod transient_books;
pub mod users;
