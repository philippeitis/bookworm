pub(crate) mod database;
#[cfg(cloud)]
pub(crate) mod google_cloud_database;

#[allow(unused_imports)]
pub(crate) use database::{AppDatabase, BasicDatabase, DatabaseError};
#[cfg(cloud)]
pub(crate) use google_cloud_database::CloudDatabase;
