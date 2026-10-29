use crate::command::Command;

pub mod bench_init;
pub mod create_user;
pub mod restore;
pub mod restore_sanitize;
pub mod sanitize;

pub static COMMANDS: [&Command; 5] = [
    &bench_init::COMMAND,
    &restore::COMMAND,
    &sanitize::COMMAND,
    &restore_sanitize::COMMAND,
    &create_user::COMMAND,
];
