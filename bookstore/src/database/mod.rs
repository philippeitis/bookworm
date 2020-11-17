pub(crate) mod basic_database;
#[cfg(cloud)]
pub(crate) mod google_cloud_database;

pub(crate) use basic_database::{AppDatabase, BasicDatabase, DatabaseError};
#[cfg(cloud)]
pub(crate) use google_cloud_database::CloudDatabase;
