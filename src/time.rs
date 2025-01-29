
#[cfg(feature = "native")]
pub fn epoch_time_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[cfg(feature = "web")]
pub fn epoch_time_seconds() -> u64 {
    web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
