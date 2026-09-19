pub mod books;
pub mod claim;
pub mod collections;
pub mod libraries;
pub mod login;
// router is registered in main.rs by the coordinating agent
#[allow(dead_code)]
pub mod opds_v1;
// router is registered in main.rs by the coordinating agent
#[allow(dead_code)]
pub mod opds_v2;
pub mod readlists;
pub mod referential;
pub mod restriction;
pub mod series;
// router is registered in main.rs by the coordinating agent
#[allow(dead_code)]
pub mod tasks;
pub mod users;
